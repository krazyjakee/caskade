use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Preferred method on Windows. Other hosts prefer extracting Windows inputs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum WindowsMethod {
    #[default]
    #[schemars(title = "Run the installer")]
    RunInstaller,
    #[schemars(title = "Extract the files")]
    Extract,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPolicy {
    #[schemars(title = "Never run installers")]
    Never,
    #[schemars(title = "Ask first")]
    Ask,
    #[default]
    #[schemars(title = "Allow")]
    Allow,
}

/// Family handlers translate these preferences into supported argument vectors.
/// The engine never appends generic switches to format-specific commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(default)]
pub struct PresetArgs {
    /// None lets the installer use its own language default.
    pub language: Option<String>,
    pub no_shortcuts: bool,
}

impl Default for PresetArgs {
    fn default() -> Self {
        Self {
            language: None,
            no_shortcuts: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(default)]
pub struct Policy {
    pub windows_method: WindowsMethod,
    pub execution: ExecutionPolicy,
    /// Finite probability in 0..=1. Equality does not require a question.
    #[schemars(range(min = 0, max = 1))]
    pub ask_below_confidence: f32,
    pub preset_args: PresetArgs,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            windows_method: WindowsMethod::RunInstaller,
            execution: ExecutionPolicy::Allow,
            ask_below_confidence: 0.5,
            preset_args: PresetArgs::default(),
        }
    }
}
