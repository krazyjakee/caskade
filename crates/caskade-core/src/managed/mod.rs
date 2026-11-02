//! Managed programs (STREAMING-PLAN §3.1): third-party programs Caskade
//! downloads, installs, finds, keeps up to date and removes on the user's
//! behalf. Emulator setup (`emulation::setup`) and helper tools (the
//! streaming host and client, [`ManagedTools`]) share these steps:
//!
//! - [`resolve`]: a source's newest version and file (GitHub releases found
//!   without the API, Dolphin's endpoint, a fixed URL).
//! - `download`: the file into the cache, with progress.
//! - `release`: unpacked beside the install, found with the detect rules,
//!   swapped in with the user's files carried over; removal deletes only
//!   what releases shipped.
//! - `flatpak`: Flathub installs for the user.
//! - [`update_available`] and [`version_label`]: newer or not, for people.

#[cfg(feature = "net")]
pub(crate) mod download;
mod env;
pub(crate) mod flatpak;
mod release;
pub(crate) mod source;
mod tool;
mod tools;
pub(crate) mod unpack;
mod versions;

/// They stand a shell script in for `flatpak`.
#[cfg(all(test, feature = "net", unix))]
mod tests;

pub use env::outside_own_appimage;
pub(crate) use release::{Release, install_release, remove_release};
pub use source::{Hosts, Resolved, resolve};
pub use tokio_util::sync::CancellationToken;
pub use tool::{Arch, Condition, Host, ToolCatalog, ToolDefinition, ToolInstall, ToolSource};
pub use tools::{ManagedTool, ManagedTools, tool_label};
pub use versions::{update_available, version_label};
