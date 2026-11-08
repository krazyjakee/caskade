//! What a game's releases carry between devices (#273): game scripts,
//! launch actions and their choices, and hand-made achievement links.
//!
//! A release (an entry) has ids of its own on each device, so these are
//! keyed by the game and a *release key* that names the same release
//! everywhere: one of its store ids, its RomM ROM (RomM servers carry their
//! ids between devices), or `only` for a game's one release. A release with
//! none keeps its actions and links on its device.
//!
//! What names a place on one device stays there (policy's
//! `Receive::BindLaunchLocally`): an action's program, folders, arguments
//! and environment, a tracking folder or process names, a script's program.
//! So only actions that can be bound here travel: a Flatpak's app id, and an
//! emulator action, which takes this release's own emulator launch on the
//! same platform. Only shell scripts travel, and they arrive turned off: a
//! script whose command changed on another device is turned off here too.
//!
//! Add-ons travel as what each device has installed, never installing or
//! turning on anything by themselves: a plugin's permissions are granted on
//! each device. A preferred emulator is a platform's emulator definition,
//! applied only where this device has that emulator.

use std::collections::{BTreeSet, HashMap};

use serde_json::{Value, json};
use sqlx::SqliteConnection;

use super::{FieldKey, SESSIONS_SOURCE, decode};
use crate::Result;
use crate::achievements::{AchievementLink, LinkMethod};
use crate::emulation::{PlatformSettings, ProfileRef};
use crate::model::{
    ActionKind, ActionTracking, EntryActionSettings, ExternalId, GameAction, InstallLocation,
    LaunchSpec, validation as v,
};
use crate::scripts::{LifecycleScript, ScriptCommand, ScriptScope};
use crate::sync::change::{EXISTS, Entity, join_key, key_parts};
use crate::tracking::TrackingMode;

/// The key of a game's only release, when it has no other.
const ONLY: &str = "only";

/// Whether entries from `source` are releases on this device, rather than
/// what another device, an account or a host has.
fn local_source(source: &str) -> bool {
    source != SESSIONS_SOURCE
        && !source.starts_with("store:")
        && !source.starts_with("stream:")
        && !source.starts_with("peer:")
}

/// Each release here (of `game`, or every game): its entry, game and key.
pub(super) async fn releases(
    conn: &mut SqliteConnection,
    game: Option<&str>,
) -> Result<Vec<(String, String, Option<String>)>> {
    let rows: Vec<(String, String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id,game_id,source_id,external_id,record_json->'$.ids' FROM game_entries \
         WHERE ?1 IS NULL OR game_id=?1",
    )
    .bind(game)
    .fetch_all(&mut *conn)
    .await?;
    let mut found: Vec<(String, String, Option<String>)> = Vec::new();
    let mut per_game: HashMap<String, usize> = HashMap::new();
    for (entry, game, source, external, ids) in rows {
        if !local_source(&source) {
            continue;
        }
        let stores: BTreeSet<String> = ids
            .and_then(|text| serde_json::from_str::<Vec<ExternalId>>(&text).ok())
            .unwrap_or_default()
            .iter()
            .filter(|id| !matches!(id, ExternalId::SteamShortcut { .. }))
            .filter_map(|id| super::store_accounts::key(id).ok().flatten())
            .collect();
        let key = match stores.into_iter().next() {
            Some(store) => Some(format!("store:{store}")),
            None if source.starts_with(crate::romm::SOURCE_PREFIX) => {
                Some(format!("{source}:{external}"))
            }
            None => None,
        };
        *per_game.entry(game.clone()).or_default() += 1;
        found.push((entry, game, key.map(|key| key.replace('|', "/"))));
    }
    for (_, game, key) in &mut found {
        if key.is_none() && per_game.get(game.as_str()) == Some(&1) {
            *key = Some(ONLY.to_owned());
        }
    }
    Ok(found)
}

/// The release of `game` here that `key` names.
async fn release(conn: &mut SqliteConnection, game: &str, key: &str) -> Result<Option<String>> {
    Ok(releases(conn, Some(game))
        .await?
        .into_iter()
        .find(|(_, _, release)| release.as_deref() == Some(key))
        .map(|(entry, _, _)| entry))
}

fn mode_value(mode: TrackingMode) -> Value {
    serde_json::to_value(mode).unwrap_or(Value::Null)
}

fn mode_of(value: &Value) -> Option<TrackingMode> {
    serde_json::from_value(value.clone()).ok()
}

/// Game scripts, launch actions, action choices and hand-made achievement
/// links here, as the log sees them.
pub(super) async fn snapshot(
    conn: &mut SqliteConnection,
    values: &mut HashMap<FieldKey, Value>,
) -> Result<()> {
    let mut put = |entity: Entity, key: String, field: &str, value: Value| {
        values.insert((entity, key, field.to_owned()), value);
    };
    for (id, game, position, record) in sqlx::query_as::<_, (String, String, i64, String)>(
        "SELECT id,game_id,position,record_json FROM lifecycle_scripts WHERE scope='game'",
    )
    .fetch_all(&mut *conn)
    .await?
    {
        let Ok(script) = decode::<LifecycleScript>(record) else {
            continue;
        };
        let ScriptCommand::Shell {
            shell,
            script: text,
        } = &script.command
        else {
            continue;
        };
        put(
            Entity::GameScript,
            join_key(&[&game, &id]),
            EXISTS,
            json!({
                "name": script.name,
                "phase": script.phase,
                "shell": shell,
                "script": text,
                "timeoutSecs": script.timeout_secs,
                "position": position,
            }),
        );
    }

    let keys: HashMap<String, (String, String)> = releases(conn, None)
        .await?
        .into_iter()
        .filter_map(|(entry, game, key)| Some((entry, (game, key?))))
        .collect();
    for (id, entry, position, record) in sqlx::query_as::<_, (String, String, i64, String)>(
        "SELECT id,entry_id,position,record_json FROM game_actions",
    )
    .fetch_all(&mut *conn)
    .await?
    {
        let (Some((game, release)), Ok(action)) = (keys.get(&entry), decode::<GameAction>(record))
        else {
            continue;
        };
        let target = match &action.target {
            LaunchSpec::Flatpak { app_id, .. } => json!({ "type": "flatpak", "appId": app_id }),
            LaunchSpec::Emulator { platform_id, .. } => {
                json!({ "type": "emulator", "platformId": platform_id })
            }
            // Programs, addresses and pages on this device stay here.
            _ => continue,
        };
        put(
            Entity::Action,
            join_key(&[game, release, &id]),
            EXISTS,
            json!({
                "name": action.name,
                "kind": action.kind,
                "target": target,
                "tracking": mode_value(action.tracking.mode),
                "position": position,
            }),
        );
    }
    for (entry, record) in sqlx::query_as::<_, (String, String)>(
        "SELECT entry_id,record_json FROM entry_action_settings",
    )
    .fetch_all(&mut *conn)
    .await?
    {
        let (Some((game, release)), Ok(settings)) =
            (keys.get(&entry), decode::<EntryActionSettings>(record))
        else {
            continue;
        };
        let key = join_key(&[game, release]);
        if let Some(action) = settings.default_action {
            put(
                Entity::ActionSettings,
                key.clone(),
                "defaultAction",
                json!(action),
            );
        }
        if settings.ask {
            put(Entity::ActionSettings, key.clone(), "ask", json!(true));
        }
        if let Some(name) = settings.built_in.name {
            put(
                Entity::ActionSettings,
                key.clone(),
                "builtInName",
                json!(name),
            );
        }
        if let Some(tracking) = settings.built_in.tracking {
            put(
                Entity::ActionSettings,
                key,
                "builtInTracking",
                mode_value(tracking.mode),
            );
        }
    }
    for (entry, provider, record) in sqlx::query_as::<_, (String, String, String)>(
        "SELECT entry_id,provider,record_json FROM achievement_links \
         WHERE record_json->>'$.method.type'='manual'",
    )
    .fetch_all(&mut *conn)
    .await?
    {
        let (Some((game, release)), Ok(link)) =
            (keys.get(&entry), decode::<AchievementLink>(record))
        else {
            continue;
        };
        put(
            Entity::AchievementLink,
            join_key(&[game, release, &provider]),
            EXISTS,
            json!({ "product": link.product_id, "linkedAt": link.linked_at_ms }),
        );
    }
    Ok(())
}

/// Applies a change to one of these entities at `target` (this device's
/// game, then the release key and the rest). `false`: it waits, for its
/// game or release, or for something it needs here.
pub(super) async fn write(
    conn: &mut SqliteConnection,
    entity: Entity,
    target: &str,
    field: &str,
    value: Option<&Value>,
) -> Result<bool> {
    let parts = key_parts(target);
    match entity {
        Entity::GameScript => {
            let [game, id] = parts[..] else {
                return Ok(true);
            };
            game_script(conn, game, id, value).await
        }
        Entity::Action => {
            let [game, release_key, id] = parts[..] else {
                return Ok(true);
            };
            let Some(entry) = release(conn, game, release_key).await? else {
                return Ok(value.is_none());
            };
            action(conn, &entry, id, value).await
        }
        Entity::ActionSettings => {
            let [game, release_key] = parts[..] else {
                return Ok(true);
            };
            let Some(entry) = release(conn, game, release_key).await? else {
                return Ok(value.is_none());
            };
            action_settings(conn, &entry, field, value).await
        }
        Entity::AchievementLink => {
            let [game, release_key, provider] = parts[..] else {
                return Ok(true);
            };
            let Some(entry) = release(conn, game, release_key).await? else {
                return Ok(value.is_none());
            };
            achievement_link(conn, &entry, provider, value).await
        }
        Entity::PreferredEmulator => preferred_emulator(conn, target, value).await,
        // What another device has installed: kept in the log, nothing to
        // write here.
        _ => Ok(true),
    }
}

/// Add-ons installed here (plugins, and what came from the catalogue) and
/// each platform's preferred emulator definition, as the log sees them.
pub(super) async fn snapshot_device(
    conn: &mut SqliteConnection,
    values: &mut HashMap<FieldKey, Value>,
) -> Result<()> {
    let mut addons: HashMap<String, Value> = HashMap::new();
    for (id, enabled) in sqlx::query_as::<_, (String, bool)>("SELECT id,enabled FROM plugins")
        .fetch_all(&mut *conn)
        .await?
    {
        addons.insert(id.clone(), json!({ "kind": "plugin" }));
        values.insert((Entity::Addon, id, "enabled".to_owned()), json!(enabled));
    }
    let records: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM settings WHERE key='addons.records'")
            .fetch_optional(&mut *conn)
            .await?;
    if let Some(records) = records.and_then(|text| serde_json::from_str::<Value>(&text).ok()) {
        for (id, record) in records
            .get("addons")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            let entry = addons.entry(id.clone()).or_insert_with(|| json!({}));
            if let Some(version) = record.get("version").filter(|version| !version.is_null()) {
                entry["version"] = version.clone();
            }
        }
    }
    for (id, value) in addons {
        values.insert((Entity::Addon, id, EXISTS.to_owned()), value);
    }
    for (platform, definition) in sqlx::query_as::<_, (String, String)>(
        "SELECT p.platform_id,e.definition_id FROM platform_settings p \
         JOIN emulators e ON e.id=p.emulator_id WHERE e.definition_id IS NOT NULL",
    )
    .fetch_all(&mut *conn)
    .await?
    {
        values.insert(
            (Entity::PreferredEmulator, platform, "definition".to_owned()),
            json!(definition),
        );
    }
    Ok(())
}

/// Makes `platform`'s default this device's emulator of the preferred
/// definition, with its first profile. It waits while this device has no
/// such emulator; another device clearing its default changes nothing here.
async fn preferred_emulator(
    conn: &mut SqliteConnection,
    platform: &str,
    value: Option<&Value>,
) -> Result<bool> {
    let Some(definition) = value.and_then(Value::as_str) else {
        return Ok(true);
    };
    let current: Option<String> = sqlx::query_scalar(
        "SELECT e.definition_id FROM platform_settings p JOIN emulators e ON e.id=p.emulator_id \
         WHERE p.platform_id=?",
    )
    .bind(platform)
    .fetch_optional(&mut *conn)
    .await?
    .flatten();
    if current.as_deref() == Some(definition) {
        return Ok(true);
    }
    let Some((emulator, profile)) = sqlx::query_as::<_, (String, String)>(
        "SELECT e.id,p.id FROM emulators e JOIN emulator_profiles p ON p.emulator_id=e.id \
         WHERE e.definition_id=? ORDER BY e.id,p.position,p.id LIMIT 1",
    )
    .bind(definition)
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(false);
    };
    let mut settings = sqlx::query_scalar::<_, String>(
        "SELECT record_json FROM platform_settings WHERE platform_id=?",
    )
    .bind(platform)
    .fetch_optional(&mut *conn)
    .await?
    .map(decode::<PlatformSettings>)
    .transpose()?
    .unwrap_or(PlatformSettings {
        platform: platform.to_owned(),
        default: None,
        bios_dir: None,
    });
    settings.default = Some(ProfileRef {
        emulator_id: emulator.clone(),
        profile_id: profile.clone(),
    });
    sqlx::query(
        "INSERT INTO platform_settings(platform_id,emulator_id,profile_id,record_json) VALUES(?,?,?,?) \
         ON CONFLICT(platform_id) DO UPDATE SET emulator_id=excluded.emulator_id,\
         profile_id=excluded.profile_id,record_json=excluded.record_json",
    )
    .bind(platform)
    .bind(&emulator)
    .bind(&profile)
    .bind(v::json(&settings)?)
    .execute(&mut *conn)
    .await?;
    Ok(true)
}

async fn game_script(
    conn: &mut SqliteConnection,
    game: &str,
    id: &str,
    value: Option<&Value>,
) -> Result<bool> {
    let Some(value) = value else {
        sqlx::query("DELETE FROM lifecycle_scripts WHERE id=? AND scope='game'")
            .bind(id)
            .execute(&mut *conn)
            .await?;
        return Ok(true);
    };
    let found: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM games WHERE id=?)")
        .bind(game)
        .fetch_one(&mut *conn)
        .await?;
    if !found {
        return Ok(false);
    }
    let (Some(shell), Some(text), Some(phase)) = (
        value
            .get("shell")
            .and_then(|shell| serde_json::from_value(shell.clone()).ok()),
        value.get("script").and_then(Value::as_str),
        value
            .get("phase")
            .and_then(|phase| serde_json::from_value(phase.clone()).ok()),
    ) else {
        return Ok(true);
    };
    let command = ScriptCommand::Shell {
        shell,
        script: text.to_owned(),
    };
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT record_json FROM lifecycle_scripts WHERE id=? AND scope='game'",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?
    .map(decode::<LifecycleScript>)
    .transpose()?;
    let mut script = existing.clone().unwrap_or_else(|| {
        let mut script = LifecycleScript::new(
            ScriptScope::Game {
                game_id: game.to_owned(),
            },
            phase,
            command.clone(),
        );
        script.id = id.to_owned();
        script.enabled = false;
        script
    });
    // A command another device changed never runs here unasked.
    if script.command != command {
        script.enabled = false;
    }
    script.command = command;
    script.phase = phase;
    script.name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if let Some(timeout) = value.get("timeoutSecs").and_then(Value::as_u64) {
        script.timeout_secs = u32::try_from(timeout).unwrap_or(u32::MAX);
    }
    // A shell this system hasn't, or a value it refuses: it stays on the
    // other device.
    let Ok(script) = script.normalized() else {
        return Ok(false);
    };
    let position = value
        .get("position")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    if existing.is_some() {
        sqlx::query("UPDATE lifecycle_scripts SET position=?,record_json=? WHERE id=?")
            .bind(position)
            .bind(v::json(&script)?)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    } else {
        sqlx::query(
            "INSERT INTO lifecycle_scripts(id,scope,profile_id,game_id,position,record_json) \
             VALUES(?,'game',NULL,?,?,?)",
        )
        .bind(id)
        .bind(game)
        .bind(position)
        .bind(v::json(&script)?)
        .execute(&mut *conn)
        .await?;
    }
    Ok(true)
}

async fn action(
    conn: &mut SqliteConnection,
    entry: &str,
    id: &str,
    value: Option<&Value>,
) -> Result<bool> {
    let Some(value) = value else {
        sqlx::query("DELETE FROM game_actions WHERE id=?")
            .bind(id)
            .execute(&mut *conn)
            .await?;
        sqlx::query(
            "UPDATE entry_action_settings SET record_json=json_remove(record_json,'$.defaultAction') \
             WHERE record_json->>'$.defaultAction'=?",
        )
        .bind(id)
        .execute(&mut *conn)
        .await?;
        return Ok(true);
    };
    let release = super::games::load_entry(conn, entry).await?;
    let existing = sqlx::query_as::<_, (String, String)>(
        "SELECT entry_id,record_json FROM game_actions WHERE id=?",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let existing = match existing {
        Some((owner, _)) if owner != entry => return Ok(true),
        Some((_, record)) => Some(decode::<GameAction>(record)?),
        None => None,
    };
    let target = value.get("target").cloned().unwrap_or(Value::Null);
    let bound = match target.get("type").and_then(Value::as_str) {
        Some("flatpak") => {
            target
                .get("appId")
                .and_then(Value::as_str)
                .map(|app_id| match &existing {
                    Some(GameAction {
                        target: target @ LaunchSpec::Flatpak { app_id: mine, .. },
                        ..
                    }) if mine == app_id => target.clone(),
                    _ => LaunchSpec::Flatpak {
                        app_id: app_id.to_owned(),
                        args: Vec::new(),
                    },
                })
        }
        // This release's own emulator launch, on the same platform.
        Some("emulator") => match (&release.launch, target.get("platformId")) {
            (Some(launch @ LaunchSpec::Emulator { platform_id, .. }), Some(platform))
                if platform.as_str() == Some(platform_id.as_str()) =>
            {
                match &existing {
                    Some(GameAction {
                        target:
                            mine @ LaunchSpec::Emulator {
                                platform_id: theirs,
                                ..
                            },
                        ..
                    }) if theirs == platform_id => Some(mine.clone()),
                    _ => Some(launch.clone()),
                }
            }
            _ => None,
        },
        _ => None,
    };
    // Nothing to bind it to here (yet): it waits.
    let Some(bound) = bound else {
        return Ok(false);
    };
    let mut tracking = existing
        .as_ref()
        .map(|action| action.tracking.clone())
        .unwrap_or_default();
    if let Some(mode) = value.get("tracking").and_then(mode_of) {
        tracking = ActionTracking { mode, ..tracking };
    }
    let action = GameAction {
        id: id.to_owned(),
        entry_id: entry.to_owned(),
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        kind: value
            .get("kind")
            .and_then(|kind| serde_json::from_value(kind.clone()).ok())
            .unwrap_or(ActionKind::Play),
        target: bound,
        tracking,
        proton: None,
    };
    let managed = matches!(release.location, Some(InstallLocation::Managed { .. }));
    let Ok(action) = action.normalized(managed) else {
        return Ok(false);
    };
    let position = value
        .get("position")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    sqlx::query(
        "INSERT INTO game_actions(id,entry_id,position,record_json) VALUES(?,?,?,?) \
         ON CONFLICT(id) DO UPDATE SET position=excluded.position,record_json=excluded.record_json",
    )
    .bind(id)
    .bind(entry)
    .bind(position)
    .bind(v::json(&action)?)
    .execute(&mut *conn)
    .await?;
    Ok(true)
}

async fn action_settings(
    conn: &mut SqliteConnection,
    entry: &str,
    field: &str,
    value: Option<&Value>,
) -> Result<bool> {
    let mut settings = sqlx::query_scalar::<_, String>(
        "SELECT record_json FROM entry_action_settings WHERE entry_id=?",
    )
    .bind(entry)
    .fetch_optional(&mut *conn)
    .await?
    .map(decode::<EntryActionSettings>)
    .transpose()?
    .unwrap_or_default();
    match field {
        "defaultAction" => {
            settings.default_action = value.and_then(Value::as_str).map(str::to_owned);
        }
        "ask" => settings.ask = value.and_then(Value::as_bool).unwrap_or(false),
        "builtInName" => {
            settings.built_in.name = value
                .and_then(Value::as_str)
                .map(|name| name.trim().to_owned())
                .filter(|name| {
                    !name.is_empty() && v::text(name, "action name", false, 256).is_ok()
                });
        }
        "builtInTracking" => {
            settings.built_in.tracking = match value.and_then(mode_of) {
                // Its folder and process names, if any, are this device's.
                Some(mode) => Some(ActionTracking {
                    mode,
                    ..settings.built_in.tracking.unwrap_or_default()
                }),
                None => None,
            };
        }
        _ => return Ok(true),
    }
    sqlx::query(
        "INSERT INTO entry_action_settings(entry_id,record_json) VALUES(?,?) \
         ON CONFLICT(entry_id) DO UPDATE SET record_json=excluded.record_json",
    )
    .bind(entry)
    .bind(v::json(&settings)?)
    .execute(&mut *conn)
    .await?;
    Ok(true)
}

async fn achievement_link(
    conn: &mut SqliteConnection,
    entry: &str,
    provider: &str,
    value: Option<&Value>,
) -> Result<bool> {
    let current = sqlx::query_scalar::<_, String>(
        "SELECT record_json FROM achievement_links WHERE entry_id=? AND provider=?",
    )
    .bind(entry)
    .bind(provider)
    .fetch_optional(&mut *conn)
    .await?
    .map(decode::<AchievementLink>)
    .transpose()?;
    // A ROM hash this device verified stays: it's this copy's own answer.
    if matches!(
        current,
        Some(AchievementLink {
            method: LinkMethod::Hash { .. },
            ..
        })
    ) {
        return Ok(true);
    }
    let Some(value) = value else {
        sqlx::query(
            "DELETE FROM achievement_links WHERE entry_id=? AND provider=? \
             AND record_json->>'$.method.type'='manual'",
        )
        .bind(entry)
        .bind(provider)
        .execute(&mut *conn)
        .await?;
        return Ok(true);
    };
    let Some(product) = value.get("product").and_then(Value::as_str) else {
        return Ok(true);
    };
    let link = AchievementLink {
        entry_id: entry.to_owned(),
        provider: provider.to_owned(),
        product_id: product.to_owned(),
        method: LinkMethod::Manual,
        linked_at_ms: value.get("linkedAt").and_then(Value::as_i64).unwrap_or(0),
    };
    sqlx::query(
        "INSERT INTO achievement_links(entry_id,provider,record_json) VALUES(?,?,?) \
         ON CONFLICT(entry_id,provider) DO UPDATE SET record_json=excluded.record_json",
    )
    .bind(entry)
    .bind(provider)
    .bind(serde_json::to_string(&link)?)
    .execute(&mut *conn)
    .await?;
    Ok(true)
}
