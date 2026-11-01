use super::*;
use crate::{
    ArtifactId,
    discover::DiscoveryHints,
    exec::{ExecError, Extractor, StepContext},
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Mutex,
};
#[derive(Default)]
pub struct NsisExtractor {
    sources: BTreeMap<ArtifactId, PathBuf>,
    limits: NsisLimits,
    hints: Mutex<BTreeMap<ArtifactId, DiscoveryHints>>,
}
impl NsisExtractor {
    pub fn new(sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self {
            sources,
            limits: NsisLimits::default(),
            hints: Mutex::default(),
        }
    }
    pub fn with_limits(mut self, limits: NsisLimits) -> Self {
        self.limits = limits;
        self
    }
    pub fn take_discovery_hints(&self, artifact: &ArtifactId) -> Option<DiscoveryHints> {
        self.hints
            .lock()
            .expect("NSIS hints poisoned")
            .remove(artifact)
    }
}
impl Extractor for NsisExtractor {
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
                step: "NSIS select/strip (script destinations already application-relative)".into(),
            });
        }
        let path = crate::exec::source_path(&self.sources, artifact)?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            extract(path, destination, context, self.limits)
        }))
        .map_err(|_| ExecError::Extract {
            message: "upstream NSIS decoder rejected malformed input".into(),
        })?;
        match result {
            Ok(hints) => {
                self.hints
                    .lock()
                    .expect("NSIS hints poisoned")
                    .insert(artifact.clone(), hints);
                Ok(())
            }
            Err(NsisError::Io(error)) => {
                context.checkpoint(0)?;
                Err(ExecError::Io(error))
            }
            Err(NsisError::Safety(error)) => Err(ExecError::Safety(error)),
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
    limits: NsisLimits,
) -> Result<DiscoveryHints> {
    context.checkpoint(0).map_err(io::Error::other)?;
    if std::fs::symlink_metadata(path)?.is_symlink() {
        return Err(crate::safety::SafetyError::WriteThroughLink(path.into()).into());
    }
    let file = File::open(path)?;
    let packed = file.metadata()?.len();
    if packed > limits.max_input_bytes {
        return Err(NsisError::Limit);
    }
    let mut bytes = vec![];
    context
        .checked_reader(file)
        .take(limits.max_input_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limits.max_input_bytes {
        return Err(NsisError::Limit);
    }
    context.checkpoint(0).map_err(io::Error::other)?;
    let installer = metadata::parse(&bytes, limits)?;
    let metadata = metadata::analyze(&installer, limits)?;
    context.checkpoint(0).map_err(io::Error::other)?;
    let mut writer = context
        .open_destination(destination, packed)
        .map_err(io::Error::other)?;
    let mut total = 0_u64;
    for file in installer.files() {
        context.checkpoint(0).map_err(io::Error::other)?;
        let file = file.map_err(invalid)?;
        let Some(path) = app_path(&file.dest_path().map_err(invalid)?)? else {
            continue;
        };
        // Upstream's slice decoder bounds both its LZMA dictionary and output.
        // Cancellation is checked before/after this bounded CPU operation and
        // between every confined write chunk.
        let data = file.decompress().map_err(invalid)?;
        total = total
            .checked_add(data.len() as u64)
            .ok_or(NsisError::Limit)?;
        if total > context.limits.max_total_bytes {
            return Err(NsisError::Limit);
        }
        context
            .checkpoint(data.len() as u64)
            .map_err(io::Error::other)?;
        let mut reader = context.progress_reader(data.as_slice());
        writer.write_file(
            path.to_str().ok_or_else(|| invalid("non-UTF8 NSIS path"))?,
            &mut reader,
            Some(data.len() as u64),
            0,
            0o644,
        )?;
        reader.finish().map_err(io::Error::other)?;
    }
    Ok(metadata.discovery_hints())
}
