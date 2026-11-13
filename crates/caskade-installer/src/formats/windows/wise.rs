//! Wise and Setup Factory default to their interactive, version-safe presets.
use crate::{
    plan::{Handler, PlanContext, PlanError, Proposal, presets::InstallerFamily},
    *,
};
#[derive(Default)]
pub struct WiseHandler;
impl Handler for WiseHandler {
    fn id(&self) -> HandlerId {
        HandlerId("wise".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        matches!(node.format, Format::Wise | Format::SetupFactory) && node.role != Role::Uninstaller
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        super::generic::propose(
            self.id(),
            node,
            context,
            InstallerFamily::from(node.format),
            false,
            false,
        )
    }
}
