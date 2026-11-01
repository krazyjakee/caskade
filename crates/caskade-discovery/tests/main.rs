//! Integration tests, compiled as one binary: a change relinks once instead
//! of once per file. Run one file's tests with `cargo test --test it <file>::`.
//! `performance` stays a separate binary (see Cargo.toml).

#[path = "fixtures/support/mod.rs"]
mod fixtures;

mod battlenet;
mod bottles;
mod classify;
mod classify_cli;
mod classify_fs;
mod deep;
mod desktop;
mod ea;
mod env;
mod epic;
mod epic_merge;
mod fixture_dump;
mod fixture_dump_cli;
mod gameux;
mod gameux_native;
mod gog;
mod hardening;
mod heroic;
mod identity;
mod itch;
mod linux_integration;
mod linux_live;
mod lutris;
mod macos_apps;
mod macos_launchers;
mod macos_native;
mod merge;
mod orchestrator;
mod parsers;
mod parsers_sqlite;
mod prefixes;
mod probes;
mod registry;
mod registry_native;
mod small_launchers;
mod startmenu;
mod steam;
mod steam_shortcuts;
mod types;
mod ubisoft;
mod uninstall;
mod uri;
mod watch;
mod windows_integration;
mod windows_integration_native;
mod windows_launchers;
mod xbox;
