//! Namespaced JSON Schema settings, persisted revisions and explicit migrations.
mod builtin;
mod installer;
pub use installer::{InstallerSettings, SourceHandling};
mod registry;
mod schema_text;
mod service;
#[cfg(test)]
mod tests;
pub use builtin::{
    AccountsSettings, AppUpdateSettings, AppearanceSettings, ConsoleSettings, DeepScanSettings,
    DiscoveryExclusion, DiscoverySettings, EmulationSettings, GeneralSettings, LauncherSettings,
    MetadataSettings, OnboardingSettings, SavesSettings, StorageSettings, UpdateSettings,
};
pub use registry::{SectionDefinition, SettingsSection};
pub use service::{SettingsChanged, SettingsService, SettingsSnapshot};
pub type Result<T> = std::result::Result<T, SettingsError>;

#[derive(Debug)]
pub enum SettingsError {
    Store(crate::Error),
    Json(serde_json::Error),
    /// A section definition that can't be registered; the text is for
    /// developers.
    Registration(&'static str),
    Unknown(String),
    Validation {
        namespace: String,
        paths: Vec<String>,
    },
    Conflict,
    Version,
    Migration,
    Stored,
    Limit,
    Internal,
}

/// The text is for people, in the core's language (#214).
impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::Store(error) => crate::tr!("settings store: {error}", error = error),
            Self::Json(error) => crate::tr!("settings JSON: {error}", error = error),
            Self::Registration(why) => crate::tr!("settings registration: {why}", why = why),
            Self::Unknown(section) => {
                crate::tr!("unknown settings section: {section}", section = section)
            }
            Self::Validation { namespace, paths } => crate::tr!(
                "invalid settings for {namespace} at {paths}",
                namespace = namespace,
                paths = format!("{paths:?}")
            ),
            Self::Conflict => crate::tr!("settings changed concurrently; reload before saving"),
            Self::Version => crate::tr!("settings schema is newer than this application"),
            Self::Migration => crate::tr!("settings migration is unavailable"),
            Self::Stored => crate::tr!("stored settings envelope is invalid"),
            Self::Limit => crate::tr!("settings exceed the supported size or revision budget"),
            Self::Internal => crate::tr!("settings registry is unavailable"),
        })
    }
}

impl std::error::Error for SettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<crate::Error> for SettingsError {
    fn from(error: crate::Error) -> Self {
        Self::Store(error)
    }
}

impl From<serde_json::Error> for SettingsError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
