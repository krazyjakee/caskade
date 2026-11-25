use super::{ManifestError, Result};
use cap_std::fs::Dir;
use std::{
    io::{self, Read},
    path::Path,
};

const MAX_BYTES: u64 = 64 * 1024 * 1024;

fn metadata_directory(root: &Path, create: bool) -> Result<Dir> {
    let directory = Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
    if create {
        match directory.create_dir(".caskade") {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    if !directory.symlink_metadata(".caskade")?.is_dir() {
        return Err(ManifestError::Invalid(
            ".caskade must be a real directory".into(),
        ));
    }
    // Dir's path resolution confines even a raced link replacement to `root`.
    Ok(directory.open_dir(".caskade")?)
}

pub(super) fn read(root: &Path, name: &str) -> Result<Vec<u8>> {
    let directory = metadata_directory(root, false)?;
    if !directory.symlink_metadata(name)?.is_file() {
        return Err(ManifestError::Invalid(
            "manifest must be a regular file".into(),
        ));
    }
    let mut bytes = Vec::new();
    directory
        .open(name)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ManifestError::Invalid(
            "manifest exceeds 64 MiB read limit".into(),
        ));
    }
    Ok(bytes)
}

pub(super) fn write(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 >= MAX_BYTES {
        return Err(ManifestError::Invalid(
            "manifest exceeds 64 MiB write limit".into(),
        ));
    }
    let directory = metadata_directory(root, true)?;
    let mut payload = Vec::with_capacity(bytes.len() + 1);
    payload.extend_from_slice(bytes);
    payload.push(b'\n');
    crate::safety::replace_file(&directory, Path::new(name), &payload, false)?;
    Ok(())
}
