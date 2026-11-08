//! Launcher protocol identity only. Parsing never opens a URI or runs a shell.
//! Heroic's current query form is
//! also accepted (format reference, no launcher implementation is copied):
//! https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/shortcuts/shortcuts/shortcuts.ts
//! goggalaxy openGameView is documented by PlayniteExtensions/GogGameController.cs.
use crate::{DetectedGame, Evidence, ExternalId, LaunchTarget, SourceId};
use percent_encoding::percent_decode_str;
use std::collections::{BTreeMap, BTreeSet};
use url::Url;

/// An identity-only shortcut contributes to its owner, never a standalone game.
pub const PROXY_RULE: &str = "identity.proxy";
pub fn mark_proxy(entry: &mut DetectedGame) {
    if !entry
        .evidence
        .iter()
        .any(|e| matches!(e, Evidence::Rule { rule, .. } if rule == PROXY_RULE))
    {
        entry.evidence.push(Evidence::Rule {
            rule: PROXY_RULE.into(),
            score: 0,
        });
    }
}

/// An identity-only entry: a launcher link (`mark_proxy`), a Steam shortcut
/// proxy or a launcher-owned uninstall alias. It names its owner's
/// installation rather than a separate one, so it is never a launch entry.
pub fn is_proxy(entry: &DetectedGame) -> bool {
    entry.evidence.iter().any(|e| {
        matches!(e, Evidence::Rule { rule, .. }
            if rule == PROXY_RULE || rule == "steam.shortcut.proxy" || rule == "alias.launcher_owned")
    })
}

/// Local launcher ids (itch caves, Lutris rows, Heroic sideloads) deliberately
/// occupy a different namespace from store ids.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum UriIdentity {
    External(ExternalId),
    Launcher { source: SourceId, key: String },
}

fn local(source: &'static str, key: impl Into<String>) -> UriIdentity {
    UriIdentity::Launcher {
        source: SourceId::new(source),
        key: key.into(),
    }
}
fn external(id: ExternalId) -> UriIdentity {
    UriIdentity::External(id)
}
fn number(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|v| v.is_ascii_digit()) {
        return None;
    }
    value.parse().ok().filter(|n| *n > 0)
}
fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'%'
            && !bytes
                .get(index + 1..index + 3)
                .is_some_and(|v| v.iter().all(u8::is_ascii_hexdigit))
        {
            return None;
        }
    }
    let text = percent_decode_str(text).decode_utf8().ok()?.into_owned();
    (!text.chars().any(char::is_control)).then_some(text)
}
/// A recognised launch/view URI's identities. Unknown, malformed, ambiguous
/// or oversized inputs yield no identities; titles are never inferred.
pub fn identities(input: &str) -> Vec<UriIdentity> {
    parse(input).unwrap_or_default()
}
fn parse(input: &str) -> Option<Vec<UriIdentity>> {
    if input.len() > 16_384 || input.chars().any(char::is_control) {
        return None;
    }
    if input
        .get(..17)
        .is_some_and(|v| v.eq_ignore_ascii_case(r"shell:AppsFolder\"))
    {
        let tail = decode(&input[17..])?;
        let (pfn, app) = tail.split_once('!')?;
        if pfn.is_empty() || app.is_empty() || tail.contains(['/', '\\']) {
            return None;
        }
        return Some(vec![external(ExternalId::Xbox {
            pfn: pfn.to_ascii_lowercase(),
        })]);
    }
    decode(input)?;
    let uri = Url::parse(input).ok()?;
    if !uri.username().is_empty() || uri.password().is_some() || uri.port().is_some() {
        return None;
    }
    let host = uri.host_str().unwrap_or_default();
    let parts: Vec<String> = uri
        .path()
        .split('/')
        .filter(|v| !v.is_empty())
        .map(decode)
        .collect::<Option<_>>()?;
    let mut query = BTreeMap::new();
    for (key, value) in uri.query_pairs() {
        if value.chars().any(char::is_control)
            || query
                .insert(key.to_ascii_lowercase(), value.into_owned())
                .is_some()
        {
            return None;
        }
    }
    let first = || parts.first().map(String::as_str);
    let id = match uri.scheme() {
        "steam"
            if host.eq_ignore_ascii_case("rungameid")
                || host.eq_ignore_ascii_case("run")
                || host.eq_ignore_ascii_case("uninstall") =>
        {
            let id = number(first()?)?;
            if let Ok(app_id) = u32::try_from(id) {
                external(ExternalId::Steam { app_id })
            } else if id as u32 == 0x02000000 {
                external(ExternalId::SteamShortcut {
                    app_id: (id >> 32) as u32,
                })
            } else {
                return None;
            }
        }
        "com.epicgames.launcher" if host.eq_ignore_ascii_case("apps") => {
            let app = first()?;
            let triple: Vec<_> = app.split(':').collect();
            if triple.len() == 3 && triple.iter().all(|v| !v.is_empty()) {
                return Some(vec![
                    external(ExternalId::Epic {
                        namespace: triple[0].into(),
                        catalog_item: triple[1].into(),
                        app_name: triple[2].into(),
                    }),
                    local("epic.app", triple[2]),
                ]);
            }
            if triple.len() != 1 || app.is_empty() {
                return None;
            }
            local("epic.app", app)
        }
        "goggalaxy"
            if ["opengameview", "launchgame", "launch"]
                .iter()
                .any(|v| host.eq_ignore_ascii_case(v)) =>
        {
            external(ExternalId::Gog {
                product_id: number(first()?)?,
            })
        }
        "uplay" if host.eq_ignore_ascii_case("launch") => external(ExternalId::Ubisoft {
            id: number(first()?)?,
        }),
        "battlenet" if !host.is_empty() => {
            let code = if host.eq_ignore_ascii_case("launch") {
                first()?
            } else {
                host
            };
            external(ExternalId::BattleNet {
                code: code.to_ascii_uppercase(),
            })
        }
        "amazon-games" if host.eq_ignore_ascii_case("play") => external(ExternalId::Amazon {
            id: first()?.into(),
        }),
        "link2ea" if host.eq_ignore_ascii_case("launchgame") => external(ExternalId::Ea {
            offer: first()?.into(),
        }),
        "origin"
            if host.eq_ignore_ascii_case("launchgame")
                || host.eq_ignore_ascii_case("launchgamejump") =>
        {
            let offer = first()?;
            if offer.is_empty() || offer.contains(',') {
                return None;
            }
            external(ExternalId::Ea {
                offer: offer.into(),
            })
        }
        "origin2" if host.eq_ignore_ascii_case("game") && first()? == "launch" => {
            let offer = query.get("offerids")?;
            if offer.is_empty() || offer.contains(',') {
                return None;
            }
            external(ExternalId::Ea {
                offer: offer.clone(),
            })
        }
        "lutris" => {
            let action = if host.is_empty() { first()? } else { host };
            let value = parts.get(usize::from(host.is_empty()))?;
            match action {
                "rungameid" => local("lutris.row", number(value)?.to_string()),
                "rungame" => external(ExternalId::Lutris {
                    slug: value.clone(),
                }),
                _ => return None,
            }
        }
        "itch" | "itchio"
            if host.eq_ignore_ascii_case("caves")
                && parts.get(1)?.eq_ignore_ascii_case("launch") =>
        {
            local("itch.cave", number(first()?)?.to_string())
        }
        "heroic" if host.eq_ignore_ascii_case("launch") => {
            let (runner, app) = if parts.is_empty() {
                (
                    query.get("runner")?.as_str(),
                    query.get("appname")?.as_str(),
                )
            } else {
                (first()?, parts.get(1)?.as_str())
            };
            if app.is_empty() {
                return None;
            }
            match runner {
                "gog" => external(ExternalId::Gog {
                    product_id: number(app)?,
                }),
                "legendary" | "epic" => local("epic.app", app),
                "nile" | "amazon" => external(ExternalId::Amazon { id: app.into() }),
                "sideload" => local("heroic.sideload", app),
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(vec![id])
}

/// Identity aliases for typed ids, so a partial Epic launch URI can match a
/// manifest carrying the full namespace/catalog/app triple.
pub fn external_identities(id: &ExternalId) -> Vec<UriIdentity> {
    match id {
        ExternalId::Epic { app_name, .. } => {
            vec![external(id.clone()), local("epic.app", app_name)]
        }
        ExternalId::BattleNet { code } => vec![external(ExternalId::BattleNet {
            code: code.to_ascii_uppercase(),
        })],
        ExternalId::Xbox { pfn } => vec![external(ExternalId::Xbox {
            pfn: pfn.to_ascii_lowercase(),
        })],
        _ => vec![external(id.clone())],
    }
}

/// Exact argv elements only. A shell command string is never evaluated or
/// searched for a URI substring. Wrapper programs can pass a URI as an arg.
pub fn target_identities(target: &LaunchTarget) -> BTreeSet<UriIdentity> {
    let values: Vec<&str> = match target {
        LaunchTarget::Uri { uri } => vec![uri],
        LaunchTarget::Exec { program, args, .. } => program
            .to_str()
            .into_iter()
            .chain(args.iter().map(String::as_str))
            .collect(),
        LaunchTarget::Flatpak { args, .. } => args.iter().map(String::as_str).collect(),
    };
    let mut result: BTreeSet<_> = values.into_iter().flat_map(identities).collect();
    if let LaunchTarget::Exec { program, args, .. } = target
        && program
            .to_str()
            .and_then(|v| v.rsplit(['/', '\\']).next())
            .is_some_and(|v| {
                v.eq_ignore_ascii_case("legendary") || v.eq_ignore_ascii_case("legendary.exe")
            })
        && args.first().is_some_and(|v| v == "launch")
        && let Some(app) = args
            .get(1)
            .filter(|v| !v.is_empty() && !v.starts_with('-') && !v.chars().any(char::is_control))
    {
        result.insert(local("epic.app", app));
    }
    // Galaxy's product identity lives in argv on both Windows and macOS.
    // Recognising the command also prevents its shared client executable from
    // becoming a path identity that joins every installed GOG game.
    if let LaunchTarget::Exec { args, .. } = target
        && args
            .iter()
            .any(|arg| arg.eq_ignore_ascii_case("/command=runGame"))
    {
        let ids: BTreeSet<u64> = args
            .iter()
            .filter_map(|arg| arg.split_once('='))
            .filter(|(key, _)| key.eq_ignore_ascii_case("/gameId"))
            .filter_map(|(_, value)| value.parse::<u64>().ok())
            .filter(|id| *id > 0)
            .collect();
        if ids.len() == 1
            && let Some(id) = ids.first()
        {
            result.insert(UriIdentity::External(ExternalId::Gog { product_id: *id }));
        }
    }
    match target {
        LaunchTarget::Flatpak { app_id, .. } => scope_local(result, Some(app_id)),
        _ => result,
    }
}

/// Store identities are global; database-local ids belong to one launcher
/// installation. An enclosing Flatpak target selects a different database.
pub(crate) fn scope_local(
    ids: BTreeSet<UriIdentity>,
    flatpak: Option<&str>,
) -> BTreeSet<UriIdentity> {
    ids.into_iter()
        .map(|id| {
            if let (UriIdentity::Launcher { source, key }, Some(app)) = (&id, flatpak)
                && matches!(
                    source.as_str(),
                    "lutris.row" | "itch.cave" | "heroic.sideload"
                )
            {
                return UriIdentity::Launcher {
                    source: SourceId::from(format!("{}.flatpak.{app}", source.as_str())),
                    key: key.clone(),
                };
            }
            id
        })
        .collect()
}
