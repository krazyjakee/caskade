//! Adapter-owned preference namespace, shared with the SettingsService (#110).
use crate::{ImportOptions, Result};
use caskade_core::settings::{SectionDefinition, SettingsService};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

pub const IMPORT_SETTINGS: &str = "discovery.import";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportSettings {
    pub seed_playtime: bool,
}
impl Default for ImportSettings {
    fn default() -> Self {
        Self {
            seed_playtime: true,
        }
    }
}
pub fn register_settings(settings: &SettingsService) -> caskade_core::settings::Result<()> {
    settings.register(SectionDefinition::typed::<ImportSettings>(
        IMPORT_SETTINGS,
        1,
    )?)
}
pub async fn import_options(settings: &SettingsService) -> Result<ImportOptions> {
    let snapshot = settings.get(IMPORT_SETTINGS).await?;
    let value: ImportSettings =
        serde_json::from_value(snapshot.value).map_err(caskade_core::Error::from)?;
    Ok(ImportOptions {
        seed_playtime: value.seed_playtime,
        ..Default::default()
    })
}
