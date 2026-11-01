//! Domain model: Game, GameEntry, StorageRoot, LaunchSpec, … (#105).
//! Persisted domain and IPC DTOs. Store identities and discovery launch targets
//! are reused; managed installations retain only root IDs and relative paths.
mod action;
mod edit;
mod game;
mod launch;
mod records;
pub(crate) mod validation;
pub use caskade_discovery::{ExternalId, GameRuntime, InstallState, SourceId, TrackingHint};
pub use action::*;
pub use edit::*;
pub use game::*;
pub use launch::*;
pub use records::*;

/// Opaque association/entity identifier, independent of titles and file paths.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}
