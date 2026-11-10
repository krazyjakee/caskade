//! Settings: the sections, reading, changing and resetting one. Validation,
//! persistence and change events are the core's (`caskade_core::settings`).
use caskade_core::settings::{SettingsSection, SettingsService, SettingsSnapshot};

use crate::error::{CommandError, report};

/// Every settings section. A plugin's (`plugins.<id>`) schema goes through
/// `translate` with the plugin's id, for its titles and descriptions from
/// its own catalog (#219).
pub fn settings_sections(
    settings: &SettingsService,
    translate: impl Fn(&str, &mut serde_json::Value),
) -> Result<Vec<SettingsSection>, CommandError> {
    let mut sections = settings.sections().map_err(report)?;
    for section in &mut sections {
        if let Some(id) = section.namespace.strip_prefix("plugins.") {
            translate(id, &mut section.schema);
        }
    }
    Ok(sections)
}

/// A section's value and revision.
pub async fn settings_get(
    settings: &SettingsService,
    namespace: &str,
) -> Result<SettingsSnapshot, CommandError> {
    settings.get(namespace).await.map_err(report)
}

/// Changes a section; refused when `expected_revision` is stale.
pub async fn settings_set(
    settings: &SettingsService,
    namespace: &str,
    value: serde_json::Value,
    expected_revision: Option<u32>,
) -> Result<SettingsSnapshot, CommandError> {
    settings
        .set(namespace, value, expected_revision)
        .await
        .map_err(report)
}

/// Puts a section back to its defaults.
pub async fn settings_reset(
    settings: &SettingsService,
    namespace: &str,
    expected_revision: Option<u32>,
) -> Result<SettingsSnapshot, CommandError> {
    settings
        .reset(namespace, expected_revision)
        .await
        .map_err(report)
}
