//! InstallShield Basic MSI has a distinct, explicitly identified silent contract.
use crate::{
    plan::{
        Handler, PlanContext, PlanError, Proposal,
        presets::{InstallerFamily, PresetError, SwitchSettings, switch_catalog},
    },
    *,
};
#[derive(Default)]
pub struct InstallShieldHandler;
impl InstallShieldHandler {
    pub fn new() -> Self {
        Self
    }
}
impl Handler for InstallShieldHandler {
    fn id(&self) -> HandlerId {
        HandlerId("installshield".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::InstallShield && node.role != Role::Uninstaller
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        let msi = node
            .facts
            .get("installshield.variant")
            .and_then(serde_json::Value::as_str)
            == Some("basic_msi");
        super::generic::propose(
            self.id(),
            node,
            context,
            InstallerFamily::InstallShield,
            msi,
            false,
        )
    }
}
pub fn identified_msi_args(
    settings: &SwitchSettings,
) -> std::result::Result<Vec<String>, PresetError> {
    if settings.install_dir.is_empty()
        || settings
            .install_dir
            .chars()
            .any(|c| c == '"' || c.is_control())
    {
        return Err(PresetError::InvalidValue("install directory"));
    }
    let templates = switch_catalog()[&InstallerFamily::InstallShield]
        .identified_msi_silent_args
        .as_ref()
        .ok_or(PresetError::SilentUnsupported)?;
    Ok(templates
        .iter()
        .map(|arg| arg.replace("{install_dir}", &settings.install_dir))
        .collect())
}
