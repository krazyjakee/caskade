//! Resolve URLs and download inputs before the local installation pipeline.
//!
//! HTTP redirects are followed explicitly (at most ten, never HTTPS to HTTP);
//! a plugin's download stays within its network grant (`grant`).
//! Persisted validators and the actual partial-file length determine resume;
//! a changed entity or a server ignoring Range restarts from zero. All names and
//! writes stay beneath a capability for the selected root's download directory.

mod api;
mod engine;
pub mod grant;
mod http;
pub mod local;
mod names;
mod resolver;
pub mod resolvers;
mod storage;
#[cfg(feature = "torrent")]
pub mod torrent;

pub(crate) use api::describe_http_error;
pub use api::{
    DownloadRequest, DownloadedFile, FetchConfig, FetchError, Resolution, ResolveFuture, Result,
    UrlResolver,
};
pub use engine::FetchEngine;
pub use local::{LocalSource, Near};
pub use names::{disposition_name, safe_file_name};
pub use resolver::{Direct, ResolverRegistry};

pub use resolvers::ResolverConfig;
