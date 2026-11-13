use super::{
    InnoError, InnoLimits, InnoMetadata, Result, app_path, inspect_path, paths::selected_path,
};
use crate::{
    formats::archive::{ArchiveOptions, ExtractionControl},
    safety::SafeDir,
};
use std::{
    fs::File,
    io::{Read, Seek},
    path::Path,
};

/// Every successful stream reaches EOF so upstream checksum verification runs.
/// Static extraction emits {app} files only; it never replays [Code]/[Run].
pub fn extract_path(
    path: &Path,
    directory: &mut SafeDir,
    options: &ArchiveOptions,
    limits: InnoLimits,
    control: &mut dyn ExtractionControl,
) -> Result<InnoMetadata> {
    control.checkpoint(0)?;
    let metadata = inspect_path(path, limits)?;
    if metadata.password_required {
        return Err(InnoError::PasswordRequired);
    }
    if metadata.files.len() > options.max_entries {
        return Err(InnoError::Limit("file entries"));
    }
    let total = metadata
        .unpacked_bytes()
        .ok_or(InnoError::Limit("unpacked size overflow"))?;
    if total > options.max_unpacked_bytes {
        return Err(InnoError::Limit("unpacked size"));
    }
    // Validate every app path before starting writes, including unselected ones.
    for file in &metadata.files {
        app_path(&file.destination)?;
    }
    let mut builder = globset::GlobSetBuilder::new();
    for glob in &options.select {
        builder.add(globset::Glob::new(glob).map_err(|e| InnoError::Invalid(e.to_string()))?);
    }
    let selected = builder
        .build()
        .map_err(|e| InnoError::Invalid(e.to_string()))?;
    for raw in &metadata.directories {
        if let Some(path) = app_path(raw)? {
            if path == Path::new(".") {
                continue;
            }
            let name = path.to_string_lossy().replace('\\', "/");
            if let Some(name) = selected_path(&name, &selected, options.strip.as_deref())? {
                directory.create_dir(&name, options.depth)?;
            }
        }
    }
    // innospect 0.2 caches decompressed solid chunks, so use it only for bounded
    // embedded payloads. Its external-slice reader is not implemented. inno
    // 0.7's streaming reader is the library fallback for slices/larger chunks.
    if !metadata.external_slices
        && total <= limits.max_cached_chunk_bytes
        && File::open(path)?.metadata()?.len() <= limits.max_metadata_input_bytes
    {
        let bytes = std::fs::read(path)?;
        let installer = innospect::InnoInstaller::from_bytes(&bytes).map_err(invalid)?;
        for file in installer.files() {
            if file.location_index == u32::MAX {
                continue;
            }
            let Some(app) = app_path(&file.destination)? else {
                continue;
            };
            let name = app.to_string_lossy().replace('\\', "/");
            let Some(name) = selected_path(&name, &selected, options.strip.as_deref())? else {
                continue;
            };
            let size = installer
                .file_locations()
                .get(file.location_index as usize)
                .ok_or_else(|| InnoError::Invalid("file location out of range".into()))?
                .original_size;
            control.checkpoint(size)?;
            let reader = installer.extract(file).map_err(invalid)?;
            write(directory, &name, reader, size, options.depth, control)?;
        }
    } else {
        let mut installer = ::inno::Inno::open(path).map_err(invalid)?;
        // Slice names come from the untrusted header. Require a leaf component
        // before the library may resolve sibling setup-N.bin files.
        if metadata.external_slices {
            if let Some(base) = installer.header().base_filename() {
                let safe = crate::safety::sanitize_path(base)?;
                if safe.components().count() != 1 {
                    return Err(InnoError::Unsupported(
                        "slice base is not a filename".into(),
                    ));
                }
            }
            let stem = path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| InnoError::Invalid("invalid installer filename".into()))?;
            crate::safety::sanitize_path(stem)?;
        }
        stream(&mut installer, directory, &selected, options, control)?;
    }
    Ok(metadata)
}
fn invalid(error: impl std::fmt::Display) -> InnoError {
    InnoError::Invalid(error.to_string())
}

fn stream<R: Read + Seek>(
    installer: &mut ::inno::Inno<R>,
    directory: &mut SafeDir,
    selected: &globset::GlobSet,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    let mut files = installer.streaming_files(|entry| {
        entry
            .file()
            .destination()
            .is_some_and(|destination| app_path(destination).ok().flatten().is_some())
    });
    while let Some(file) = files.next() {
        let (entry, reader) = file.map_err(invalid)?;
        let app = app_path(entry.file().destination().unwrap_or_default())?
            .ok_or_else(|| InnoError::Invalid("missing app destination".into()))?;
        let name = app.to_string_lossy().replace('\\', "/");
        let Some(name) = selected_path(&name, selected, options.strip.as_deref())? else {
            // Dropping this reader is supported by inno; the next reader drains
            // skipped solid content. Selected streams reach checksum EOF.
            continue;
        };
        let size = entry.file_location().uncompressed_size();
        control.checkpoint(size)?;
        write(directory, &name, reader, size, options.depth, control)?;
    }
    Ok(())
}
struct Checked<'a, R> {
    inner: R,
    control: &'a mut dyn ExtractionControl,
    pending: u64,
}
impl<R: Read> Read for Checked<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.pending != 0 {
            // SafeDir consumed/wrote the preceding read before requesting more.
            self.control
                .progress(self.pending, 0)
                .map_err(std::io::Error::other)?;
            self.pending = 0;
        }
        self.control.checkpoint(0).map_err(std::io::Error::other)?;
        let count = self.inner.read(buffer)?;
        self.control
            .checkpoint(count as u64)
            .map_err(std::io::Error::other)?;
        self.pending = count as u64;
        Ok(count)
    }
}
fn write(
    directory: &mut SafeDir,
    name: &str,
    reader: impl Read,
    size: u64,
    depth: usize,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    directory.write_file(
        name,
        Checked {
            inner: reader,
            control,
            pending: 0,
        },
        Some(size),
        depth,
        0o644,
    )?;
    control.progress(0, 1)?;
    Ok(())
}
