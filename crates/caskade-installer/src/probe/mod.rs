//! Identify input artifacts statically, without executing their contents.
//!
//! `ProbeEngine::probe_reader` only reads the supplied stream. `probe_path` is
//! the filesystem edge adapter. Neither detects the host or executes input code.
//! Containers are identified immediately but listed only on an explicit request
//! through a registered `FormatProbe`; archive parsers register those in #9.

pub use crate::types::{ArtifactId, ArtifactNode, Format, PayloadPlatform, Role};

mod reader;
pub use reader::{CountingReader, ReadStats};

mod engine;
mod magic;
pub(crate) use magic::compression;
mod pe;
mod source;
pub(crate) use pe::parse_pe;
pub use pe::{PeMetadata, PeProbe, PeProbeLimits};

pub use engine::{Detection, FormatProbe, Probe, ProbeEngine, ProbeError, ProbeLimits};
pub use source::{ProbeSource, ReadSeek};
