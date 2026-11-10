//! Fan ports of library games (`caskade_core::ports`): decompilations,
//! recompilations and source ports from Decomp Buddy's install plans,
//! listed on game pages and installed as another release of the game.
use caskade_core::{
    db::LibraryStore,
    hasheous::Hasheous,
    install::{InstallService, JobSnapshot},
    ports::{GamePorts, Ports},
};

use crate::error::{CommandError, report};

/// The ports of a library game, surest first, and the catalogue's state.
pub async fn game_ports(
    ports: &Ports,
    store: &LibraryStore,
    hasheous: &Hasheous,
    game_id: &str,
) -> Result<GamePorts, CommandError> {
    ports
        .for_game(store, Some(hasheous), game_id)
        .await
        .map_err(report)
}

/// Installs port `project` as another release of the game, into `root_id`
/// or the default root; the job follows like any install.
pub async fn game_port_install(
    ports: &Ports,
    store: &LibraryStore,
    installs: &InstallService,
    game_id: &str,
    project: &str,
    root_id: Option<String>,
) -> Result<JobSnapshot, CommandError> {
    ports
        .install(store, installs, game_id, project, root_id)
        .await
        .map_err(report)
}
