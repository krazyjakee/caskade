//! Disc images are inspected with the pinned 7zz sidecar without mounting.
//!
//! Open a `DiscSession` at the application boundary. Console evidence produces
//! a ROM hand-off containing the complete original set. A PC session exposes a
//! re-probed installer and physical sources for the ordinary family planner and
//! executor. Retain the session until execution finishes. Insert-next-disc uses
//! interactive fallback; no automatic mounting or sector rewriting is performed.
mod cue;
mod probe;
mod session;
pub use cue::{DataTrack, SectorReader, parse_cue};
pub use probe::DiscProbe;
pub use session::{DiscKind, DiscSession};

pub type Result<T> = std::result::Result<T, DiscError>;
#[derive(Debug, thiserror::Error)]
pub enum DiscError {
    #[error("disc I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("disc path safety: {0}")]
    Safety(#[from] crate::safety::SafetyError),
    #[error("disc sidecar: {0}")]
    Sidecar(#[from] crate::formats::archive::sevenzip::SevenZipError),
    #[error("disc extraction control: {0}")]
    Control(#[from] crate::formats::archive::ArchiveError),
    #[error("invalid disc: {0}")]
    Invalid(String),
    #[error("unsupported disc: {0}")]
    Unsupported(String),
    #[error("disc limit exceeded: {0}")]
    Limit(&'static str),
}
