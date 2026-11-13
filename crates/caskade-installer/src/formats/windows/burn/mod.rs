//! WiX Burn v2 container metadata and embedded MSI extraction, without execution.
mod extract;
mod metadata;
use crate::{
    plan::{Handler, PlanContext, PlanError, Proposal, presets::InstallerFamily},
    probe::{Detection, FormatProbe, ProbeError, ProbeSource},
    *,
};
pub use extract::BurnExtractor;
pub use metadata::{BurnMetadata, Payload, Span, inspect, parse_manifest};
#[derive(Debug, thiserror::Error)]
pub enum BurnError {
    #[error("Burn I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid or unsupported Burn container: {0}")]
    Invalid(String),
    #[error("Burn metadata or payload limit exceeded")]
    Limit,
    #[error(transparent)]
    Safety(#[from] crate::safety::SafetyError),
}
pub type Result<T> = std::result::Result<T, BurnError>;
pub(crate) fn invalid(message: &str) -> BurnError {
    BurnError::Invalid(message.into())
}
#[derive(Default)]
pub struct BurnHandler;
impl Handler for BurnHandler {
    fn id(&self) -> HandlerId {
        HandlerId("burn".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::Burn && node.role != Role::Uninstaller
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        super::generic::propose(self.id(), node, context, InstallerFamily::Burn, true, true)
    }
}
#[derive(Default)]
pub struct BurnProbe;
impl FormatProbe for BurnProbe {
    fn id(&self) -> &str {
        "burn"
    }
    fn detect(
        &self,
        source: &mut ProbeSource<'_>,
    ) -> std::result::Result<Option<Detection>, ProbeError> {
        if source.size() < 64 || source.read_at(0, 2)? != b"MZ" {
            return Ok(None);
        }
        match inspect(source) {
            Ok(metadata) => {
                let mut detection = Detection::new(Format::Burn, PayloadPlatform::Windows, true);
                detection.role = Some(Role::Installer);
                detection.container = true;
                detection.facts.insert(
                    "burn.metadata".into(),
                    serde_json::to_value(metadata).expect("Burn metadata serializes"),
                );
                Ok(Some(detection))
            }
            Err(_) => Ok(None),
        }
    }
}
