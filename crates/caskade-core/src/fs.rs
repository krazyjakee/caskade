//! Atomic file replacement and free folder naming helpers.

use cap_std::fs::{Dir, OpenOptions};
#[cfg(feature = "net")]
use std::path::PathBuf;
use std::{
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn next_temp_name() -> String {
    format!(
        ".tmp-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    )
}

/// Atomically writes `bytes` to `name` inside capability directory `dir`.
///
/// Uses a unique `create_new` temporary file, flushes with `sync_all`, removes
/// the temp file on failure, and syncs the parent directory on Unix.
pub fn atomic_write_dir(dir: &Dir, name: &str, bytes: &[u8]) -> io::Result<()> {
    let mut candidate = None;
    for _ in 0..100 {
        let temp = next_temp_name();
        match dir.open_with(&temp, OpenOptions::new().write(true).create_new(true)) {
            Ok(file) => {
                candidate = Some((temp, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let (temporary, mut file) = candidate.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "temporary file namespace exhausted",
        )
    })?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(windows)]
        if dir.try_exists(name)? {
            dir.remove_file(name)?;
        }
        dir.rename(&temporary, dir, name)?;
        #[cfg(unix)]
        dir.open(".")?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = dir.remove_file(&temporary);
    }
    result
}

/// Atomically writes `bytes` to `path`, creating parent directories as needed.
///
/// Uses a unique `create_new` temporary file in `path`'s parent directory,
/// flushes with `sync_all`, cleans up on failure, and syncs the parent
/// directory on Unix.
pub fn atomic_write_path(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(parent)?;
    let mut candidate = None;
    for _ in 0..100 {
        let temp = parent.join(next_temp_name());
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => {
                candidate = Some((temp, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let (temporary, mut file) = candidate.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "temporary file namespace exhausted",
        )
    })?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(windows)]
        if path.try_exists()? {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(&temporary, path)?;
        #[cfg(unix)]
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// `stem`, or `stem (2)`, `stem (3)`, …: the first that `parent` hasn't got.
#[cfg(feature = "net")]
pub(crate) fn free_folder(parent: &Path, stem: &str) -> crate::Result<PathBuf> {
    for n in 1..1000 {
        let candidate = if n == 1 {
            stem.to_owned()
        } else {
            format!("{stem} ({n})")
        };
        if !parent.join(&candidate).exists() {
            return Ok(PathBuf::from(candidate));
        }
    }
    Err(crate::Error::Invalid(crate::tr!(
        "no free folder name for it"
    )))
}
