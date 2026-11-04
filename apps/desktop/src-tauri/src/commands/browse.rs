//! The in-app path browser's listings (console mode's file and folder
//! picker). Read-only: it lists names, and the caller still checks
//! whatever path the user picks.
use crate::error::{CommandError, internal};
use caskade_core::browse::{self, DirListing};
use std::path::PathBuf;

/// Lists `path` (home without one), hidden entries only with `show_hidden`.
#[tauri::command]
#[specta::specta]
pub async fn browse_dir(
    path: Option<String>,
    show_hidden: bool,
) -> Result<DirListing, CommandError> {
    tokio::task::spawn_blocking(move || {
        browse::browse_dir(path.map(PathBuf::from).as_deref(), show_hidden)
    })
    .await
    .map_err(internal)?
    .map_err(CommandError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn a_folder_that_can_t_be_listed_is_a_report() {
        let error =
            tauri::async_runtime::block_on(browse_dir(Some("games".into()), false)).unwrap_err();
        assert_eq!(error.code, ErrorCode::Other);
        assert!(error.summary.contains("absolute path"), "{error:?}");
    }
}
