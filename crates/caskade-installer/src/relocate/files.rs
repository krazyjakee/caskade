//! Durable filesystem operations for moves: copy, verify, rename, rewrite.
use super::RelocateError;
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

pub(super) fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(super) fn rename_durable(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)?;
    if let Some(parent) = from.parent() {
        sync_directory(parent)?;
    }
    if let Some(parent) = to.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temporary = path.with_extension(format!(
        "{}-{}.caskade-move",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        if let Ok(metadata) = fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.sync_all()?;
        drop(file);
        rename_durable(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) fn relink(link: &Path, target: &Path) -> io::Result<()> {
    fs::remove_file(link)?;
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link);
    #[cfg(not(unix))]
    {
        let _ = target;
        Err(io::Error::other("prefix links need a Unix host"))
    }
}

#[cfg(unix)]
pub(super) fn same_volume(a: &Path, b: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::metadata(a)?.dev() == fs::metadata(b)?.dev())
}

#[cfg(not(unix))]
pub(super) fn same_volume(a: &Path, b: &Path) -> io::Result<bool> {
    let volume = |path: &Path| {
        path.components()
            .next()
            .map(|component| component.as_os_str().to_string_lossy().to_ascii_uppercase())
    };
    Ok(volume(a) == volume(b))
}

pub(super) fn tree_size(root: &Path) -> io::Result<u64> {
    let mut total = 0;
    for entry in walk(root)? {
        let metadata = fs::symlink_metadata(&entry)?;
        if metadata.is_file() {
            total += metadata.len();
        }
    }
    Ok(total)
}

/// Every path below `root` (not following links), parents first.
fn walk(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = vec![];
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                pending.push(path.clone());
            }
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

pub(super) fn copy_tree(
    from: &Path,
    to: &Path,
    cancel: &CancellationToken,
    progress: &dyn Fn(u64),
) -> io::Result<()> {
    use std::io::{Read, Write};
    fs::create_dir(to)?;
    let mut copied = 0;
    let mut buffer = vec![0u8; 1024 * 1024];
    for path in walk(from)? {
        if cancel.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "move cancelled"));
        }
        let relative = path.strip_prefix(from).expect("walked below root");
        let target = to.join(relative);
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(&path)?, &target)?;
            #[cfg(windows)]
            {
                let link = fs::read_link(&path)?;
                if path.is_dir() {
                    std::os::windows::fs::symlink_dir(link, &target)?;
                } else {
                    std::os::windows::fs::symlink_file(link, &target)?;
                }
            }
        } else if metadata.is_dir() {
            fs::create_dir(&target)?;
        } else if metadata.is_file() {
            let mut input = fs::File::open(&path)?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            loop {
                if cancel.is_cancelled() {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "move cancelled"));
                }
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                output.write_all(&buffer[..read])?;
                copied += read as u64;
                progress(copied);
            }
            output.sync_all()?;
            fs::set_permissions(&target, metadata.permissions())?;
        } else {
            return Err(io::Error::other(
                "unsupported special file in game directory",
            ));
        }
    }
    #[cfg(unix)]
    {
        let mut directories = walk(to)?
            .into_iter()
            .filter(|path| fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()))
            .collect::<Vec<_>>();
        directories.push(to.into());
        for directory in directories.into_iter().rev() {
            fs::File::open(directory)?.sync_all()?;
        }
    }
    Ok(())
}

pub(super) fn verify_tree(from: &Path, to: &Path) -> Result<(), RelocateError> {
    for path in walk(from)? {
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() {
            continue;
        }
        let relative = path.strip_prefix(from).expect("walked below root");
        let copy = to.join(relative);
        let digest = |path: &Path| -> io::Result<Vec<u8>> {
            let mut hasher = Sha256::new();
            io::copy(&mut fs::File::open(path)?, &mut hasher)?;
            Ok(hasher.finalize().to_vec())
        };
        if fs::metadata(&copy)?.len() != metadata.len() || digest(&path)? != digest(&copy)? {
            return Err(RelocateError::Verify(relative.into()));
        }
    }
    Ok(())
}

pub(super) fn remove_tree(path: &Path) -> io::Result<()> {
    // remove_dir_all never follows symlinks out of the tree.
    fs::remove_dir_all(path)
}
