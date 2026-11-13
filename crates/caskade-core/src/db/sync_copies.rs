//! Other devices' installed copies of a game (#232). Each device announces
//! its own as `copy` changes, keyed by the device, the game and the entry
//! (the entry's id is an opaque UUID): the release key, size, the build's
//! OS, where it lives (a Caskade-managed folder, a folder of its own, or a
//! launcher's) and whether its folder can be copied. Never a path.
//!
//! Another device's copy is an entry here from source `peer:<device id>`,
//! uninstalled and not launchable, like a store account's owned entry; its
//! details stay in the change log's field, read with the entry. Uninstalled
//! there, its tombstone removes the entry here.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;

use super::games::save_entry;
use super::{FieldKey, LibraryStore, decode};
use crate::Result;
use crate::model::{GameEntry, GameRuntime, InstallLocation, InstallState, LaunchSpec};
use crate::sync::change::{EXISTS, Entity, key_parts};

/// The source of another device's copies.
pub(super) const PEER_PREFIX: &str = "peer:";

/// Where a copy lives on its device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CopyLocation {
    /// A folder Caskade installed and manages.
    Managed,
    /// A folder of its own, found or added there.
    External,
    /// A launcher's (Steam, …): it starts through that launcher.
    Launcher,
}

/// What a device says about one of its installed copies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnouncedCopy {
    #[serde(default)]
    pub release: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    /// `windows`, `linux` or `mac`.
    pub os: String,
    pub location: CopyLocation,
    pub copyable: bool,
}

/// Another device's copy of a game, as this device knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCopyRecord {
    pub device_id: String,
    pub entry_id: String,
    pub copy: AnnouncedCopy,
}

/// This system as a copy's `os`.
pub fn this_os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    }
}

/// What a copy is, from its entry; `None` when it isn't installed.
fn announce(entry: &GameEntry, release: Option<String>) -> Option<AnnouncedCopy> {
    if !matches!(
        entry.state,
        InstallState::Installed | InstallState::Updating
    ) {
        return None;
    }
    let location = match (&entry.location, &entry.launch) {
        (None, _) => return None,
        (Some(InstallLocation::Managed { .. }), _) => CopyLocation::Managed,
        (
            Some(InstallLocation::External { .. }),
            Some(LaunchSpec::Executable { .. } | LaunchSpec::Emulator { .. }),
        ) => CopyLocation::External,
        (Some(InstallLocation::External { .. }), _) => CopyLocation::Launcher,
    };
    let os = match entry.runtime {
        GameRuntime::Windows { .. } => "windows",
        GameRuntime::Native => this_os(),
    };
    Some(AnnouncedCopy {
        release,
        size: entry.size,
        os: os.to_owned(),
        location,
        copyable: location != CopyLocation::Launcher,
    })
}

/// This device's installed copies, keyed `game|entry` (the engine puts the
/// device in front, as for add-ons).
pub(super) async fn snapshot(
    conn: &mut SqliteConnection,
    values: &mut HashMap<FieldKey, Value>,
) -> Result<()> {
    let releases: HashMap<String, Option<String>> = super::sync_carried::releases(conn, None)
        .await?
        .into_iter()
        .map(|(entry, _, key)| (entry, key))
        .collect();
    for record in sqlx::query_scalar::<_, String>(
        "SELECT record_json FROM game_entries WHERE location_kind<>'none' \
         AND state_kind IN ('installed','updating')",
    )
    .fetch_all(&mut *conn)
    .await?
    {
        let Ok(entry) = decode::<GameEntry>(record) else {
            continue;
        };
        // Only this device's releases: not other devices', accounts' or hosts'.
        let Some(release) = releases.get(&entry.id) else {
            continue;
        };
        if let Some(copy) = announce(&entry, release.clone()) {
            values.insert(
                (
                    Entity::Copy,
                    format!("{}|{}", entry.game_id, entry.id),
                    EXISTS.to_owned(),
                ),
                serde_json::to_value(copy)?,
            );
        }
    }
    Ok(())
}

/// Applies another device's copy at `target` (`device|game|entry`, the game
/// in this device's ids): its entry here, or its removal. `false` while the
/// game hasn't arrived.
pub(super) async fn write(
    conn: &mut SqliteConnection,
    target: &str,
    value: Option<&Value>,
) -> Result<bool> {
    let [device, game, entry] = key_parts(target)[..] else {
        return Ok(true);
    };
    let source = format!("{PEER_PREFIX}{device}");
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT record_json FROM game_entries WHERE source_id=? AND external_id=?",
    )
    .bind(&source)
    .bind(entry)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(value) = value else {
        sqlx::query("DELETE FROM game_entries WHERE source_id=? AND external_id=?")
            .bind(&source)
            .bind(entry)
            .execute(&mut *conn)
            .await?;
        return Ok(true);
    };
    let Ok(copy) = serde_json::from_value::<AnnouncedCopy>(value.clone()) else {
        return Ok(true);
    };
    let found: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM games WHERE id=?)")
        .bind(game)
        .fetch_one(&mut *conn)
        .await?;
    if !found {
        return Ok(false);
    }
    let mut peer = match existing {
        Some(record) => decode::<GameEntry>(record)?,
        None => GameEntry::new(game, source.into(), entry),
    };
    peer.game_id = game.to_owned();
    peer.state = InstallState::Partial;
    peer.location = None;
    peer.launch = None;
    peer.size = copy.size;
    let peer = peer.normalized()?;
    save_entry(conn, &peer).await?;
    Ok(true)
}

impl LibraryStore {
    /// Other devices' copies of `game` here.
    pub async fn sync_peer_copies(&self, game: &str) -> Result<Vec<PeerCopyRecord>> {
        let entries: Vec<(String, String)> = sqlx::query_as(
            "SELECT source_id,external_id FROM game_entries WHERE game_id=? AND source_id LIKE 'peer:%' \
             ORDER BY source_id,external_id",
        )
        .bind(game)
        .fetch_all(&self.pool)
        .await?;
        let mut copies = Vec::new();
        for (source, entry) in entries {
            let device = source.trim_start_matches(PEER_PREFIX).to_owned();
            let value: Option<String> = sqlx::query_scalar(
                "SELECT value_json FROM sync_fields WHERE entity=? AND field=? AND key LIKE ? \
                 ORDER BY wall DESC,counter DESC LIMIT 1",
            )
            .bind(Entity::Copy.as_str())
            .bind(EXISTS)
            .bind(format!("{device}|%|{entry}"))
            .fetch_optional(&self.pool)
            .await?
            .flatten();
            let Some(copy) = value
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .and_then(|value| serde_json::from_value::<AnnouncedCopy>(value).ok())
            else {
                continue;
            };
            copies.push(PeerCopyRecord {
                device_id: device,
                entry_id: entry,
                copy,
            });
        }
        Ok(copies)
    }

    /// Removes a forgotten device's copies.
    pub async fn sync_forget_copies(&self, device: &str) -> Result<u64> {
        Ok(sqlx::query("DELETE FROM game_entries WHERE source_id=?")
            .bind(format!("{PEER_PREFIX}{device}"))
            .execute(&self.pool)
            .await?
            .rows_affected())
    }
}

/// SQL: whether game `g` has another device's copy that can come here (for
/// Ready to Install): copyable, and a build this system runs.
///
/// The announced copies that qualify are read once per statement, as
/// `(device, entry)` pairs from their `device|game|entry` keys (the game
/// part may name a game merged away since), and each peer entry is looked
/// up among them. Matching each entry's key with `LIKE` instead compared
/// every peer entry with every announced copy for every game: with a few
/// thousand copies, Library's counts took seconds per query and held the
/// library's connections long enough that everything else timed out (#77).
pub fn copyable_here_sql() -> String {
    // A Windows build runs on Linux through Proton.
    let systems = match this_os() {
        "linux" => "'linux','windows'",
        "windows" => "'windows'",
        _ => "'mac'",
    };
    // The key after its device: `game|entry`.
    let rest = "substr(pf.key,instr(pf.key,'|')+1)";
    format!(
        "EXISTS(SELECT 1 FROM game_entries pe WHERE pe.game_id=g.id AND pe.source_id LIKE 'peer:%' \
         AND (substr(pe.source_id,6),pe.external_id) IN (\
         SELECT substr(pf.key,1,instr(pf.key,'|')-1),substr({rest},instr({rest},'|')+1) \
         FROM sync_fields pf WHERE pf.entity='copy' AND pf.field='' \
         AND pf.value_json->>'$.copyable' AND pf.value_json->>'$.os' IN ({systems})))"
    )
}
