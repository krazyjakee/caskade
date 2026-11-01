use super::{SevenZip, SevenZipError};
use crate::{
    ArtifactId, ArtifactNode, Candidate, Fidelity, Format, HandlerId, Requirements, Step, Strategy,
    exec::{ExecError, Extractor, StepContext},
    plan::{Handler, PlanContext, PlanError, Proposal},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Default)]
pub struct SevenZipHandler;
impl Handler for SevenZipHandler {
    fn id(&self) -> HandlerId {
        HandlerId("sevenzip".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        super::super::supported(node.format)
            || matches!(
                node.format,
                Format::Rar | Format::Cab | Format::Wim | Format::Iso | Format::Unknown
            )
            || node.facts.contains_key("pe.sfx_format")
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        if context.host.seven_zip.is_none() {
            return Ok(Proposal::default());
        }
        let known = node.format != Format::Unknown;
        let native = super::super::supported(node.format);
        Ok(Proposal {
            candidates: vec![Candidate {
                handler: self.id(),
                strategy: Strategy::Extract,
                confidence: if native {
                    0.9
                } else if known {
                    0.99
                } else {
                    0.5
                },
                fidelity: if native {
                    Fidelity::Equivalent
                } else if known {
                    Fidelity::Exact
                } else {
                    Fidelity::Partial
                },
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
pub struct SevenZipExtractor {
    tool: SevenZip,
    sources: BTreeMap<ArtifactId, PathBuf>,
    artifacts: Mutex<BTreeMap<ArtifactId, Vec<ArtifactNode>>>,
}
impl SevenZipExtractor {
    pub fn new(tool: SevenZip, sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self {
            tool,
            sources,
            artifacts: Mutex::default(),
        }
    }
    pub fn take_artifacts(&self, id: &ArtifactId) -> Option<Vec<ArtifactNode>> {
        self.artifacts
            .lock()
            .expect("sidecar artifacts poisoned")
            .remove(id)
    }
}
impl Extractor for SevenZipExtractor {
    fn extract(
        &self,
        id: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> std::result::Result<(), ExecError> {
        let source = crate::exec::source_path(&self.sources, id)?;
        let (_, packed) = super::resolved(source).map_err(map_error)?;
        let scratch = context.create_scratch()?;
        let mut writer = context.open_destination(destination, packed)?;
        let mut tool = self
            .tool
            .clone()
            .with_limits(context.limits)
            .with_case_sensitivity(context.case_sensitive);
        tool.options.select = select.to_vec();
        tool.options.strip = strip.map(Path::to_path_buf);
        let artifacts = tool
            .extract(source, scratch.path(), &mut writer, &mut &*context)
            .map_err(map_error)?;
        self.artifacts
            .lock()
            .expect("sidecar artifacts poisoned")
            .insert(id.clone(), artifacts);
        Ok(())
    }
}
fn map_error(error: SevenZipError) -> ExecError {
    match error {
        SevenZipError::Io(e) => ExecError::Io(e),
        SevenZipError::Safety(e) => ExecError::Safety(e),
        SevenZipError::Control(e) => e.into_exec_error(|e| SevenZipError::Control(e).to_string()),
        other => ExecError::Extract {
            message: other.to_string(),
        },
    }
}

/// Route retained archive/sidecar candidates without retrying inside a dirty
/// destination. The executor rolls each failed attempt back before the next.
pub struct HybridArchiveExtractor {
    native: super::super::ArchiveExtractor,
    sidecar: SevenZipExtractor,
    sfx: super::super::sfx::SfxExtractor,
}
impl HybridArchiveExtractor {
    pub fn new(tool: SevenZip, sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self {
            native: super::super::ArchiveExtractor::new(sources.clone()),
            sfx: super::super::sfx::SfxExtractor::new(sources.clone()).with_sidecar(tool.clone()),
            sidecar: SevenZipExtractor::new(tool, sources),
        }
    }
    pub fn take_artifacts(&self, id: &ArtifactId) -> Option<Vec<ArtifactNode>> {
        self.sidecar
            .take_artifacts(id)
            .or_else(|| self.sfx.take_artifacts(id))
            .or_else(|| self.native.take_artifacts(id))
    }
    #[cfg(feature = "linux-formats")]
    pub fn take_discovery_hints(&self, id: &ArtifactId) -> Option<crate::discover::DiscoveryHints> {
        self.native.take_discovery_hints(id)
    }
}
impl Extractor for HybridArchiveExtractor {
    fn extract(
        &self,
        id: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> std::result::Result<(), ExecError> {
        if context
            .handler_id()
            .is_some_and(|handler| handler.0 == "sfx")
        {
            self.sfx.extract(id, select, strip, destination, context)
        } else if context.handler_id().is_some_and(|handler| {
            matches!(
                handler.0.as_str(),
                "sevenzip" | "windows-generic" | "installshield" | "wise"
            )
        }) {
            self.sidecar
                .extract(id, select, strip, destination, context)
        } else {
            self.native.extract(id, select, strip, destination, context)
        }
    }
}
