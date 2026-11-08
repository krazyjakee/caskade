//! BIOS status (#122): which of a platform's and
//! profile's BIOS files exist, and whether their MD5 matches. Files are
//! looked up in the platform's BIOS folder override, otherwise in the
//! emulator's BIOS folder: its definition's `bios` variable, or `system`
//! and `bios` next to the emulator.

use super::{Bios, InstallKind, VarContext, Vars, vars::Expander};
use caskade_installer::OperatingSystem;
use md5::{Digest, Md5};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum BiosState {
    Found,
    Missing,
    /// A file is there, but not the dump the definition names.
    WrongHash,
}

/// BIOS files bigger than this are not hashed (firmware packages).
const HASH_LIMIT: u64 = 64 * 1024 * 1024;

/// Folders a BIOS file is looked up in, first match wins.
pub fn bios_dirs(
    override_dir: Option<&Path>,
    vars: &Vars,
    os: OperatingSystem,
    kind: InstallKind,
    context: &VarContext,
) -> Vec<PathBuf> {
    if let Some(dir) = override_dir {
        return vec![dir.to_path_buf()];
    }
    let expander = Expander {
        vars,
        os,
        kind,
        context,
    };
    if vars.get("bios", os, kind).is_some()
        && let Ok(dir) = expander.expand("{bios}")
        && !dir.is_empty()
    {
        return vec![PathBuf::from(dir)];
    }
    context
        .emulator_dir
        .iter()
        .flat_map(|dir| [dir.join("system"), dir.join("bios")])
        .collect()
}

/// A file's size and time when it was hashed, and its MD5.
type Hashed = (u64, Option<SystemTime>, String);

/// MD5s of BIOS files, kept while the file's size and time don't change.
#[derive(Default)]
pub struct BiosHashes {
    known: Mutex<HashMap<PathBuf, Hashed>>,
}

impl BiosHashes {
    fn md5(&self, path: &Path) -> Option<String> {
        let metadata = std::fs::metadata(path).ok()?;
        if metadata.len() > HASH_LIMIT {
            return None;
        }
        let modified = metadata.modified().ok();
        let mut known = crate::lock(&self.known);
        if let Some((size, time, hash)) = known.get(path)
            && *size == metadata.len()
            && *time == modified
        {
            return Some(hash.clone());
        }
        drop(known);
        let mut file = std::fs::File::open(path).ok()?;
        let mut hasher = Md5::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).ok()?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let hash = format!("{:x}", hasher.finalize());
        known = crate::lock(&self.known);
        known.insert(path.to_path_buf(), (metadata.len(), modified, hash.clone()));
        Some(hash)
    }

    /// The state of one BIOS file and where it was found.
    pub fn check(&self, bios: &Bios, dirs: &[PathBuf]) -> (BiosState, Option<PathBuf>) {
        let mut wrong = None;
        for dir in dirs {
            let path = dir.join(&bios.file);
            if !path.is_file() {
                continue;
            }
            match &bios.md5 {
                Some(expected) => match self.md5(&path) {
                    Some(actual) if actual.eq_ignore_ascii_case(expected) => {
                        return (BiosState::Found, Some(path));
                    }
                    // Too big to hash: present is the best we know.
                    None => return (BiosState::Found, Some(path)),
                    Some(_) => wrong = wrong.or(Some(path)),
                },
                None => return (BiosState::Found, Some(path)),
            }
        }
        match wrong {
            Some(path) => (BiosState::WrongHash, Some(path)),
            None => (BiosState::Missing, None),
        }
    }
}
