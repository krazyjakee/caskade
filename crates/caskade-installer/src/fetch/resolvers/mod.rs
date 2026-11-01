//! Built-in public share/release resolvers. Metadata requests are bounded and
//! use the downloader's explicit redirect/downgrade policy. No resolver writes
//! files, authenticates accounts, or executes downloaded content.
mod common;
mod drive;
mod dropbox;
mod github;

pub use common::ResolverConfig;
pub use drive::GoogleDriveResolver;
pub use dropbox::DropboxResolver;
pub use github::GitHubResolver;

use super::{Result, UrlResolver};
use std::sync::Arc;

pub(super) fn builtins(config: ResolverConfig) -> Result<Vec<Arc<dyn UrlResolver>>> {
    let http = common::MetadataHttp::new(config)?;
    Ok(vec![
        Arc::new(GitHubResolver::from_http(http.clone())),
        Arc::new(DropboxResolver::from_http(http.clone())),
        Arc::new(GoogleDriveResolver::from_http(http)),
    ])
}
