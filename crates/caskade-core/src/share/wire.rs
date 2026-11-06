//! The `share` session's frames: one JSON object per line, like copies.

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

/// The longest frame read on a share session.
pub(super) const MAX_LINE: usize = 64 * 1024;
/// The longest `About` frame, which only a recipient reads: a shared
/// collection's list rides in it (#248).
pub(super) const MAX_ABOUT: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(super) enum Message {
    /// Recipient: who opened the link, and whether to receive it now or only
    /// look at it.
    Hello { name: String, fetch: bool },
    /// Sharer: what the share is.
    About {
        kind: String,
        title: String,
        size: Option<u64>,
        windows: bool,
        sharer: String,
        expires_at_ms: Option<i64>,
        extras: Vec<String>,
        /// What a share other than a game is (#248, `items::Details`).
        #[serde(default)]
        details: Option<serde_json::Value>,
    },
    /// Sharer: still reading the files.
    Preparing { done: u64, total: u64 },
    /// Sharer: here it is. `addrs` are the sharer's own direct addresses.
    Offer {
        root: String,
        size: u64,
        files: u64,
        extras_root: Option<String>,
        launch: Option<String>,
        addrs: Vec<SocketAddr>,
    },
    /// Sharer: the share's other recipients that serve it, by endpoint id.
    Providers {
        list: Vec<(String, Vec<SocketAddr>)>,
    },
    /// Sharer, to a recipient that serves: let these fetch it from you too.
    Allow { peers: Vec<String> },
    /// Recipient: done, they may.
    Allowed,
    /// Sharer: not this, and why.
    Refused { reason: String },
    /// Sharer: the share was stopped or expired.
    Stopped,
    /// Recipient: bytes of the share it has.
    Progress { done: u64 },
    /// Recipient: it serves the share at `addrs` (none: no longer).
    Serving { addrs: Vec<SocketAddr> },
    /// Recipient: everything arrived.
    Done,
    /// Both ways: no plain dial reaches the sharer, so this side opened a
    /// port for these files at `addrs`. Empty: it couldn't.
    Punch { addrs: Vec<SocketAddr> },
}
