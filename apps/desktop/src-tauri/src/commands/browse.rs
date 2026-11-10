//! The in-app path browser's listings (console mode's file and folder
//! picker). Read-only: it lists names, and the caller still checks
//! whatever path the user picks.
use crate::error::CommandError;
use caskade_app::commands::browse as app;
use caskade_core::browse::DirListing;
/// Lists `path` (home without one), hidden entries only with `show_hidden`.
#[tauri::command]
#[specta::specta]
pub async fn browse_dir(
    path: Option<String>,
    show_hidden: bool,
) -> Result<DirListing, CommandError> {
    app::browse_dir(path, show_hidden).await
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
