//! Which installed apps are games: the `appinfo.vdf` app type, then
//! `data/steam-tools.toml`, then well-known tool names.

use super::{
    super::{is_file, join, read},
    vdf,
};
use crate::{
    data::Data,
    env::HostEnv,
    parsers::{Limits, binary_vdf},
    types::ScanCtx,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone, Default)]
pub(super) struct AppInfo {
    /// `common.type`, lowercased.
    pub kind: Option<String>,
    pub name: Option<String>,
}

const APPINFO_LIMIT: usize = 256 << 20;

/// `appcache/appinfo.vdf` entries for the wanted app ids.
pub(super) fn app_info(
    env: &HostEnv,
    root: &Path,
    wanted: &BTreeSet<u32>,
    ctx: &ScanCtx,
) -> BTreeMap<u32, AppInfo> {
    let file = join(root, "appcache/appinfo.vdf");
    if wanted.is_empty() || !is_file(env, &file) {
        return BTreeMap::new();
    }
    // Node budgets apply per selected app; unwanted entries are skipped.
    let limits = Limits {
        max_input_bytes: APPINFO_LIMIT,
        ..Limits::default()
    };
    let parsed = read(env, &file, APPINFO_LIMIT as u64)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            binary_vdf::parse_appinfo_selected(&bytes, &limits, wanted)
                .map_err(|error| error.to_string())
        });
    match parsed {
        Ok(info) => info
            .apps
            .into_iter()
            .filter(|app| wanted.contains(&app.app_id))
            .map(|app| {
                let common = vdf::path(&app.data, &["appinfo", "common"]);
                let field = |name| {
                    common
                        .and_then(|common| vdf::text(common, name))
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                };
                let info = AppInfo {
                    kind: field("type").map(str::to_ascii_lowercase),
                    name: field("name").map(str::to_owned),
                };
                (app.app_id, info)
            })
            .collect(),
        Err(error) => {
            ctx.warn(format!("{}: {error}; using the tool list", file.display()));
            BTreeMap::new()
        }
    }
}

/// Why an installed app is not a game, or `None` for a game.
pub(super) fn not_a_game(
    app_id: u32,
    info: Option<&AppInfo>,
    name: &str,
    install_dir: &str,
) -> Option<String> {
    if let Some(kind) = info.and_then(|info| info.kind.as_deref()) {
        return (!matches!(kind, "game" | "demo" | "mod")).then(|| format!("app type {kind}"));
    }
    if Data::builtin().ok().is_some_and(|data| {
        data.steam_tools
            .tools
            .iter()
            .any(|tool| tool.app_id == app_id)
    }) {
        return Some("listed in steam-tools.toml".into());
    }
    let tool_name = name == "Proton Experimental"
        || name == "Proton Hotfix"
        || name.starts_with("Proton ")
        || name.starts_with("Steam Linux Runtime")
        || name == "Steamworks Common Redistributables"
        || install_dir.starts_with("SteamLinuxRuntime")
        || install_dir == "Steamworks Shared";
    tool_name.then(|| "tool name".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(kind: &str) -> AppInfo {
        AppInfo {
            kind: Some(kind.into()),
            name: None,
        }
    }

    #[test]
    fn games_and_tools() {
        assert_eq!(
            not_a_game(620, Some(&info("game")), "Portal 2", "Portal 2"),
            None
        );
        assert_eq!(not_a_game(1, Some(&info("demo")), "A demo", "A"), None);
        assert!(not_a_game(1493710, Some(&info("tool")), "Proton Experimental", "x").is_some());
        assert!(not_a_game(5, Some(&info("dlc")), "Soundtrack", "x").is_some());
        assert!(not_a_game(6, Some(&info("application")), "Editor", "x").is_some());
        // The app type wins over a tool-like name.
        assert_eq!(
            not_a_game(7, Some(&info("game")), "Proton Quest", "x"),
            None
        );
        // Without appinfo: names, then a plain game.
        assert!(not_a_game(1628350, None, "Steam Linux Runtime 3.0 (sniper)", "x").is_some());
        assert!(not_a_game(8, None, "Something", "SteamLinuxRuntime_soldier").is_some());
        assert!(
            not_a_game(
                228980,
                None,
                "Steamworks Common Redistributables",
                "Steamworks Shared"
            )
            .is_some()
        );
        assert!(not_a_game(2180100, None, "Proton Hotfix", "Proton Hotfix").is_some());
        assert_eq!(not_a_game(9, None, "Fixture Game", "Fixture Game"), None);
    }
}
