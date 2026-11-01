use super::{Result, SettingsError};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) type Migration = Arc<dyn Fn(Value) -> Result<Value> + Send + Sync>;

#[derive(Debug, Clone, Serialize, serde::Deserialize, schemars::JsonSchema, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSection {
    pub namespace: String,
    pub version: u32,
    pub schema: Value,
    pub defaults: Value,
}

/// Registration is done by core, installer, discovery or later plugin adapters.
/// Schemas have no network/file retriever; only embedded references resolve.
pub struct SectionDefinition {
    pub(crate) descriptor: SettingsSection,
    pub(crate) validator: jsonschema::Validator,
    pub(crate) migrations: BTreeMap<u32, Migration>,
    pub(crate) revalidate: bool,
}
impl SectionDefinition {
    pub fn new(namespace: &str, version: u32, schema: Value, defaults: Value) -> Result<Self> {
        if !valid_namespace(namespace) || version == 0 {
            return Err(SettingsError::Registration(
                "invalid namespace or zero schema version",
            ));
        }
        bounded(&schema)?;
        bounded(&defaults)?;
        let validator = jsonschema::validator_for(&schema)
            .map_err(|_| SettingsError::Registration("invalid or unresolved settings schema"))?;
        let definition = Self {
            descriptor: SettingsSection {
                namespace: namespace.into(),
                version,
                schema,
                defaults,
            },
            validator,
            migrations: BTreeMap::new(),
            revalidate: false,
        };
        definition.validate(&definition.descriptor.defaults)?;
        Ok(definition)
    }

    pub fn typed<T: Serialize + DeserializeOwned + Default + schemars::JsonSchema>(
        namespace: &str,
        version: u32,
    ) -> Result<Self> {
        Self::new(
            namespace,
            version,
            serde_json::to_value(schemars::schema_for!(T))?,
            serde_json::to_value(T::default())?,
        )
    }

    /// Register one transition N -> N+1. Missing transitions fail explicitly,
    /// preserving persisted settings instead of silently replacing them.
    pub fn migration(
        mut self,
        from_version: u32,
        migrate: impl Fn(Value) -> Result<Value> + Send + Sync + 'static,
    ) -> Result<Self> {
        if from_version == 0
            || from_version >= self.descriptor.version
            || self.migrations.contains_key(&from_version)
        {
            return Err(SettingsError::Registration(
                "invalid or duplicate settings migration",
            ));
        }
        self.migrations.insert(from_version, Arc::new(migrate));
        Ok(self)
    }

    /// A value stored under another schema version is checked against this
    /// schema instead of migrated: kept when valid, otherwise rebuilt from
    /// the defaults plus every stored property that still fits. For schemas
    /// a plugin ships, whose versions have no migrations.
    pub fn revalidating(mut self) -> Self {
        self.revalidate = true;
        self
    }

    /// The stored value made valid for this schema; see [`Self::revalidating`].
    pub(crate) fn salvage(&self, stored: Value) -> Value {
        if self.validator.is_valid(&stored) {
            return stored;
        }
        let mut value = self.descriptor.defaults.clone();
        if let (Value::Object(kept), Value::Object(stored)) = (&mut value, stored) {
            for (key, property) in stored {
                let previous = kept.insert(key.clone(), property);
                if !self.validator.is_valid(&Value::Object(kept.clone())) {
                    match previous {
                        Some(previous) => kept.insert(key, previous),
                        None => kept.remove(&key),
                    };
                }
            }
        }
        value
    }

    pub(crate) fn validate(&self, value: &Value) -> Result<()> {
        bounded(value)?;
        let paths: Vec<_> = self
            .validator
            .iter_errors(value)
            .take(8)
            .map(|error| error.instance_path().to_string())
            .collect();
        if !paths.is_empty() {
            return Err(SettingsError::Validation {
                namespace: self.descriptor.namespace.clone(),
                paths,
            });
        }
        Ok(())
    }
}

pub(crate) fn bounded(value: &Value) -> Result<()> {
    if serde_json::to_vec(value)?.len() > 64 * 1024 {
        return Err(SettingsError::Limit);
    }
    Ok(())
}

fn valid_namespace(namespace: &str) -> bool {
    namespace.len() <= 128
        && namespace.contains('.')
        && namespace.split('.').all(|part| {
            part.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn namespaces_allow_camel_case_segments_and_keep_identifier_boundaries() {
        for namespace in [
            "core.general",
            "core.appUpdates",
            "plugins.dev.my_plugin-v2",
        ] {
            assert!(
                SectionDefinition::new(namespace, 1, json!({}), json!({})).is_ok(),
                "{namespace}"
            );
        }
        for namespace in [
            "",
            "core",
            ".core",
            "core.",
            "core..updates",
            "Core.updates",
            "core.Updates",
            "core.2updates",
            "core.app/updates",
            "core.app updates",
            "core.app\u{00dc}pdates",
        ] {
            assert!(
                SectionDefinition::new(namespace, 1, json!({}), json!({})).is_err(),
                "{namespace}"
            );
        }
        assert!(
            SectionDefinition::new(
                &format!("core.{}", "a".repeat(124)),
                1,
                json!({}),
                json!({})
            )
            .is_err()
        );
        assert!(SectionDefinition::new("core.appUpdates", 0, json!({}), json!({})).is_err());
    }
}
