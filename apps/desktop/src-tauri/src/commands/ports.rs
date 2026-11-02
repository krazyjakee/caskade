//! Fan ports of library games (`caskade_core::ports`): decompilations,
//! recompilations and source ports from Decomp Buddy's install plans,
//! listed on game pages and installed as another release of the game.
use crate::error::{CommandError, report};
use caskade_core::{
    db::LibraryStore,
    hasheous::Hasheous,
    install::{InstallService, JobSnapshot},
    ports::{GamePorts, Ports},
};
use tauri::State;

/// The ports of a library game, surest first, and the catalogue's state.
#[tauri::command]
#[specta::specta]
pub async fn game_ports(
    ports: State<'_, Ports>,
    store: State<'_, LibraryStore>,
    hasheous: State<'_, Hasheous>,
    game_id: String,
) -> Result<GamePorts, CommandError> {
    ports
        .for_game(&store, Some(&hasheous), &game_id)
        .await
        .map_err(report)
}

/// Installs port `project` as another release of the game, into `root_id`
/// or the default root; the job follows like any install.
#[tauri::command]
#[specta::specta]
pub async fn game_port_install(
    ports: State<'_, Ports>,
    store: State<'_, LibraryStore>,
    installs: State<'_, InstallService>,
    game_id: String,
    project: String,
    root_id: Option<String>,
) -> Result<JobSnapshot, CommandError> {
    ports
        .install(&store, &installs, &game_id, &project, root_id)
        .await
        .map_err(report)
}
