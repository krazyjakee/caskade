//! Settings: the sections, reading, changing and resetting one. Validation,
//! persistence and change events are the core's (`caskade_core::settings`).
use caskade_core::settings::{SettingsSection, SettingsService, SettingsSnapshot};
use caskade_plugin_host::PluginHost;

use crate::error::{CommandError, report};

/// Every settings section. A plugin's (`plugins.<id>`) comes with its
/// titles and descriptions from the plugin's own catalog (#219).
pub fn settings_sections(
    settings: &SettingsService,
    plugins: Option<&PluginHost>,
) -> Result<Vec<SettingsSection>, CommandError> {
    let mut sections = settings.sections().map_err(report)?;
    if let Some(plugins) = plugins {
        for section in &mut sections {
            if let Some(id) = section.namespace.strip_prefix("plugins.") {
                plugins.translate_settings_schema(id, &mut section.schema);
            }
        }
    }
    Ok(sections)
}

/// A section's value and revision.
pub async fn settings_get(
    settings: &SettingsService,
    namespace: &str,
) -> Result<SettingsSnapshot, CommandError> {
    settings.get(namespace).await.map_err(|error| {
        settings_error(
            settings,
            caskade_core::tr!("Could not read settings"),
            error,
        )
    })
}

/// Changes a section; refused when `expected_revision` is stale.
pub async fn settings_set(
    settings: &SettingsService,
    namespace: &str,
    value: serde_json::Value,
    expected_revision: Option<u32>,
) -> Result<SettingsSnapshot, CommandError> {
    crate::permissions::enforce(
        "settings_set",
        || serde_json::json!({ "namespace": namespace }),
    )?;
    settings
        .set(namespace, value, expected_revision)
        .await
        .map_err(|error| {
            settings_error(
                settings,
                caskade_core::tr!("Could not save settings"),
                error,
            )
        })
}

/// Puts a section back to its defaults.
pub async fn settings_reset(
    settings: &SettingsService,
    namespace: &str,
    expected_revision: Option<u32>,
) -> Result<SettingsSnapshot, CommandError> {
    crate::permissions::enforce(
        "settings_reset",
        || serde_json::json!({ "namespace": namespace }),
    )?;
    settings
        .reset(namespace, expected_revision)
        .await
        .map_err(|error| {
            settings_error(
                settings,
                caskade_core::tr!("Could not save settings"),
                error,
            )
        })
}

/// Keep the OS/SQLite cause and name the actual database, including for setup.
fn settings_error(
    settings: &SettingsService,
    operation: String,
    error: caskade_core::settings::SettingsError,
) -> CommandError {
    let storage = matches!(error, caskade_core::settings::SettingsError::Store(_));
    let mut failure = report(error);
    if storage && let Some(path) = settings.database_path() {
        failure.summary = caskade_core::error::ErrorReport::new(
            failure.code,
            caskade_core::tr!(
                "{operation} at “{path}”. {reason}",
                operation = operation,
                path = path.display(),
                reason = failure.summary
            ),
        )
        .summary;
    }
    failure
}
