use super::*;
use crate::{
    ArtifactId,
    exec::{ExecError, Extractor, StepContext},
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Seek},
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct MsiExtractor {
    sources: BTreeMap<ArtifactId, PathBuf>,
    limits: MsiLimits,
}
impl MsiExtractor {
    pub fn new(sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self {
            sources,
            limits: MsiLimits::default(),
        }
    }
    pub fn with_limits(mut self, limits: MsiLimits) -> Self {
        self.limits = limits;
        self
    }
}
impl Extractor for MsiExtractor {
    fn extract(
        &self,
        artifact: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> crate::exec::Result<()> {
        if !select.is_empty() || strip.is_some() {
            return Err(ExecError::UnsupportedStep {
                step: "MSI select/strip (table paths already relative to application root)".into(),
            });
        }
        let path = crate::exec::source_path(&self.sources, artifact)?;
        let result = extract(path, destination, context, self.limits);
        match result {
            Ok(()) => Ok(()),
            Err(MsiError::Io(e)) => {
                context.checkpoint(0)?;
                Err(ExecError::Io(e))
            }
            Err(MsiError::Safety(e)) => Err(ExecError::Safety(e)),
            Err(error) => Err(ExecError::Extract {
                message: error.to_string(),
            }),
        }
    }
}
fn extract(
    path: &Path,
    destination: &Path,
    context: &StepContext,
    limits: MsiLimits,
) -> Result<()> {
    context.checkpoint(0).map_err(io::Error::other)?;
    if std::fs::symlink_metadata(path)?.is_symlink() {
        return Err(crate::safety::SafetyError::WriteThroughLink(path.into()).into());
    }
    let source = File::open(path)?;
    let source_size = source.metadata()?.len();
    let (mut package, budget) = preflight::open(source, limits)?;
    let metadata = metadata::read(&mut package, limits)?;
    let total = metadata.unpacked_bytes().ok_or(MsiError::Limit)?;
    if total > context.limits.max_total_bytes
        || metadata.files.len() as u64 > context.limits.max_entries
    {
        return Err(MsiError::Limit);
    }
    budget.payload();
    // External cabinets are opened from a confined sibling directory, without
    // following links. No path from the package is used as an ambient path.
    let parent = crate::safety::open_staging(path.parent().unwrap_or(Path::new(".")))?;
    let mut packed = source_size;
    for media in &metadata.media {
        if !media.cabinet.starts_with('#') {
            let m = parent.symlink_metadata(&media.cabinet)?;
            if m.is_symlink() || !m.is_file() {
                return Err(crate::safety::SafetyError::SpecialFile.into());
            }
            packed = packed.checked_add(m.len()).ok_or(MsiError::Limit)?;
        }
    }
    let mut writer = context
        .open_destination(destination, packed)
        .map_err(io::Error::other)?;
    for media in &metadata.media {
        context.checkpoint(0).map_err(io::Error::other)?;
        let files: Vec<_> = metadata
            .files
            .iter()
            .filter(|file| file.cabinet == media.cabinet)
            .collect();
        if files.is_empty() {
            continue;
        }
        if let Some(stream) = media.cabinet.strip_prefix('#') {
            write_cab(package.read_stream(stream)?, &files, &mut writer, context)?;
        } else {
            let file = parent.open(&media.cabinet)?;
            if !file.metadata()?.is_file() {
                return Err(crate::safety::SafetyError::SpecialFile.into());
            }
            write_cab(file, &files, &mut writer, context)?;
        }
    }
    Ok(())
}
fn write_cab(
    reader: impl Read + Seek,
    files: &[&MsiFile],
    writer: &mut crate::safety::SafeDir,
    context: &StepContext,
) -> Result<()> {
    let mut cabinet = cab::Cabinet::new(reader)?;
    for file in files {
        let entry = cabinet
            .get_file_entry(&file.id)
            .ok_or_else(|| MsiError::Invalid(format!("missing cabinet member {}", file.id)))?;
        if u64::from(entry.uncompressed_size()) != file.size {
            return Err(MsiError::Invalid("MSI/CAB file size mismatch".into()));
        }
        context.checkpoint(file.size).map_err(io::Error::other)?;
        let mut reader = context.progress_reader(cabinet.read_file(&file.id)?);
        writer.write_file(
            file.path
                .to_str()
                .ok_or_else(|| MsiError::Invalid("non-UTF8 MSI path".into()))?,
            &mut reader,
            Some(file.size),
            0,
            0o644,
        )?;
        reader.finish().map_err(io::Error::other)?;
    }
    Ok(())
}
