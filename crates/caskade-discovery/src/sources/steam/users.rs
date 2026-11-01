//! Per-install user data: the active account's playtime and last-played
//! times, and the compatibility tool chosen for each app.

use super::{
    super::{is_dir, join, list},
    vdf,
};
use crate::{env::HostEnv, types::ScanCtx};
use std::{collections::BTreeMap, path::Path};

const STEAM_ID_BASE: u64 = 76_561_197_960_265_728;

/// The `userdata/<account id>` of the most recent user that has one. With
/// no usable `loginusers.vdf` entry, a single `userdata` folder is
/// unambiguous.
pub(super) fn account(env: &HostEnv, root: &Path, ctx: &ScanCtx) -> Option<u64> {
    let userdata = join(root, "userdata");
    let folders: Vec<u64> = list(env, &userdata)
        .unwrap_or_default()
        .iter()
        .filter_map(|name| name.parse().ok())
        .filter(|id| *id != 0)
        .collect();
    let file = join(root, "config/loginusers.vdf");
    let mut users: Vec<_> = vdf::load(env, &file, &vdf::small())
        .ok()
        .and_then(|(value, _)| {
            let users = vdf::child(&value, "users")?;
            Some(
                vdf::entries(users)
                    .iter()
                    .filter_map(|(id, user)| {
                        let account = id.parse::<u64>().ok()?.checked_sub(STEAM_ID_BASE)?;
                        let recent = vdf::text(user, "MostRecent") == Some("1");
                        let timestamp = vdf::number::<u64>(user, "Timestamp").unwrap_or(0);
                        Some((recent, timestamp, std::cmp::Reverse(account)))
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();
    users.sort();
    let from_login = users
        .iter()
        .rev()
        .map(|(_, _, std::cmp::Reverse(account))| *account)
        .find(|account| folders.contains(account));
    from_login.or_else(|| match folders.as_slice() {
        [only] => Some(*only),
        [] => None,
        _ => {
            ctx.warn("several Steam users and none marked most recent; playtime skipped");
            None
        }
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Played {
    pub playtime_secs: Option<u64>,
    pub last_played_unix: Option<u64>,
}

/// Playtime and last played per app from the account's `localconfig.vdf`.
pub(super) fn played(
    env: &HostEnv,
    root: &Path,
    account: u64,
    ctx: &ScanCtx,
) -> BTreeMap<u32, Played> {
    let config = join(root, &format!("userdata/{account}/config"));
    let file = join(&config, "localconfig.vdf");
    if !is_dir(env, &config) {
        return BTreeMap::new();
    }
    let value = match vdf::load(env, &file, &vdf::large()) {
        Ok((value, _)) => value,
        Err(error) => {
            ctx.warn(error.to_string());
            return BTreeMap::new();
        }
    };
    let apps = vdf::path(
        &value,
        &["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"],
    );
    apps.map(vdf::entries)
        .unwrap_or_default()
        .iter()
        .filter_map(|(id, app)| {
            let played = Played {
                playtime_secs: vdf::number::<u64>(app, "Playtime")
                    .filter(|minutes| *minutes > 0)
                    .map(|minutes| minutes.saturating_mul(60)),
                last_played_unix: vdf::number(app, "LastPlayed").filter(|time| *time > 0),
            };
            (played != Played::default()).then_some((id.parse().ok()?, played))
        })
        .collect()
}

/// `CompatToolMapping` from `config/config.vdf`: app id (0 is the default
/// for every app) to tool name.
pub(super) fn compat_tools(env: &HostEnv, root: &Path, ctx: &ScanCtx) -> BTreeMap<u32, String> {
    let file = join(root, "config/config.vdf");
    let value = match vdf::load(env, &file, &vdf::large()) {
        Ok((value, _)) => value,
        Err(_) => return BTreeMap::new(),
    };
    let mapping = vdf::path(
        &value,
        &[
            "InstallConfigStore",
            "Software",
            "Valve",
            "Steam",
            "CompatToolMapping",
        ],
    );
    if mapping.is_none() {
        ctx.warn("config.vdf has no CompatToolMapping");
    }
    mapping
        .map(vdf::entries)
        .unwrap_or_default()
        .iter()
        .filter_map(|(id, tool)| {
            let name = vdf::text(tool, "name")?.trim();
            (!name.is_empty()).then(|| Some((id.parse().ok()?, name.to_owned())))?
        })
        .collect()
}
