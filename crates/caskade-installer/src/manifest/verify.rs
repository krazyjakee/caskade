use super::{InstallManifest, InstalledFile, ManifestPath, Result, Sha256Digest};
use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read},
    path::Path,
};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyReport {
    pub checked_files: u64,
    pub missing: Vec<ManifestPath>,
    pub changed: Vec<ChangedFile>,
}
impl VerifyReport {
    pub fn is_ok(&self) -> bool {
        self.missing.is_empty() && self.changed.is_empty()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: ManifestPath,
    pub reason: FileChange,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum FileChange {
    NotRegularFile,
    Size {
        expected: u64,
        actual: u64,
    },
    Sha256 {
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    ChangedDuringRead,
    Unreadable {
        message: String,
    },
}

/// Check recorded regular files without mutating the installation. Optional
/// hashes detect equal-size edits; unhashed files are verified by type and size.
/// Unrecorded files are ignored (saves and runtime caches are expected). A root
/// open failure returns an error; individual missing/unreadable files are all
/// collected into the report. Reads cannot escape the game directory through
/// symlinked parents, and a leaf symlink is never accepted as a regular file.
pub fn verify(game_dir: &Path, manifest: &InstallManifest) -> Result<VerifyReport> {
    let directory = Dir::open_ambient_dir(game_dir, cap_std::ambient_authority())?;
    let mut report = VerifyReport {
        checked_files: manifest.record().files.len() as u64,
        missing: Vec::new(),
        changed: Vec::new(),
    };
    for file in &manifest.record().files {
        let reason = match check_file(&directory, file) {
            Ok(reason) => reason,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                report.missing.push(file.path.clone());
                continue;
            }
            Err(error) => Some(FileChange::Unreadable {
                message: error.to_string(),
            }),
        };
        if let Some(reason) = reason {
            report.changed.push(ChangedFile {
                path: file.path.clone(),
                reason,
            });
        }
    }
    report.missing.sort();
    report.changed.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(report)
}

fn check_file(directory: &Dir, expected: &InstalledFile) -> io::Result<Option<FileChange>> {
    if !directory.symlink_metadata(&expected.path)?.is_file() {
        return Ok(Some(FileChange::NotRegularFile));
    }
    let mut file = directory.open(&expected.path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Ok(Some(FileChange::NotRegularFile));
    }
    if metadata.len() != expected.size {
        return Ok(Some(FileChange::Size {
            expected: expected.size,
            actual: metadata.len(),
        }));
    }
    let Some(expected_hash) = &expected.sha256 else {
        return Ok(None);
    };
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut read_bytes = 0_u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        read_bytes += count as u64;
    }
    let after = file.metadata()?;
    if read_bytes != metadata.len()
        || after.len() != metadata.len()
        || after.modified()? != metadata.modified()?
    {
        return Ok(Some(FileChange::ChangedDuringRead));
    }
    let actual = Sha256Digest::new(format!("{:x}", hash.finalize()))
        .expect("formatted SHA-256 is valid hex");
    if actual != *expected_hash {
        return Ok(Some(FileChange::Sha256 {
            expected: expected_hash.clone(),
            actual,
        }));
    }
    Ok(None)
}
