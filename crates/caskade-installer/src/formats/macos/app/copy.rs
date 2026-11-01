use super::{AppError, Result, inspect_bundle};
use crate::{
    formats::archive::ExtractionControl,
    safety::{SafeDir, sanitize_path},
};
use cap_std::{
    ambient_authority,
    fs::{Dir, Metadata},
};
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
};

#[cfg(unix)]
fn mode(metadata: &Metadata) -> u32 {
    use cap_std::fs::PermissionsExt;
    metadata.permissions().mode()
}
#[cfg(not(unix))]
fn mode(_metadata: &Metadata) -> u32 {
    0o755
}

pub fn copy_bundle(
    source: &Path,
    destination: &mut SafeDir,
    control: &mut dyn ExtractionControl,
) -> Result<PathBuf> {
    inspect_bundle(source)?;
    let name = source
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or(crate::safety::SafetyError::InvalidEncoding)?;
    let bundle = sanitize_path(name)?;
    let root = Dir::open_ambient_dir(source, ambient_authority())?;
    destination.create_dir(name, 0)?;
    let mut copier = Copier {
        source: &root,
        destination,
        control,
        links: vec![],
        dirs: vec![],
        count: 0,
        metadata_bytes: 0,
    };
    copier
        .dirs
        .push((bundle.clone(), mode(&root.dir_metadata()?)));
    copier.attributes(Path::new("."), &bundle)?;
    copier.walk(Path::new("."), &bundle)?;
    let mut pending = std::mem::take(&mut copier.links);
    while !pending.is_empty() {
        let mut retry = Vec::new();
        let mut progress = false;
        for (path, target) in pending {
            copier.control.checkpoint(0)?;
            match copier
                .destination
                .symlink(&path.to_string_lossy(), &target.to_string_lossy(), 0)
            {
                Ok(_) => {
                    #[cfg(target_os = "macos")]
                    copier.symlink_attributes(
                        path.strip_prefix(&bundle)
                            .map_err(|_| AppError::Invalid("bundle link prefix".into()))?,
                        &path,
                    )?;
                    copier.control.progress(0, 1)?;
                    progress = true;
                }
                Err(crate::safety::SafetyError::InvalidSymlinkTarget) => retry.push((path, target)),
                Err(error) => return Err(error.into()),
            }
        }
        if !progress && !retry.is_empty() {
            return Err(crate::safety::SafetyError::InvalidSymlinkTarget.into());
        }
        pending = retry;
    }
    // Apply restrictive directory modes only once their children are complete.
    copier
        .dirs
        .sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
    for (path, mode) in copier.dirs {
        copier
            .destination
            .set_entry_mode(&path.to_string_lossy(), mode)?;
    }
    Ok(bundle)
}
struct Copier<'a> {
    source: &'a Dir,
    destination: &'a mut SafeDir,
    control: &'a mut dyn ExtractionControl,
    links: Vec<(PathBuf, PathBuf)>,
    dirs: Vec<(PathBuf, u32)>,
    count: u64,
    metadata_bytes: usize,
}
impl Copier<'_> {
    fn account(&mut self, bytes: usize) -> Result<()> {
        self.metadata_bytes = self
            .metadata_bytes
            .checked_add(bytes)
            .ok_or(AppError::MetadataLimit)?;
        if self.metadata_bytes > 16 * 1024 * 1024 {
            return Err(AppError::MetadataLimit);
        }
        Ok(())
    }
    fn walk(&mut self, source: &Path, destination: &Path) -> Result<()> {
        if source.components().count() > 128 {
            return Err(crate::safety::SafetyError::DepthLimit.into());
        }
        let mut entries = Vec::new();
        for entry in self.source.read_dir(source)? {
            if entries.len() as u64 + self.count >= 100_000 {
                return Err(crate::safety::SafetyError::EntryLimit.into());
            }
            entries.push(entry?);
        }
        entries.sort_by_key(|entry| entry.file_name());
        let mut names = BTreeSet::new();
        for entry in entries {
            self.control.checkpoint(0)?;
            self.count += 1;
            if self.count > 100_000 {
                return Err(crate::safety::SafetyError::EntryLimit.into());
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| crate::safety::SafetyError::InvalidEncoding)?;
            let name = sanitize_path(&name)?;
            if name.components().count() != 1 || !names.insert(name.clone()) {
                return Err(AppError::Invalid("ambiguous bundle child".into()));
            }
            let from = source.join(&name);
            let to = destination.join(name);
            self.account(to.as_os_str().len())?;
            let metadata = self.source.symlink_metadata(&from)?;
            if metadata.is_symlink() {
                let target = self.source.read_link(&from)?;
                self.account(target.as_os_str().len())?;
                self.links.push((to, target));
            } else if metadata.is_dir() {
                self.destination.create_dir(&to.to_string_lossy(), 0)?;
                self.dirs.push((to.clone(), mode(&metadata)));
                self.attributes(&from, &to)?;
                self.walk(&from, &to)?;
                self.control.progress(0, 1)?;
            } else if metadata.is_file() {
                let file = self.source.open(&from)?;
                let mut reader = ProgressReader {
                    inner: file,
                    control: self.control,
                    pending: 0,
                };
                self.destination.write_file(
                    &to.to_string_lossy(),
                    &mut reader,
                    Some(metadata.len()),
                    0,
                    0o600,
                )?;
                let remaining = reader.pending;
                self.control.progress(remaining, 1)?;
                self.attributes(&from, &to)?;
                self.destination
                    .set_entry_mode(&to.to_string_lossy(), mode(&metadata))?;
            } else {
                return Err(crate::safety::SafetyError::SpecialFile.into());
            }
        }
        Ok(())
    }
    #[cfg(unix)]
    fn attributes(&mut self, source: &Path, destination: &Path) -> Result<()> {
        use xattr::FileExt;
        // Dir may hold an O_PATH descriptor; reopen for metadata operations
        // that require a readable descriptor (flistxattr/fgetxattr).
        let file = self.source.open(source)?.into_std();
        let mut bytes = 0_usize;
        for (count, name) in file.list_xattr()?.enumerate() {
            if count >= 1024 {
                return Err(AppError::MetadataLimit);
            }
            if let Some(value) = file.get_xattr(&name)? {
                bytes = bytes
                    .checked_add(value.len())
                    .ok_or(AppError::MetadataLimit)?;
                if bytes > 1024 * 1024 {
                    return Err(AppError::MetadataLimit);
                }
                self.destination
                    .set_entry_xattr(&destination.to_string_lossy(), &name, &value)?;
            }
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    fn symlink_attributes(&mut self, source: &Path, destination: &Path) -> Result<()> {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
        use cap_std::fs::{OpenOptions, OpenOptionsExt};
        use xattr::FileExt;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .follow(FollowSymlinks::No)
            .custom_flags(rustix::fs::OFlags::SYMLINK.bits() as i32);
        let file = self.source.open_with(source, &options)?.into_std();
        let mut bytes = 0_usize;
        for (count, name) in file.list_xattr()?.enumerate() {
            if count >= 1024 {
                return Err(AppError::MetadataLimit);
            }
            if let Some(value) = file.get_xattr(&name)? {
                bytes = bytes
                    .checked_add(value.len())
                    .ok_or(AppError::MetadataLimit)?;
                if bytes > 1024 * 1024 {
                    return Err(AppError::MetadataLimit);
                }
                self.destination.set_symlink_xattr(
                    &destination.to_string_lossy(),
                    &name,
                    &value,
                )?;
            }
        }
        Ok(())
    }
    #[cfg(not(unix))]
    fn attributes(&mut self, _source: &Path, _destination: &Path) -> Result<()> {
        Ok(())
    }
}
struct ProgressReader<'a, R> {
    inner: R,
    control: &'a mut dyn ExtractionControl,
    pending: u64,
}
impl<R: Read> Read for ProgressReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = buffer.len().min(64 * 1024);
        self.control
            .checkpoint(count as u64)
            .map_err(std::io::Error::other)?;
        let n = self.inner.read(&mut buffer[..count])?;
        self.pending += n as u64;
        if self.pending >= 64 * 1024 {
            self.control
                .progress(self.pending, 0)
                .map_err(std::io::Error::other)?;
            self.pending = 0;
        }
        Ok(n)
    }
}

pub(super) fn measure(source: &Path) -> Result<u64> {
    fn walk(root: &Dir, path: &Path, count: &mut u64, depth: usize) -> Result<u64> {
        if depth > 128 {
            return Err(crate::safety::SafetyError::DepthLimit.into());
        }
        let mut size = 0_u64;
        for entry in root.read_dir(path)? {
            let entry = entry?;
            *count += 1;
            if *count > 100_000 {
                return Err(crate::safety::SafetyError::EntryLimit.into());
            }
            let child = path.join(entry.file_name());
            let metadata = root.symlink_metadata(&child)?;
            let bytes = if metadata.is_file() {
                metadata.len()
            } else if metadata.is_dir() {
                walk(root, &child, count, depth + 1)?
            } else {
                0
            };
            size = size
                .checked_add(bytes)
                .ok_or(crate::safety::SafetyError::ByteLimit)?;
        }
        Ok(size)
    }
    let root = Dir::open_ambient_dir(source, ambient_authority())?;
    walk(&root, Path::new("."), &mut 0, 0)
}
