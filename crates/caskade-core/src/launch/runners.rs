use std::path::PathBuf;

/// Machine-local runner configuration; never persisted into a managed entry's
/// root-relative launch spec. Settings can replace these paths at app startup.
#[derive(Debug, Clone)]
pub struct RunnerPaths {
    pub wine: PathBuf,
    pub crossover: Option<PathBuf>,
    pub umu_run: PathBuf,
    pub proton_path: String,
}
impl Default for RunnerPaths {
    fn default() -> Self {
        Self {
            wine: "wine".into(),
            crossover: None,
            umu_run: caskade_installer::runtime::umu::UmuBootstrap::for_host()
                .and_then(|b| b.installed())
                .unwrap_or_else(|| "umu-run".into()),
            proton_path: "UMU-Proton".into(),
        }
    }
}

/// Resolve machine-local runner paths for the current launch. Managed entries
/// retain only portable runner hints; implementations may read current settings.
#[async_trait::async_trait]
pub trait RunnerResolver: Send + Sync {
    async fn resolve(&self, entry: &crate::model::GameEntry) -> super::Result<RunnerPaths>;
}
