//! The shell's chooser for games to install: a shell opens its own file
//! dialog; tests inject one without opening an OS dialog or granting the
//! UI filesystem access.
use std::{future::Future, pin::Pin, sync::Arc};

/// What a chooser answers, when the user has chosen.
pub type Selection<T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send>>;

pub struct InstallPicker {
    pub files: Arc<dyn Fn() -> Selection<Vec<String>> + Send + Sync>,
    pub folder: Arc<dyn Fn() -> Selection<Option<String>> + Send + Sync>,
}
