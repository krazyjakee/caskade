//! Caskade's application layer: what an app shell does besides drawing
//! windows. The Tauri app (`apps/desktop/src-tauri`) and the native app
//! both run it, so its state, permissions, events and recovery behave the
//! same in each. Nothing here depends on Tauri or any other UI toolkit.

pub mod background;
pub mod commands;
pub mod deep_link;
pub mod discovery;
pub mod error;
pub mod events;
pub mod navigation;
pub mod onboarding;
pub mod permissions;
pub mod recovery;
pub mod runtime;
pub mod torrent_links;
