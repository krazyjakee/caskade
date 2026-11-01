use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors that end a whole discovery call. A failing source never does; it is
/// reported in its `SourceReport` instead.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Data(#[from] crate::data::DataError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Dump(#[from] crate::dump::DumpError),
    #[error("{0}")]
    Usage(String),
    #[error("watch: {0}")]
    Watch(String),
}
