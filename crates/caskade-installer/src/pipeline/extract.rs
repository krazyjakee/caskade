use super::*;
use crate::{
    ArtifactId,
    exec::{Extractor, StepContext},
};
pub struct PipelineExtractor {
    archives: ArchiveEngine,
    #[cfg(feature = "macos-formats")]
    dmg: crate::formats::macos::dmg::DmgExtractor,
    #[cfg(feature = "windows-formats")]
    msi: crate::formats::windows::msi::MsiExtractor,
    #[cfg(feature = "windows-formats")]
    inno: crate::formats::windows::inno::InnoExtractor,
    #[cfg(feature = "windows-formats")]
    nsis: crate::formats::windows::nsis::NsisExtractor,
}
impl PipelineExtractor {
    pub fn new(
        ctx: &Context,
        sources: BTreeMap<ArtifactId, PathBuf>,
        host: HostCaps,
        runtime: Arc<dyn crate::runtime::Runtime>,
    ) -> Self {
        #[cfg(not(feature = "macos-formats"))]
        let _ = (host, runtime);
        Self {
            #[cfg(feature = "macos-formats")]
            dmg: crate::formats::macos::dmg::DmgExtractor::new(sources.clone(), host, runtime),
            #[cfg(feature = "windows-formats")]
            msi: crate::formats::windows::msi::MsiExtractor::new(sources.clone()),
            #[cfg(feature = "windows-formats")]
            inno: crate::formats::windows::inno::InnoExtractor::new(sources.clone()),
            #[cfg(feature = "windows-formats")]
            nsis: crate::formats::windows::nsis::NsisExtractor::new(sources.clone()),
            archives: if let Some(tool) = &ctx.tool {
                ArchiveEngine::Hybrid(Box::new(
                    crate::formats::archive::sevenzip::HybridArchiveExtractor::new(
                        tool.clone(),
                        sources,
                    ),
                ))
            } else {
                ArchiveEngine::Native(Box::new(crate::formats::archive::ArchiveExtractor::new(
                    sources,
                )))
            },
        }
    }
    pub fn discovery_hints(&self, artifact: &ArtifactNode) -> Option<DiscoveryHints> {
        #[cfg(feature = "windows-formats")]
        if artifact.format == crate::Format::Inno {
            return self.inno.take_discovery_hints(&artifact.id);
        }
        #[cfg(feature = "windows-formats")]
        if artifact.format == crate::Format::Nsis {
            return self.nsis.take_discovery_hints(&artifact.id);
        }
        #[cfg(feature = "linux-formats")]
        return self.archives.discovery_hints(&artifact.id);
        #[cfg(not(feature = "linux-formats"))]
        {
            let _ = artifact;
            None
        }
    }
}
impl Extractor for PipelineExtractor {
    fn extract(
        &self,
        artifact: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> crate::exec::Result<()> {
        #[cfg(feature = "macos-formats")]
        if context.handler_id().is_some_and(|id| id.0 == "macos-dmg") {
            return self
                .dmg
                .extract(artifact, select, strip, destination, context);
        }
        #[cfg(feature = "windows-formats")]
        match context.handler_id().map(|id| id.0.as_str()) {
            Some("msi") => {
                return self
                    .msi
                    .extract(artifact, select, strip, destination, context);
            }
            Some("nsis") => {
                return self
                    .nsis
                    .extract(artifact, select, strip, destination, context);
            }
            Some("inno") => {
                return self
                    .inno
                    .extract(artifact, select, strip, destination, context);
            }
            _ => (),
        }
        self.archives
            .extract(artifact, select, strip, destination, context)
    }
}

enum ArchiveEngine {
    Native(Box<crate::formats::archive::ArchiveExtractor>),
    Hybrid(Box<crate::formats::archive::sevenzip::HybridArchiveExtractor>),
}
impl ArchiveEngine {
    #[cfg(feature = "linux-formats")]
    fn discovery_hints(&self, id: &ArtifactId) -> Option<DiscoveryHints> {
        match self {
            Self::Native(engine) => engine.take_discovery_hints(id),
            Self::Hybrid(engine) => engine.take_discovery_hints(id),
        }
    }
}
impl Extractor for ArchiveEngine {
    fn extract(
        &self,
        artifact: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> crate::exec::Result<()> {
        match self {
            Self::Native(engine) => engine.extract(artifact, select, strip, destination, context),
            Self::Hybrid(engine) => engine.extract(artifact, select, strip, destination, context),
        }
    }
}

#[cfg(all(test, feature = "macos-formats", unix))]
#[path = "extract_tests.rs"]
mod tests;
