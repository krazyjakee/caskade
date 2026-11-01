use super::*;
use crate::{
    ArtifactId,
    exec::{ExecError, Extractor, StepContext},
    formats::archive::PayloadReader,
    safety::{SafeDir, open_staging},
};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
#[derive(Default)]
pub struct BurnExtractor {
    sources: BTreeMap<ArtifactId, PathBuf>,
}
impl BurnExtractor {
    pub fn new(sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self { sources }
    }
}
impl Extractor for BurnExtractor {
    fn extract(
        &self,
        id: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> crate::exec::Result<()> {
        if !select.is_empty() || strip.is_some() {
            return Err(ExecError::UnsupportedStep {
                step: "Burn select/strip".into(),
            });
        }
        let path = crate::exec::source_path(&self.sources, id)?;
        let result = extract(path, id, destination, context);
        match result {
            Ok(()) => Ok(()),
            Err(BurnError::Safety(e)) => Err(ExecError::Safety(e)),
            Err(error) => {
                context.checkpoint(0)?;
                Err(ExecError::Extract {
                    message: error.to_string(),
                })
            }
        }
    }
}
fn extract(path: &Path, id: &ArtifactId, destination: &Path, context: &StepContext) -> Result<()> {
    use sha1::{Digest, Sha1};
    context.checkpoint(0).map_err(std::io::Error::other)?;
    if std::fs::symlink_metadata(path)?.is_symlink() {
        return Err(crate::safety::SafetyError::WriteThroughLink(path.into()).into());
    }
    let mut source = std::fs::File::open(path)?;
    let metadata = inspect(&mut source)?;
    let scratch = context.create_scratch().map_err(std::io::Error::other)?;
    let mut writer = SafeDir::new(
        open_staging(scratch.path())?,
        context.limits,
        source.metadata()?.len(),
        context.case_sensitive,
    );
    let view = PayloadReader::new(
        &mut source,
        metadata.attached.offset,
        metadata.attached.length,
    )
    .map_err(|e| invalid(&e.to_string()))?;
    let mut cabinet = cab::Cabinet::new(view)?;
    for payload in &metadata.payloads {
        context
            .checkpoint(payload.size)
            .map_err(std::io::Error::other)?;
        let entry = cabinet
            .get_file_entry(&payload.source)
            .ok_or_else(|| invalid("missing Burn payload"))?;
        if u64::from(entry.uncompressed_size()) != payload.size {
            return Err(invalid("Burn/CAB size mismatch"));
        }
        let stream = cabinet.read_file(&payload.source)?;
        writer.write_file(
            &payload.path,
            &mut context.checked_reader(stream),
            Some(payload.size),
            0,
            0o644,
        )?;
        let mut file = writer.open_read(&payload.path)?;
        let mut hash = Sha1::new();
        let mut buffer = [0u8; 65536];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            context
                .checkpoint(count as u64)
                .map_err(std::io::Error::other)?;
            hash.update(&buffer[..count]);
        }
        if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&payload.sha1) {
            return Err(invalid("Burn payload checksum mismatch"));
        }
    }
    let mut extracted = 0;
    for payload in &metadata.payloads {
        let mut file = writer.open_read(&payload.path)?;
        let mut magic = [0u8; 8];
        if file.read(&mut magic)? != 8 || magic != [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]
        {
            continue;
        }
        file.seek(SeekFrom::Start(0))?;
        super::super::msi::inspect_reader(file, super::super::msi::MsiLimits::default())
            .map_err(|e| invalid(&e.to_string()))?;
        let source = scratch
            .path()
            .join(crate::safety::sanitize_path(&payload.path)?);
        super::super::msi::MsiExtractor::new(BTreeMap::from([(id.clone(), source)]))
            .extract(id, &[], None, destination, context)
            .map_err(std::io::Error::other)?;
        extracted += 1;
    }
    if extracted == 0 {
        return Err(invalid("Burn chain contains no extractable MSI"));
    }
    Ok(())
}
