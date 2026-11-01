use super::{ArchiveOptions, Result, common::*, volume::Volumes};
use crate::{
    ArtifactId, ArtifactNode, Candidate, Fidelity, Format, HandlerId, Requirements, Step, Strategy,
    exec::{ExecError, Extractor, StepContext},
    input::{ResolvedInput, SetKind},
    plan::{Handler, PlanContext, PlanError, Proposal},
    probe::ProbeEngine,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct ArchiveHandler;
impl ArchiveHandler {
    pub fn new() -> Self {
        Self
    }
}
impl Handler for ArchiveHandler {
    fn id(&self) -> HandlerId {
        HandlerId("archive".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        super::supported(node.format)
            || node
                .facts
                .get("pe.sfx_format")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|format| matches!(format, "zip" | "seven_zip"))
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        _: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        Ok(Proposal {
            candidates: vec![Candidate {
                handler: self.id(),
                strategy: Strategy::Extract,
                confidence: 1.0,
                fidelity: Fidelity::Exact,
                requires: Requirements {
                    est_disk_bytes: if node.children_complete {
                        node.children
                            .iter()
                            .try_fold(0_u64, |sum, child| sum.checked_add(child.size?))
                    } else {
                        None
                    },
                    ..Default::default()
                },
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

/// Artifact source mapping is explicit. Multipart discovery happens before any
/// parser is opened, so missing sibling volumes have already failed resolution.
#[derive(Default)]
pub struct ArchiveExtractor {
    sources: BTreeMap<ArtifactId, PathBuf>,
    #[cfg(feature = "windows-formats")]
    burn: crate::formats::windows::burn::BurnExtractor,
    artifacts: std::sync::Mutex<BTreeMap<ArtifactId, Vec<ArtifactNode>>>,
    #[cfg(feature = "linux-formats")]
    shell: crate::formats::linux::makeself::MakeselfExtractor,
    #[cfg(feature = "linux-formats")]
    package: crate::formats::linux::deb::PackageExtractor,
    #[cfg(feature = "macos-formats")]
    app: crate::formats::macos::app::AppExtractor,
    #[cfg(feature = "macos-formats")]
    mac_pkg: crate::formats::macos::pkg::PkgExtractor,
}
impl ArchiveExtractor {
    pub fn new(sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self {
            #[cfg(feature = "windows-formats")]
            burn: crate::formats::windows::burn::BurnExtractor::new(sources.clone()),
            #[cfg(feature = "linux-formats")]
            shell: crate::formats::linux::makeself::MakeselfExtractor::new(sources.clone()),
            #[cfg(feature = "linux-formats")]
            package: crate::formats::linux::deb::PackageExtractor::new(sources.clone()),
            #[cfg(feature = "macos-formats")]
            app: crate::formats::macos::app::AppExtractor::new(sources.clone()),
            #[cfg(feature = "macos-formats")]
            mac_pkg: crate::formats::macos::pkg::PkgExtractor::new(sources.clone()),
            sources,
            artifacts: std::sync::Mutex::default(),
        }
    }
    /// Provisional extraction output. Consume only after the executor reports
    /// success; a later failed step may have rolled staging back.
    pub fn take_artifacts(&self, artifact: &ArtifactId) -> Option<Vec<ArtifactNode>> {
        #[cfg(feature = "macos-formats")]
        if let Some(nodes) = self.mac_pkg.take_artifacts(artifact) {
            return Some(nodes);
        }
        #[cfg(feature = "linux-formats")]
        if let Some(nodes) = self.package.take_artifacts(artifact) {
            return Some(nodes);
        }
        #[cfg(feature = "linux-formats")]
        if let Some(nodes) = self.shell.take_artifacts(artifact) {
            return Some(nodes);
        }
        self.artifacts
            .lock()
            .expect("archive artifacts poisoned")
            .remove(artifact)
    }
    #[cfg(feature = "linux-formats")]
    pub fn take_discovery_hints(
        &self,
        artifact: &ArtifactId,
    ) -> Option<crate::discover::DiscoveryHints> {
        if let Some(records) = self.package.references(artifact) {
            return Some(crate::formats::linux::deb::discovery_hints(&records));
        }
        self.shell.take_discovery_hints(artifact)
    }
}
pub(super) fn source(path: &Path) -> Result<(super::PayloadReader<Volumes>, Format)> {
    let resolved =
        crate::input::resolve_path(path).map_err(|e| ArchiveError::Invalid(e.to_string()))?;
    let (primary, files) = match resolved {
        ResolvedInput::File { path } => (path.clone(), vec![path]),
        ResolvedInput::MultiPart {
            kind: SetKind::SevenZip | SetKind::SplitZip,
            primary,
            files,
        } => (primary, files),
        _ => {
            return Err(ArchiveError::Unsupported(
                "input is not a supported archive set".into(),
            ));
        }
    };
    let mut reader = Volumes::open(&files)?;
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(crate::probe::PeProbe::new()));
    engine.register(Box::new(super::ArchiveProbe));
    let node = engine
        .probe_reader(&mut reader, &primary)
        .map_err(|e| ArchiveError::Invalid(e.to_string()))?
        .root;
    let (format, start) = if node.format == Format::Pe {
        let payload = super::sfx::locate(&mut reader, &ArchiveOptions::default())?;
        (
            payload.format,
            if payload.format == Format::Zip {
                0
            } else {
                payload.offset
            },
        )
    } else {
        (node.format, 0)
    };
    if !super::supported(format) {
        return Err(ArchiveError::Unsupported(format!("{format:?}")));
    }
    let length = reader.size() - start;
    Ok((super::PayloadReader::new(reader, start, length)?, format))
}
impl Extractor for ArchiveExtractor {
    fn extract(
        &self,
        artifact: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> std::result::Result<(), ExecError> {
        #[cfg(feature = "windows-formats")]
        if context.handler_id().is_some_and(|id| id.0 == "burn") {
            return self
                .burn
                .extract(artifact, select, strip, destination, context);
        }
        #[cfg(feature = "macos-formats")]
        if context.handler_id().is_some_and(|id| id.0 == "macos-pkg") {
            return self
                .mac_pkg
                .extract(artifact, select, strip, destination, context);
        }
        #[cfg(feature = "macos-formats")]
        if context.handler_id().is_some_and(|id| id.0 == "macos-app") {
            return self
                .app
                .extract(artifact, select, strip, destination, context);
        }
        #[cfg(feature = "linux-formats")]
        if context
            .handler_id()
            .is_some_and(|id| matches!(id.0.as_str(), "deb" | "rpm"))
        {
            return self
                .package
                .extract(artifact, select, strip, destination, context);
        }
        #[cfg(feature = "linux-formats")]
        if context
            .handler_id()
            .is_some_and(|id| matches!(id.0.as_str(), "makeself" | "mojosetup"))
        {
            return self
                .shell
                .extract(artifact, select, strip, destination, context);
        }
        let path = crate::exec::source_path(&self.sources, artifact)?;
        context.checkpoint(0)?;
        let (reader, format) = source(path).map_err(ExecError::from)?;
        let mut dir = context.open_destination(destination, reader.size())?;
        let options = ArchiveOptions::for_step(select, strip, &context.limits);
        let artifacts = super::extract(reader, format, &mut dir, &options, &mut &*context)
            .map_err(ExecError::from)?;
        self.artifacts
            .lock()
            .expect("archive artifacts poisoned")
            .insert(artifact.clone(), artifacts);
        Ok(())
    }
}
