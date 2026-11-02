//! Installer preferences; the pipeline consumes a fresh snapshot for each run.
use caskade_installer::{
    pipeline::{CompatibilityRunner, InstallerConfig},
    plan::{ExecutionPolicy, Policy, PresetArgs, WindowsMethod},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceHandling {
    #[default]
    #[schemars(title = "Keep source")]
    Keep,
    #[schemars(title = "Move to trash")]
    Trash,
    #[schemars(title = "Delete source")]
    Delete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallerSettings {
    #[schemars(title = "Windows install method")]
    /// Run supported installers on Windows, or prefer extracting their files.
    pub windows_method: WindowsMethod,
    #[schemars(title = "Execution policy")]
    /// Allow installer processes, ask before running them, or only use plans that do not execute code.
    pub execution: ExecutionPolicy,
    #[schemars(title = "Execution policy for downloads")]
    /// For games downloaded from a link (deep links, pasted or dropped URLs, magnet links and torrents, updates). The stricter of this and the execution policy applies.
    pub download_execution: ExecutionPolicy,
    #[schemars(title = "Ask below confidence", range(min = 0, max = 1))]
    /// Pause a job when its best plan scores below this probability.
    pub ask_below_confidence: f32,
    #[schemars(
        title = "Installer language",
        length(max = 64),
        regex(pattern = "^[A-Za-z0-9_-]*$")
    )]
    /// Leave empty for the installer's default. Unsupported language switches are reported by its handler.
    pub language: String,
    #[schemars(title = "Let installers create shortcuts")]
    /// Allow installer-created shortcuts where the package supports this preference.
    pub let_installers_create_shortcuts: bool,
    #[schemars(title = "Compatibility runner")]
    /// Proton uses umu. GE-Proton and Steam Proton use detected builds; system Wine uses the detected Wine executable. Applies to new jobs.
    pub compatibility_runner: CompatibilityRunner,
    #[schemars(title = "Source handling after install")]
    /// Keep, move to the system trash, or delete the selected local source after a successful managed install. Downloads have their own setting.
    pub source_handling: SourceHandling,
    #[schemars(title = "Keep downloads")]
    /// Retain downloaded installers after successful installation.
    pub keep_downloads: bool,
    #[schemars(title = "Concurrent downloads", range(min = 1, max = 8))]
    /// Maximum simultaneous downloads. Active downloads finish when this limit is lowered.
    pub concurrent_downloads: u32,
}
impl Default for InstallerSettings {
    fn default() -> Self {
        Self {
            windows_method: WindowsMethod::RunInstaller,
            execution: ExecutionPolicy::Allow,
            // Installs run without a question: a silent run, then extraction,
            // then an interactive run. Ask is a choice in Settings → Installer.
            download_execution: ExecutionPolicy::Allow,
            ask_below_confidence: 0.5,
            language: String::new(),
            let_installers_create_shortcuts: false,
            compatibility_runner: CompatibilityRunner::Proton,
            source_handling: SourceHandling::Keep,
            keep_downloads: false,
            concurrent_downloads: 2,
        }
    }
}
impl InstallerSettings {
    pub fn config(&self) -> InstallerConfig {
        InstallerConfig {
            policy: Policy {
                windows_method: self.windows_method,
                execution: self.execution,
                ask_below_confidence: self.ask_below_confidence,
                preset_args: PresetArgs {
                    language: (!self.language.is_empty()).then(|| self.language.clone()),
                    no_shortcuts: !self.let_installers_create_shortcuts,
                },
            },
            compatibility_runner: self.compatibility_runner,
        }
    }
    /// The execution policy for a job: a download from a link takes the
    /// stricter of the two policies (CSK-02).
    pub fn execution_for(&self, download: bool) -> ExecutionPolicy {
        use ExecutionPolicy::{Allow, Ask, Never};
        if !download {
            return self.execution;
        }
        match (self.execution, self.download_execution) {
            (Never, _) | (_, Never) => Never,
            (Ask, _) | (_, Ask) => Ask,
            (Allow, Allow) => Allow,
        }
    }
}
