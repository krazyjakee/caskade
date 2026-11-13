//! PE self-extracting archives are opened statically. Their executable stubs
//! are never launched; only archive readers or the verified sidecar see them.
use super::{ArchiveError, ArchiveOptions, ArchiveProbe, ExtractionControl, PayloadReader, Result};
use crate::{
    ArtifactId, ArtifactNode, Candidate, Fidelity, Format, HandlerId, Requirements, Step, Strategy,
    exec::{ExecError, Extractor, StepContext},
    plan::{Handler, PlanContext, PlanError, Proposal},
    probe::{FormatProbe, PeProbe, ProbeEngine, ProbeError, ProbeSource},
    safety::SafeDir,
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfxPayload {
    pub format: Format,
    pub offset: u64,
    pub length: u64,
}
fn kind(node: &ArtifactNode) -> Option<Format> {
    if node.format != Format::Pe {
        return None;
    }
    match node.facts.get("pe.sfx_format")?.as_str()? {
        "zip" => Some(Format::Zip),
        "seven_zip" => Some(Format::SevenZip),
        "rar" => Some(Format::Rar),
        _ => None,
    }
}
/// Find a signature in the PE overlay, checking native archive candidates with
/// the bounded metadata readers before accepting an offset. PE resource strings
/// alone never qualify as payloads. At most 16 candidate headers are parsed.
pub fn locate(reader: &mut (impl Read + Seek), options: &ArchiveOptions) -> Result<SfxPayload> {
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(PeProbe::new()));
    let node = engine
        .probe_reader(reader, Path::new("source.exe"))
        .map_err(|e| ArchiveError::Invalid(e.to_string()))?
        .root;
    locate_node(reader, &node, options)
}
fn locate_node(
    reader: &mut (impl Read + Seek),
    node: &ArtifactNode,
    options: &ArchiveOptions,
) -> Result<SfxPayload> {
    let format =
        kind(node).ok_or_else(|| ArchiveError::Unsupported("not a recognised PE SFX".into()))?;
    let size = reader.seek(SeekFrom::End(0))?;
    let overlay = node
        .facts
        .get("pe.overlay_offset")
        .and_then(|v| v.as_u64())
        .filter(|offset| *offset < size)
        .ok_or_else(|| ArchiveError::Invalid("invalid PE overlay offset".into()))?;
    let signature: &[u8] = match format {
        Format::Zip => b"PK\x03\x04",
        Format::SevenZip => b"7z\xbc\xaf\x27\x1c",
        Format::Rar => b"Rar!\x1a\x07",
        _ => unreachable!(),
    };
    let limit =
        size.min(overlay.saturating_add(options.max_metadata_bytes.min(16 * 1024 * 1024) as u64));
    let mut position = overlay;
    let mut attempts = 0;
    while position < limit {
        reader.seek(SeekFrom::Start(position))?;
        let length = (limit - position).min(64 * 1024) as usize;
        let mut block = vec![0; length];
        reader.read_exact(&mut block)?;
        for index in 0..block.len().saturating_sub(signature.len() - 1) {
            if &block[index..index + signature.len()] != signature {
                continue;
            }
            attempts += 1;
            if attempts > 16 {
                return Err(ArchiveError::MemoryLimit);
            }
            let offset = position + index as u64;
            let payload = SfxPayload {
                format,
                offset,
                length: size - offset,
            };
            if format == Format::Rar {
                return Ok(payload);
            }
            let start = if format == Format::Zip { 0 } else { offset };
            let view = PayloadReader::new(&mut *reader, start, size - start)?;
            if super::list(view, format, options).is_ok() {
                return Ok(payload);
            }
        }
        if length < signature.len() {
            break;
        }
        position += (length - signature.len() + 1) as u64;
    }
    Err(ArchiveError::Invalid(
        "no valid archive in PE overlay".into(),
    ))
}

#[derive(Default)]
pub struct SfxProbe;
impl FormatProbe for SfxProbe {
    fn id(&self) -> &str {
        "sfx"
    }
    fn list(
        &self,
        source: &mut ProbeSource<'_>,
        node: &ArtifactNode,
    ) -> std::result::Result<Option<Vec<ArtifactNode>>, ProbeError> {
        if !matches!(kind(node), Some(Format::Zip | Format::SevenZip)) {
            return Ok(None);
        }
        let payload = locate_node(source, node, &ArchiveOptions::default())
            .map_err(|e| ProbeError::Io(std::io::Error::other(e)))?;
        let size = source.size();
        let start = if payload.format == Format::Zip {
            0
        } else {
            payload.offset
        };
        let mut view = PayloadReader::new(source, start, size - start)?;
        let mut bounded = ProbeSource::new(&mut view, u64::MAX)?;
        let mut archive = node.clone();
        archive.format = payload.format;
        ArchiveProbe.list(&mut bounded, &archive)
    }
}

#[derive(Default)]
pub struct SfxHandler;
impl Handler for SfxHandler {
    fn id(&self) -> HandlerId {
        HandlerId("sfx".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        kind(node).is_some()
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        if kind(node) == Some(Format::Rar) && context.host.seven_zip.is_none() {
            return Ok(Proposal::default());
        }
        Ok(Proposal {
            candidates: vec![Candidate {
                handler: self.id(),
                strategy: Strategy::Extract,
                confidence: 1.0,
                fidelity: Fidelity::Exact,
                requires: Requirements::default(),
                steps: vec![Step::Extract {
                    from: node.id.clone(),
                    select: vec![],
                    strip: None,
                    to: PathBuf::from("."),
                }],
                warnings: vec![],
            }],
            question: None,
        })
    }
}

pub fn extract_path(
    path: &Path,
    scratch: &Path,
    tool: Option<&super::sevenzip::SevenZip>,
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    let mut file = File::open(path)?;
    let payload = locate(&mut file, options)?;
    if payload.format == Format::Rar {
        let tool = tool.ok_or_else(|| {
            ArchiveError::Unsupported("RAR SFX requires the pinned 7zz sidecar".into())
        })?;
        return tool
            .clone()
            .with_options(options.clone())
            .extract(path, scratch, dir, control)
            .map_err(|e| ArchiveError::Invalid(e.to_string()));
    }
    let size = file.metadata()?.len();
    let start = if payload.format == Format::Zip {
        0
    } else {
        payload.offset
    };
    super::extract(
        PayloadReader::new(file, start, size - start)?,
        payload.format,
        dir,
        options,
        control,
    )
}

pub struct SfxExtractor {
    sources: BTreeMap<ArtifactId, PathBuf>,
    tool: Option<super::sevenzip::SevenZip>,
}
impl SfxExtractor {
    pub fn new(sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self {
            sources,
            tool: None,
        }
    }
    pub fn with_sidecar(mut self, tool: super::sevenzip::SevenZip) -> Self {
        self.tool = Some(tool);
        self
    }
}
impl Extractor for SfxExtractor {
    fn extract(
        &self,
        id: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> std::result::Result<(), ExecError> {
        let path = crate::exec::source_path(&self.sources, id)?;
        context.checkpoint(0)?;
        let scratch = context.create_scratch()?;
        let mut dir = context.open_destination(destination, path.metadata()?.len())?;
        let options = ArchiveOptions {
            select: select.to_vec(),
            strip: strip.map(Path::to_path_buf),
            max_entries: context.limits.max_entries.min(100_000) as usize,
            max_unpacked_bytes: context.limits.max_total_bytes,
            max_expansion_ratio: context.limits.max_expansion_ratio,
            ..ArchiveOptions::default()
        };
        let tool = self.tool.clone().map(|tool| {
            tool.with_limits(context.limits)
                .with_case_sensitivity(context.case_sensitive)
        });
        extract_path(
            path,
            scratch.path(),
            tool.as_ref(),
            &mut dir,
            &options,
            &mut &*context,
        )
        .map_err(crate::exec::ExecError::from)
    }
}
