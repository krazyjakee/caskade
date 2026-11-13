//! Caskade's application layer: what an app shell does besides drawing
//! windows. The native app (`apps/native`) runs it; nothing here depends on
//! a UI toolkit, so its state, permissions, events and recovery are tested
//! without a window.

pub mod background;
pub mod commands;
pub mod deep_link;
pub mod dev_plugins;
pub mod discovery;
pub mod error;
pub mod events;
pub mod game_focus;
pub mod launch_cli;
pub mod link_handler;
pub mod logging;
pub mod navigation;
pub mod onboarding;
pub mod permissions;
pub mod recovery;
pub mod runtime;
pub mod self_update;
pub mod single_instance;
pub mod start;
pub mod torrent_links;
pub mod tray;
