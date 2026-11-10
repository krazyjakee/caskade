//! The in-app path browser's listings (console mode's file and folder
//! picker). Read-only: it lists names, and the caller still checks
//! whatever path the user picks.
use crate::error::{CommandError, internal};
use caskade_core::browse::{self, DirListing};
use std::path::PathBuf;
/// Lists `path` (home without one), hidden entries only with `show_hidden`.
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
