//! How a game page names a release's source and install state (the
//! webview's `api/presentation.ts`, `formatSource` and `formatInstallState`).

use caskade_core::model::InstallState;
use caskade_ui_i18n::{t, tp};

/// Stores and launchers by their own names.
const STORE_NAMES: &[(&str, &str)] = &[
    ("amazon", "Amazon"),
    ("battlenet", "Battle.net"),
    ("ea", "EA app"),
    ("epic", "Epic"),
    ("epic.app", "Epic"),
    ("flatpak", "Flatpak"),
    ("gog", "GOG"),
    ("heroic", "Heroic"),
    ("itch", "itch.io"),
    ("legendary", "Epic"),
    ("lutris", "Lutris"),
    ("minigalaxy", "GOG"),
    ("steam", "Steam"),
    ("ubisoft", "Ubisoft"),
    ("xbox", "Xbox"),
];

/// A library source (an entry's source id) for people: `romm:<server>` is
/// RomM, `steam.shortcuts` Steam shortcuts, and so on.
pub fn source(source: &str) -> String {
    if source.starts_with("romm:") {
        return "RomM".to_owned();
    }
    if let Some(plugin) = source.strip_prefix("plugin:") {
        return plugin.to_owned();
    }
    if source.starts_with("store:") {
        return tp!("library source", "Store account");
    }
    if source.starts_with("stream:") {
        return tp!("library source", "Streaming");
    }
    if source.starts_with("peer:") {
        return tp!("library source", "Another of your devices");
    }
    match source {
        // Games added to Steam by hand.
        "steam.shortcuts" => t!("Steam shortcuts"),
        "desktop" => tp!("library source", "Desktop entries"),
        "roms" => tp!("library source", "ROM folders"),
        "web" => tp!("library source", "Web games"),
        "caskade-storage" | "caskade-handoff" => tp!("library source", "Installs"),
        "manual" => tp!("library source", "Added by hand"),
        other => STORE_NAMES
            .iter()
            .find(|(id, _)| *id == other)
            .map(|(_, name)| (*name).to_owned())
            .unwrap_or_else(|| {
                let mut chars = other.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().chain(chars).collect())
                    .unwrap_or_default()
            }),
    }
}

/// A release's install state for people.
pub fn install_state(state: &InstallState) -> String {
    match state {
        InstallState::Installed => tp!("release state", "Installed"),
        InstallState::Updating => tp!("release state", "Updating"),
        InstallState::Partial => tp!("release state", "Partly installed"),
        InstallState::Broken { .. } => tp!("release state", "Broken"),
    }
}

/// The state's id, as `data-status` names it.
pub fn state_id(state: &InstallState) -> &'static str {
    match state {
        InstallState::Installed => "installed",
        InstallState::Updating => "updating",
        InstallState::Partial => "partial",
        InstallState::Broken { .. } => "broken",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_named_as_the_webview_names_them() {
        assert_eq!(source("steam"), "Steam");
        assert_eq!(source("romm:home"), "RomM");
        assert_eq!(source("plugin:acme"), "acme");
        assert_eq!(source("manual"), "Added by hand");
        assert_eq!(source("caskade-storage"), "Installs");
        assert_eq!(source("bottles"), "Bottles");
    }
}
