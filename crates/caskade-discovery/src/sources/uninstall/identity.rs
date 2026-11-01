use super::*;
/// Launcher registrations only enrich an independently discovered owner.
pub const ALIAS_RULE: &str = "alias.launcher_owned";
pub(super) fn launcher_ids(record: &UninstallRecord) -> (bool, Vec<ExternalId>) {
    let name = record.name.to_ascii_lowercase();
    let mut ids = vec![];
    if let Some(id) = name
        .strip_prefix("steam app ")
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|id| *id > 0)
    {
        ids.push(ExternalId::Steam { app_id: id });
    }
    if let Some(id) = name
        .strip_prefix("uplay install ")
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|id| *id > 0)
    {
        ids.push(ExternalId::Ubisoft { id });
    }
    let gog = name
        .strip_prefix("gog ")
        .unwrap_or(&name)
        .strip_suffix("_is1")
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|id| *id > 0);
    if let Some(product_id) = gog {
        ids.push(ExternalId::Gog { product_id });
    }
    for command in [&record.uninstall_string, &record.quiet_uninstall_string]
        .into_iter()
        .flatten()
    {
        for token in crate::parsers::lnk::split_arguments(command).unwrap_or_default() {
            for identity in crate::uri::identities(&token) {
                if let crate::uri::UriIdentity::External(id) = identity {
                    ids.push(id);
                }
            }
        }
    }
    ids.sort();
    ids.dedup();
    let path = record
        .install_location
        .as_deref()
        .unwrap_or_default()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let library = [
        "/steamapps/common/",
        "/gog games/",
        "/epic games/",
        "/origin games/",
        "/ea games/",
        "/ubisoft game launcher/games/",
    ]
    .iter()
    .any(|part| path.contains(part));
    let owned = !ids.is_empty()
        || library
        || name.starts_with("battle.net")
        || name.starts_with("battlenet")
        || name.starts_with("origin ")
        || name.starts_with("ea app ");
    (owned, ids)
}
