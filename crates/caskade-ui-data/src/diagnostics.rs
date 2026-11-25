//! Diagnostics for Settings → About (#176): opening the logs folder through
//! the app layer, and formatting privacy-safe system information.

use caskade_app::commands::diagnostics;
use caskade_core::{AppPaths, error::ErrorReport};

pub use caskade_app::commands::diagnostics::{
    RendererMode, SystemInfo, detect_gpu_adapter, detect_os_version, format_system_info,
};

type Result<T> = std::result::Result<T, ErrorReport>;

/// What Settings → About uses to reach diagnostics commands.
#[derive(Clone)]
pub struct Diagnostics {
    paths: AppPaths,
}

impl Diagnostics {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    /// The running app's diagnostics.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self::new(services.paths.clone())
    }

    /// Opens Caskade's logs folder in the OS file manager.
    pub async fn open_logs_folder(&self) -> Result<()> {
        diagnostics::open_logs_folder(&self.paths).await
    }
}
