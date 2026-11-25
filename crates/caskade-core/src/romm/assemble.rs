//! Filesystem, hash and archive verification, and ROM assembly helpers for
//! the RomM download cache.
#![cfg_attr(not(feature = "net"), allow(dead_code, unused_imports))]

use super::{RemoteFile, RemoteRom, Result, RommError};
use std::path::{Path, PathBuf};

/// Archives the cache unpacks when the platform doesn't list them.
const ARCHIVES: &[&str] = &["zip", "7z", "tar", "gz", "tgz", "xz", "zst", "bz2"];
/// Disc images and playlists, preferred over the track files they name.
const DISCS: &[&str] = &[
    "m3u", "cue", "gdi", "ccd", "chd", "pbp", "iso", "cso", "rvz", "gcz", "wbfs", "cdi", "mds",
];

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// A hash RomM gives for a file.
#[derive(Debug, Clone, Copy)]
enum HashKind {
    Sha1,
    Md5,
    Crc32,
}

impl HashKind {
    fn name(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA-1",
            Self::Md5 => "MD5",
            Self::Crc32 => "CRC32",
        }
    }
}

/// The strongest hash the server gave, lowercase.
fn strongest(expected: &RemoteFile) -> Option<(HashKind, String)> {
    let wanted = |hash: &Option<String>| {
        hash.as_deref()
            .map(str::trim)
            .filter(|hash| !hash.is_empty())
            .map(str::to_ascii_lowercase)
    };
    wanted(&expected.sha1_hash)
        .map(|hash| (HashKind::Sha1, hash))
        .or_else(|| wanted(&expected.md5_hash).map(|hash| (HashKind::Md5, hash)))
        .or_else(|| wanted(&expected.crc_hash).map(|hash| (HashKind::Crc32, hash)))
}

/// The hash of `paths`' bytes, one after the other.
fn digest(kind: HashKind, paths: &[PathBuf]) -> Result<String> {
    use std::io::Read;
    let mut buffer = vec![0u8; 1 << 20];
    let mut feed = |update: &mut dyn FnMut(&[u8])| -> std::io::Result<()> {
        for path in paths {
            let mut file = std::fs::File::open(path)?;
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                update(&buffer[..read]);
            }
        }
        Ok(())
    };
    Ok(match kind {
        HashKind::Sha1 => {
            use sha1::Digest;
            let mut hasher = sha1::Sha1::new();
            feed(&mut |bytes| hasher.update(bytes))?;
            format!("{:x}", hasher.finalize())
        }
        HashKind::Md5 => {
            use md5::Digest;
            let mut hasher = md5::Md5::new();
            feed(&mut |bytes| hasher.update(bytes))?;
            format!("{:x}", hasher.finalize())
        }
        HashKind::Crc32 => {
            let mut hasher = crc32fast::Hasher::new();
            feed(&mut |bytes| hasher.update(bytes))?;
            format!("{:08x}", hasher.finalize())
        }
    })
}

fn same(want: &str, got: &str) -> bool {
    want.trim_start_matches('0') == got.trim_start_matches('0')
}

fn mismatch(kind: HashKind, expected: &RemoteFile) -> RommError {
    RommError::Verify(crate::tr!(
        "{checksum} for {file}",
        checksum = kind.name(),
        file = expected.file_name
    ))
}

/// Archive members RomM leaves out of its hashes (`DEFAULT_EXCLUDED_FILES`
/// and `DEFAULT_EXCLUDED_EXTENSIONS` in its config).
fn excluded_member(path: &Path) -> bool {
    const NAMES: [&str; 7] = [
        ".DS_Store",
        ".localized",
        ".Trashes",
        ".stfolder",
        "@SynoResource",
        "gamelist.xml",
        "metadata.pegasus.txt",
    ];
    const EXTENSIONS: [&str; 9] = [
        "db",
        "tmp",
        "bak",
        "lock",
        "log",
        "cache",
        "crdownload",
        "part",
        "assembling",
    ];
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let lower = name.to_lowercase();
    NAMES.contains(&name.as_ref())
        || name.ends_with(":Zone.Identifier")
        || EXTENSIONS
            .iter()
            .any(|extension| lower.ends_with(&format!(".{extension}")))
}

/// Whether an archive's unpacked contents in `dir` have RomM's hash. RomM
/// hashes what an archive holds, not the archive: every member in path
/// order (RomM 4 and later), or the largest member (earlier versions).
fn contents_match(dir: &Path, kind: HashKind, want: &str) -> Result<bool> {
    let mut members: Vec<(String, PathBuf)> = walk(dir)?
        .into_iter()
        .filter(|path| !excluded_member(path))
        .map(|path| {
            let name = path
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            (name, dir.join(path))
        })
        .collect();
    if members.is_empty() {
        return Ok(false);
    }
    // Python sorts member names by code point; UTF-8 bytes sort the same.
    members.sort_by(|a, b| a.0.cmp(&b.0));
    let paths: Vec<PathBuf> = members.iter().map(|(_, path)| path.clone()).collect();
    if same(want, &digest(kind, &paths)?) {
        return Ok(true);
    }
    let size = |path: &PathBuf| std::fs::metadata(path).map_or(0, |m| m.len());
    let largest = paths
        .iter()
        .fold(None::<&PathBuf>, |best, path| match best {
            Some(best) if size(best) >= size(path) => Some(best),
            _ => Some(path),
        });
    match largest {
        Some(largest) if paths.len() > 1 => {
            Ok(same(want, &digest(kind, std::slice::from_ref(largest))?))
        }
        _ => Ok(false),
    }
}

fn check_size(path: &Path, expected: &RemoteFile, size: Option<u64>) -> Result<()> {
    let actual = std::fs::metadata(path)?.len();
    if let Some(size) = size.filter(|size| *size > 0)
        && size != actual
    {
        return Err(RommError::Verify(crate::tr!(
            "size for {file} ({actual} bytes, expected {size})",
            file = expected.file_name,
            actual = actual,
            size = size
        )));
    }
    Ok(())
}

/// Checks a file against the size and the strongest hash the server gave.
/// An archive whose own bytes don't have the hash is unpacked beside it
/// and checked by its contents, as RomM hashes them.
fn verify(path: &Path, expected: &RemoteFile, size: Option<u64>) -> Result<()> {
    check_size(path, expected, size)?;
    let Some((kind, want)) = strongest(expected) else {
        return Ok(());
    };
    if same(&want, &digest(kind, &[path.to_path_buf()])?) {
        return Ok(());
    }
    if ARCHIVES.contains(&extension(Path::new(&expected.file_name)).as_str()) {
        let mut check = path.as_os_str().to_owned();
        check.push(".contents");
        let check = PathBuf::from(check);
        let matched = unpack(path, &check).and_then(|()| contents_match(&check, kind, &want));
        let _ = std::fs::remove_dir_all(&check);
        match matched {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            // Out of space, refused or a drive gone: say so rather than
            // blame the download, which would only be fetched again (#160).
            Err(error) if environmental(&error) => return Err(error),
            // An archive that won't unpack doesn't match.
            Err(_) => {}
        }
    }
    Err(mismatch(kind, expected))
}

/// Whether `error` is about the machine (space, access, a drive), not the
/// file's contents.
fn environmental(error: &RommError) -> bool {
    use crate::error::{ErrorCode, ErrorReport};
    matches!(
        ErrorReport::from_error(error).code,
        ErrorCode::DiskFull | ErrorCode::AccessDenied | ErrorCode::StorageOffline | ErrorCode::Busy
    )
}

/// Every regular file under `dir`, relative to it, without following links.
fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        for item in std::fs::read_dir(dir.join(&relative))? {
            let item = item?;
            let kind = item.file_type()?;
            let path = relative.join(item.file_name());
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                files.push(path);
            }
            if files.len() > 100_000 {
                return Err(RommError::Extract(crate::tr!("too many files")));
            }
        }
    }
    files.sort();
    Ok(files)
}

fn unpack(archive: &Path, into: &Path) -> Result<()> {
    crate::managed::unpack::extract_into(archive, into)?.map_err(RommError::Extract)
}

/// What the emulator opens, relative to the ROM's folder: a playlist, the
/// disc images (an `.m3u` is written for several when the platform takes
/// one), or the files the platform runs.
fn launch_files(dir: &Path, rom: &RemoteRom, extensions: &[String]) -> Result<Vec<PathBuf>> {
    let files = walk(dir)?;
    let supports = |extension: &str| extensions.iter().any(|known| known == extension);
    // Shallowest first.
    let by_depth = |mut list: Vec<PathBuf>| {
        list.sort_by_key(|path| (path.components().count(), path.clone()));
        list
    };
    let playlists = by_depth(
        files
            .iter()
            .filter(|path| extension(path) == "m3u")
            .cloned()
            .collect(),
    );
    if supports("m3u")
        && let Some(playlist) = playlists.first()
    {
        return Ok(vec![playlist.clone()]);
    }
    let playable: Vec<PathBuf> = files
        .iter()
        .filter(|path| extensions.is_empty() || supports(&extension(path)))
        .cloned()
        .collect();
    let discs: Vec<PathBuf> = playable
        .iter()
        .filter(|path| DISCS.contains(&extension(path).as_str()))
        .cloned()
        .collect();
    let chosen = if discs.is_empty() { playable } else { discs };
    if chosen.is_empty() {
        // Nothing the platform names: the largest file.
        let largest = files
            .iter()
            .max_by_key(|path| std::fs::metadata(dir.join(path)).map_or(0, |m| m.len()))
            .cloned()
            .ok_or_else(|| RommError::Extract(crate::tr!("the download has no files")))?;
        return Ok(vec![largest]);
    }
    if chosen.len() > 1 && supports("m3u") {
        let stem = if rom.fs_name_no_ext.trim().is_empty() {
            "game".to_owned()
        } else {
            rom.fs_name_no_ext.replace(['/', '\\'], "_")
        };
        let playlist = PathBuf::from(format!("{stem}.m3u"));
        let text: String = chosen
            .iter()
            .map(|path| format!("{}\n", path.to_string_lossy().replace('\\', "/")))
            .collect();
        std::fs::write(dir.join(&playlist), text)?;
        return Ok(vec![playlist]);
    }
    Ok(chosen)
}

/// Turns `download.part` into the finished ROM folder `files/`, verified
/// and unpacked when needed. Returns what the emulator opens and the size.
pub(super) fn assemble(
    part: &Path,
    files: &Path,
    rom: &RemoteRom,
    extensions: &[String],
    check: bool,
) -> Result<(Vec<PathBuf>, u64)> {
    if files.exists() {
        std::fs::remove_dir_all(files)?;
    }
    let name = Path::new(&rom.fs_name)
        .file_name()
        .map(PathBuf::from)
        .ok_or_else(|| RommError::Server(crate::tr!("the ROM has no file name")))?;
    let archive = ARCHIVES.contains(&extension(&name).as_str())
        && !extensions.iter().any(|known| *known == extension(&name));
    // The extractor recognises archives by content, but give it a plain
    // archive name rather than a `.part`.
    let unpack_part = |kind: &str| -> Result<()> {
        let archive = part.with_file_name(format!("download.{kind}"));
        std::fs::rename(part, &archive)?;
        let unpacked = unpack(&archive, files);
        let _ = std::fs::remove_file(&archive);
        unpacked
    };
    if rom.is_multi() {
        // The server zips the ROM's files.
        unpack_part("zip")?;
        if check {
            let found = walk(files)?;
            for expected in &rom.files {
                let candidates: Vec<&PathBuf> = found
                    .iter()
                    .filter(|path| {
                        path.file_name()
                            .is_some_and(|name| name.to_string_lossy() == expected.file_name)
                    })
                    .collect();
                let path = candidates
                    .iter()
                    .find(|path| {
                        std::fs::metadata(files.join(path))
                            .is_ok_and(|m| m.len() == expected.file_size_bytes)
                    })
                    .or(candidates.first())
                    .ok_or_else(|| {
                        RommError::Verify(crate::tr!(
                            "file list ({file} is missing)",
                            file = expected.file_name
                        ))
                    })?;
                verify(&files.join(path), expected, Some(expected.file_size_bytes))?;
            }
        }
    } else {
        if check {
            let expected = rom.files.first().cloned().unwrap_or_else(|| RemoteFile {
                file_name: rom.fs_name.clone(),
                file_size_bytes: rom.fs_size_bytes,
                crc_hash: rom.crc_hash.clone(),
                md5_hash: rom.md5_hash.clone(),
                sha1_hash: rom.sha1_hash.clone(),
                ..Default::default()
            });
            let size = Some(rom.fs_size_bytes).filter(|size| *size > 0);
            // An archive is checked by its contents once unpacked, unless
            // its own bytes have the hash (older RomM servers).
            let checked = if archive {
                check_size(part, &expected, size).map(|()| {
                    strongest(&expected).filter(|(kind, want)| {
                        !digest(*kind, &[part.to_path_buf()]).is_ok_and(|got| same(want, &got))
                    })
                })
            } else {
                verify(part, &expected, size).map(|()| None)
            };
            match checked {
                Ok(Some((kind, want))) => {
                    unpack_part(&extension(&name))?;
                    if !contents_match(files, kind, &want)? {
                        let _ = std::fs::remove_dir_all(files);
                        return Err(mismatch(kind, &expected));
                    }
                    return finish(files, rom, extensions);
                }
                Ok(None) => {}
                Err(error) => {
                    // Start over next time rather than resume a bad file.
                    let _ = std::fs::remove_file(part);
                    return Err(error);
                }
            }
        }
        if archive {
            unpack_part(&extension(&name))?;
        } else {
            std::fs::create_dir_all(files)?;
            std::fs::rename(part, files.join(&name))?;
        }
    }
    finish(files, rom, extensions)
}

/// What the emulator opens in the ROM's folder, and the folder's size.
fn finish(files: &Path, rom: &RemoteRom, extensions: &[String]) -> Result<(Vec<PathBuf>, u64)> {
    let roms = launch_files(files, rom, extensions)?;
    let size = walk(files)?
        .iter()
        .map(|path| std::fs::metadata(files.join(path)).map_or(0, |m| m.len()))
        .sum();
    Ok((roms, size))
}
