use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use super::tree::Tree;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Windows,
    Linux,
    Macos,
    Script,
    Native,
}
pub(super) struct BinaryInfo {
    pub kind: Option<Kind>,
    pub gui: bool,
    pub arch: Option<String>,
}

fn empty() -> BinaryInfo {
    BinaryInfo {
        kind: None,
        gui: false,
        arch: None,
    }
}

pub(super) fn inspect(tree: &Tree, path: &Path) -> BinaryInfo {
    let Some(path) = tree.resolve(path) else {
        return empty();
    };
    let Ok(mut file) = tree.open_entry(&path) else {
        return empty();
    };
    parse(&mut file)
}

/// Header reads for many scanned entries (in tree order), spread over a few
/// threads; each keeps its own parent-directory handle, so siblings are
/// opened relative to one directory instead of resolving every full path.
pub(super) fn inspect_all(tree: &Tree, paths: &[&Path]) -> Vec<BinaryInfo> {
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, 2);
    let chunk = paths.len().div_ceil(threads).max(256);
    std::thread::scope(|scope| {
        let workers: Vec<_> = paths
            .chunks(chunk)
            .map(|paths| {
                scope.spawn(move || {
                    let mut parent: Option<(std::path::PathBuf, cap_std::fs::Dir)> = None;
                    paths
                        .iter()
                        .map(|path| {
                            let Some(path) = tree.resolve(path) else {
                                return empty();
                            };
                            let file = match (path.parent(), path.file_name()) {
                                (Some(directory), Some(name))
                                    if !directory.as_os_str().is_empty() =>
                                {
                                    if parent.as_ref().is_none_or(|(open, _)| open != directory) {
                                        parent = tree
                                            .dir()
                                            .open_dir(directory)
                                            .ok()
                                            .map(|handle| (directory.to_path_buf(), handle));
                                    }
                                    parent
                                        .as_ref()
                                        .and_then(|(_, handle)| handle.open(name).ok())
                                }
                                _ => tree.dir().open(&path).ok(),
                            };
                            file.map_or_else(empty, |mut file| parse(&mut file))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("header reader panicked"))
            .collect()
    })
}

fn parse(file: &mut (impl Read + Seek)) -> BinaryInfo {
    let mut info = empty();
    let mut header = [0; 64];
    let Ok(read) = file.read(&mut header) else {
        return info;
    };
    if read >= 4 && header.starts_with(b"\x7fELF") {
        info.kind = Some(Kind::Linux);
        if read >= 20 {
            let machine = if header[5] == 2 {
                u16::from_be_bytes([header[18], header[19]])
            } else {
                u16::from_le_bytes([header[18], header[19]])
            };
            info.arch = match machine {
                3 => Some("x86".into()),
                62 => Some("x86_64".into()),
                40 => Some("arm".into()),
                183 => Some("aarch64".into()),
                _ => None,
            };
        }
    } else if read >= 64 && header.starts_with(b"MZ") {
        let offset = u32::from_le_bytes(header[60..64].try_into().expect("four bytes")) as u64;
        if file.seek(SeekFrom::Start(offset)).is_ok() {
            let mut optional = [0; 96];
            if file.read_exact(&mut optional).is_ok() && optional.starts_with(b"PE\0\0") {
                info.kind = Some(Kind::Windows);
                info.gui = u16::from_le_bytes([optional[92], optional[93]]) == 2;
            }
        }
    } else if read >= 4
        && [
            b"\xfe\xed\xfa\xce",
            b"\xce\xfa\xed\xfe",
            b"\xfe\xed\xfa\xcf",
            b"\xcf\xfa\xed\xfe",
            b"\xca\xfe\xba\xbe",
        ]
        .iter()
        .any(|magic| header.starts_with(*magic))
    {
        info.kind = Some(Kind::Macos);
    } else if read >= 2 && header.starts_with(b"#!") {
        info.kind = Some(Kind::Script);
    }
    info
}

pub(super) fn product_name(tree: &Tree, path: &Path, limit: u64) -> Option<String> {
    let bytes = tree.read(path, limit).ok()?;
    let pe = crate::probe::parse_pe(&bytes)?;
    let resources = pe.resources().ok()?;
    let version = resources.version_info().ok()?;
    for language in version.translation() {
        if let Some(name) = version
            .value(*language, "ProductName")
            .filter(|name| !name.trim().is_empty())
        {
            return Some(name.trim().into());
        }
    }
    None
}
