//! Persistent session state and crash recovery (#108).
mod repository;
mod sqlite;
mod state;
pub use repository::SessionRepository;
pub use state::{RecoveryState, ScriptState, SessionPhase};
