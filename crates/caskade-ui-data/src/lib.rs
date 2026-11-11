//! The native UI's data layer (#89): what views read and change, over
//! `caskade-app`, with no IPC, no JSON and no query cache.
//!
//! - [`Library`] reads and changes library data through the app layer's
//!   commands, with the core's [`ErrorReport`]s (codes, hints, references).
//!   Pages stay bounded ([`PAGE_LIMIT`]).
//! - [`Settings`] reads and changes settings sections, and says which ones
//!   the signed-in account may change.
//! - [`Key`] names what a view reads; [`Changed::from_event`] says which keys
//!   an app event makes stale, so a view refetches only what changed.
//! - [`fixtures`] is an in-memory library for UI development and tests:
//!   the real core and app layer over synthetic games, the same ones the
//!   webview's `fixtures.ts` shows.
//! - [`labels`] are the recovery hints' words, as the webview has them.
//!
//! No UI toolkit: the native app's hooks (`apps/native/src/data`) turn keys
//! and loads into signals and resources.

pub mod accounts;
pub mod addons;
pub mod backups;
pub mod devices;
pub mod discovery;
pub mod fixtures;
pub mod jobs;
pub mod keys;
pub mod labels;
pub mod library;
mod manual;
pub mod pairing;
pub mod plugins;
pub mod scripts;
pub mod settings;
pub mod shares;
pub mod storage;
pub mod stores;
pub mod streaming;
pub mod sync;
pub mod themes;

pub use caskade_core::error::{ErrorCode, ErrorReport, RecoveryHint};
pub use discovery::Discovery;
pub use keys::{Changed, Key};
pub use library::{Library, PAGE_LIMIT, SEARCH_LIMIT};
pub use settings::{Editable, Settings};
pub use themes::Themes;
