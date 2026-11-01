//! Plugin settings (#128, TECH-PLAN §4.2 `contributes.settings`). A
//! manifest's JSON Schema becomes the settings section `plugins.<id>`,
//! which Settings → Plugins renders and the backend reads with
//! `settings-get`. Changes reach the plugin as `settings.changed`.

use crate::{PluginHost, manifest::Manifest, settings_namespace};
use caskade_core::settings::{SectionDefinition, SettingsChanged, SettingsService};
use serde_json::{Map, Value};

/// The event a plugin lists in `contributes.hooks` to hear about changes.
pub const SETTINGS_CHANGED: &str = "settings.changed";

/// The section's schema version: the plugin's semver packed into a `u32`
/// (12 bits major, 10 minor, 10 patch, saturating). Any change revalidates
/// the stored value, so the packing only has to tell versions apart.
pub fn schema_version(version: &semver::Version) -> u32 {
    let part = |value: u64, bits: u32| value.min((1 << bits) - 1) as u32;
    (part(version.major, 12) << 20 | part(version.minor, 10) << 10 | part(version.patch, 10)).max(1)
}

/// The schema's own `default`, otherwise each top-level property's.
pub fn defaults(schema: &Value) -> Value {
    if let Some(default) = schema.get("default") {
        return default.clone();
    }
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, property)| Some((key.clone(), property.get("default")?.clone())))
        .collect::<Map<_, _>>();
    Value::Object(properties)
}

/// The plugin's section for `schema`, its manifest's settings.
fn definition(manifest: &Manifest, schema: &Value) -> Result<SectionDefinition, String> {
    SectionDefinition::new(
        &settings_namespace(&manifest.id),
        schema_version(&manifest.version),
        schema.clone(),
        defaults(schema),
    )
    .map_err(|error| format!("settings: {error}"))
}

/// Why a manifest's settings schema would be refused, if it would.
pub fn check(manifest: &Manifest) -> Option<String> {
    definition(manifest, manifest.contributes.settings.as_ref()?).err()
}

/// Registers, replaces or (when the manifest no longer has settings)
/// unregisters the plugin's section. The error is shown on the plugin; its
/// other contributions are unaffected.
pub async fn register(settings: &SettingsService, manifest: &Manifest) -> Result<(), String> {
    let Some(schema) = &manifest.contributes.settings else {
        // The stored value stays, in case the contribution comes back.
        settings
            .remove(&settings_namespace(&manifest.id), false)
            .await
            .map_err(|error| format!("settings: {error}"))?;
        return Ok(());
    };
    settings
        .replace(definition(manifest, schema)?.revalidating())
        .map_err(|error| format!("settings: {error}"))
}

/// Delivers one settings change to the plugin that owns the section, as
/// `settings.changed` with `{"version": 1, "data": {namespace, revision,
/// value}}`, when the plugin is
/// enabled, has a backend and lists the event in `contributes.hooks`.
/// Returns whether it was delivered; the app runs this for every change.
pub async fn deliver(
    host: &PluginHost,
    settings: &SettingsService,
    changed: &SettingsChanged,
) -> crate::host::Result<bool> {
    let Some(id) = changed.namespace.strip_prefix("plugins.") else {
        return Ok(false);
    };
    let wants = host.enabled(id)
        && host.manifest(id).is_some_and(|manifest| {
            manifest.backend.is_some()
                && manifest
                    .contributes
                    .hooks
                    .iter()
                    .any(|hook| hook == SETTINGS_CHANGED)
        });
    if !wants {
        return Ok(false);
    }
    let value = settings
        .get(&changed.namespace)
        .await
        .map_err(|error| caskade_core::Error::Invalid(error.to_string()))?
        .value;
    // The same envelope as every other `on-event` payload (#125).
    let payload = serde_json::json!({
        "version": 1,
        "data": {
            "namespace": changed.namespace,
            "revision": changed.revision,
            "value": value,
        },
    })
    .to_string();
    host.event(id, SETTINGS_CHANGED, &payload).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn versions_pack_in_order_and_never_to_zero() {
        let v = |text: &str| schema_version(&semver::Version::parse(text).unwrap());
        assert_eq!(v("0.0.0"), 1);
        assert!(v("0.1.0") < v("0.2.0"));
        assert!(v("1.9.9") < v("2.0.0"));
        assert!(v("1.2.3") != v("1.2.4"));
        assert_eq!(v("99999.0.0"), v("4095.0.0"));
    }

    #[test]
    fn defaults_come_from_the_schema_or_its_properties() {
        assert_eq!(
            defaults(&json!({"type": "object", "default": {"a": 1}})),
            json!({"a": 1})
        );
        assert_eq!(
            defaults(&json!({"properties": {"a": {"default": true}, "b": {"type": "string"}}})),
            json!({"a": true})
        );
        assert_eq!(defaults(&json!({"type": "object"})), json!({}));
    }
}
