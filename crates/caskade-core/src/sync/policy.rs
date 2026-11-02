//! Nothing is shared without an explicit table/key AND field declaration.
//! These are portable payloads, not SQL rows ready to insert on another
//! machine: the change exchange must map identities and honor `receive`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Library,
    Artwork,
    AchievementLinks,
    PlaySessions,
    Saves,
    Accounts,
    Appearance,
    RommServers,
    LaunchActions,
    GameScripts,
    PreferredEmulators,
    Addons,
}

impl Category {
    pub const ALL: [Self; 12] = [
        Self::Library,
        Self::Artwork,
        Self::AchievementLinks,
        Self::PlaySessions,
        Self::Saves,
        Self::Accounts,
        Self::Appearance,
        Self::RommServers,
        Self::LaunchActions,
        Self::GameScripts,
        Self::PreferredEmulators,
        Self::Addons,
    ];

    pub const fn disposition(self) -> Disposition {
        match self {
            Self::LaunchActions | Self::GameScripts | Self::PreferredEmulators | Self::Addons => {
                Disposition::Optional
            }
            Self::Library
            | Self::Artwork
            | Self::AchievementLinks
            | Self::PlaySessions
            | Self::Saves
            | Self::Accounts
            | Self::Appearance
            | Self::RommServers => Disposition::Synced,
        }
    }

    pub const fn default_state(self) -> CategoryState {
        match self.disposition() {
            Disposition::Synced => CategoryState::Sync,
            _ => CategoryState::ThisDeviceOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Synced,
    Optional,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CategoryState {
    #[schemars(title = "Sync")]
    Sync,
    #[schemars(title = "This device only")]
    ThisDeviceOnly,
}

/// `core.sync.categories`: each device decides what it sends AND applies.
/// Missing categories use policy defaults; an optional category never turns
/// itself on when a new version adds it. These choices themselves stay local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncSettings {
    #[schemars(title = "What syncs")]
    /// Choose Sync or This device only for each category.
    pub categories: BTreeMap<Category, CategoryState>,
    #[schemars(title = "Never use relays")]
    /// Connect to other devices only directly. A device only a relay could reach shows as offline.
    #[serde(default)]
    pub never_relay: bool,
    #[schemars(title = "Listening port", range(min = 1, max = 65535))]
    /// A fixed UDP port for firewall rules; empty lets the system choose.
    #[serde(default)]
    pub port: Option<u16>,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            categories: Category::ALL
                .into_iter()
                .map(|category| (category, category.default_state()))
                .collect(),
            never_relay: false,
            port: None,
        }
    }
}

impl SyncSettings {
    pub fn state(&self, category: Category) -> CategoryState {
        self.categories
            .get(&category)
            .copied()
            .unwrap_or_else(|| category.default_state())
    }

    /// Both directions use the same gate; transport implementation is #228/#230.
    pub fn allows(&self, category: Category) -> bool {
        self.state(category) == CategoryState::Sync
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receive {
    /// Resolve referenced game/account/release identities before applying.
    MapIdentities,
    /// `overrides_json` in the portable payload contains user values. Apply
    /// them through game edits, preserving this device's automatic layer:
    /// the SQL column itself stores automatic values under manual field keys.
    GameEdits,
    /// Rebind executable locations on the receiving device.
    BindLaunchLocally,
    /// Game scripts always arrive disabled; global/profile scripts stay local.
    DisableGameScript,
    /// Resolve a definition hint only when this device has that emulator.
    ExistingEmulatorHint,
    /// A new server needs its own keychain secret. Removal also removes that
    /// server's downloaded ROMs, through the existing RomM removal operation.
    RommServer,
    /// Drop unknown game references; map merged games and never resurrect an
    /// approval deleted by a merge. One-time approvals are never persisted.
    AccountApproval,
}

#[derive(Debug, Clone, Copy)]
pub enum Condition {
    Any,
    Equals {
        pointer: &'static str,
        value: &'static str,
    },
}

#[derive(Debug)]
pub struct Rule {
    pub name: &'static str,
    pub category: Option<Category>,
    pub reason: &'static str,
    /// Dot-separated leaf paths; `*` matches exactly one array/map segment.
    /// SQL JSON columns must be decoded before field lookup (project does it).
    pub fields: &'static [&'static str],
    pub condition: Condition,
    pub receive: Receive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classification {
    pub disposition: Disposition,
    pub category: Option<Category>,
    pub reason: &'static str,
}

impl Rule {
    pub fn classification(&self) -> Classification {
        Classification {
            disposition: self
                .category
                .map_or(Disposition::Local, Category::disposition),
            category: self.category,
            reason: self.reason,
        }
    }

    /// An undeclared field is local, even in a table containing shared data.
    pub fn field(&self, path: &str) -> Classification {
        self.field_segments(&path.split('.').collect::<Vec<_>>())
    }

    /// Use separate segments for dynamic map keys containing dots, such as
    /// an add-on id. Their punctuation must not create extra nesting.
    pub fn field_segments(&self, path: &[&str]) -> Classification {
        if self.fields.iter().any(|pattern| matches(pattern, path)) {
            return self.classification();
        }
        Classification {
            disposition: Disposition::Local,
            category: None,
            reason: "undeclared fields, locations, automatic values and secrets stay on this device",
        }
    }

    /// A portable JSON projection of stored SQL/setting data for the later
    /// exchange to serialize. This doesn't send, insert, delete or apply
    /// anything. The exchange must check SyncSettings::allows before sending
    /// AND applying, then honor the receive contract for the payload.
    pub fn project(&self, row: &Value, settings: &SyncSettings) -> Option<Value> {
        let category = self.category?;
        if !settings.allows(category) {
            return None;
        }
        let mut row = decode(row);
        if self.receive == Receive::GameEdits {
            manual_game_values(&mut row);
        }
        if let Condition::Equals { pointer, value } = self.condition
            && row.pointer(pointer).and_then(Value::as_str) != Some(value)
        {
            return None;
        }
        let mut projected = project(self, &row, &[])?;
        if self.receive == Receive::DisableGameScript {
            projected
                .get_mut("record_json")?
                .as_object_mut()?
                .insert("enabled".into(), false.into());
        }
        Some(projected)
    }
}

const fn shared(
    name: &'static str,
    category: Category,
    reason: &'static str,
    fields: &'static [&'static str],
) -> Rule {
    Rule {
        name,
        category: Some(category),
        reason,
        fields,
        condition: Condition::Any,
        receive: Receive::MapIdentities,
    }
}
const fn local(name: &'static str, reason: &'static str) -> Rule {
    Rule {
        name,
        category: None,
        reason,
        fields: &[],
        condition: Condition::Any,
        receive: Receive::MapIdentities,
    }
}

/// Every migrated library table, including SQLite's FTS shadow tables. New
/// tables must be classified here; there is no wildcard table policy.
pub const TABLES: &[Rule] = &[
    Rule {
        receive: Receive::GameEdits,
        ..shared(
            "games",
            Category::Library,
            "only explicit user overrides, favorites and hidden flags; automatic metadata stays local",
            &[
                "id",
                "favorite",
                "hidden",
                "overrides_json.title",
                "overrides_json.sortTitle",
                "overrides_json.description",
                "overrides_json.aliases.*",
                "overrides_json.genres.*",
                "overrides_json.tags.*",
                "overrides_json.releaseDate",
                "overrides_json.platforms.*",
                "overrides_json.rating",
                "overrides_json.notes",
            ],
        )
    },
    local(
        "game_entries",
        "installed locations, launch targets and discovery belong to this device",
    ),
    local(
        "storage_roots",
        "absolute folders and drive availability belong to this device",
    ),
    shared(
        "play_sessions",
        Category::PlaySessions,
        "play history travels; process tracking, heartbeat and local entry binding do not",
        &[
            "id",
            "game_id",
            "started_at_ms",
            "ended_at_ms",
            "duration_ms",
        ],
    ),
    Rule {
        condition: Condition::Equals {
            pointer: "/record_json/source",
            value: "user",
        },
        ..shared(
            "artwork",
            Category::Artwork,
            "user-imported artwork is addressed by content hash; cache/thumbnail paths are rebuilt locally",
            &[
                "id",
                "game_id",
                "kind",
                "content_hash",
                "record_json.source",
                "record_json.width",
                "record_json.height",
            ],
        )
    },
    shared(
        "artwork_choices",
        Category::Artwork,
        "the user's selected or removed artwork kinds",
        &["game_id", "kind"],
    ),
    shared(
        "collections",
        Category::Library,
        "account-owned collections and their portable query rules",
        &[
            "id",
            "name",
            "kind",
            "description",
            "pinned",
            "updated_at_ms",
            "account_id",
            "query_json.query",
            "query_json.filter",
            "query_json.platform",
            "query_json.sort",
            "query_json.offset",
            "query_json.limit",
        ],
    ),
    shared(
        "collection_members",
        Category::Library,
        "manual collection membership and order",
        &["collection_id", "game_id", "position"],
    ),
    shared(
        "kept_apart",
        Category::Library,
        "the user's decision not to merge this game automatically",
        &["game_id"],
    ),
    Rule {
        condition: Condition::Equals {
            pointer: "/source",
            value: "manual",
        },
        ..shared(
            "igdb_matches",
            Category::Library,
            "hand-set matches only; fetched metadata and automatic match state are local",
            &["game_id", "igdb_id", "source"],
        )
    },
    Rule {
        condition: Condition::Equals {
            pointer: "/record_json/method/type",
            value: "manual",
        },
        ..shared(
            "achievement_links",
            Category::AchievementLinks,
            "hand-made links only; hash matches and fetched achievements are local",
            &[
                "entry_id",
                "provider",
                "record_json.entryId",
                "record_json.provider",
                "record_json.productId",
                "record_json.method.type",
                "record_json.linkedAtMs",
            ],
        )
    },
    shared(
        "accounts",
        Category::Accounts,
        "local account identity, permissions and PIN hashes travel; PIN attempts, lockouts and active sign-in never do",
        &[
            "id",
            "name",
            "avatar",
            "role",
            "pin_hash",
            "permissions_json.settings.*",
            "permissions_json.install",
            "max_age",
            "unrated_needs_pin",
            "created_at_ms",
            "updated_at_ms",
        ],
    ),
    Rule {
        receive: Receive::AccountApproval,
        ..shared(
            "account_game_approvals",
            Category::Accounts,
            "permanent account/game exceptions follow accounts; map games and preserve merge deletions",
            &["account_id", "game_id"],
        )
    },
    Rule {
        receive: Receive::BindLaunchLocally,
        ..shared(
            "game_actions",
            Category::LaunchActions,
            "optional launch action definitions; executable locations, working folders, arguments and environment remain local",
            &[
                "id",
                "entry_id",
                "position",
                "record_json.id",
                "record_json.entryId",
                "record_json.name",
                "record_json.kind",
                "record_json.target.type",
                "record_json.target.appId",
                "record_json.target.platformId",
                "record_json.tracking.mode",
            ],
        )
    },
    Rule {
        receive: Receive::BindLaunchLocally,
        ..shared(
            "entry_action_settings",
            Category::LaunchActions,
            "optional default/ask choices and display names; tracking locations stay local",
            &[
                "entry_id",
                "record_json.defaultAction",
                "record_json.ask",
                "record_json.builtIn.name",
                "record_json.builtIn.tracking.mode",
            ],
        )
    },
    Rule {
        condition: Condition::Equals {
            pointer: "/scope",
            value: "game",
        },
        receive: Receive::DisableGameScript,
        ..shared(
            "lifecycle_scripts",
            Category::GameScripts,
            "only game-scoped scripts, optional and disabled on receipt; program/cwd bindings remain local",
            &[
                "id",
                "scope",
                "game_id",
                "position",
                "record_json.id",
                "record_json.name",
                "record_json.scope.type",
                "record_json.scope.gameId",
                "record_json.phase",
                "record_json.command.type",
                "record_json.command.shell",
                "record_json.command.script",
                "record_json.timeoutSecs",
            ],
        )
    },
    shared(
        "plugins",
        Category::Addons,
        "optional installed plugin identities and enabled choices; permission grants must be made on each device",
        &["id", "enabled"],
    ),
    local(
        "plugin_kv",
        "untyped plugin private data may contain device paths or credentials",
    ),
    local(
        "emulators",
        "emulator executables and installs belong to this device",
    ),
    local(
        "emulator_profiles",
        "profile bindings and arguments belong to this device",
    ),
    local(
        "managed_emulators",
        "managed installs, their locations and update state belong to this device",
    ),
    local(
        "platform_settings",
        "emulator/profile and BIOS bindings are local; only a separate definition-id preference hint is optional",
    ),
    local(
        "rom_scanners",
        "ROM folders and emulator/profile bindings belong to this device",
    ),
    local(
        "rom_scan_state",
        "scanner progress belongs to this device's folders",
    ),
    local(
        "rom_overrides",
        "scanner-relative ROM paths and bindings belong to this device",
    ),
    local(
        "romm_roms",
        "RomM inventory and downloaded ROM paths are per device",
    ),
    local(
        "release_save_preferences",
        "per-release save locations, overrides and manifest matching belong to this device; save versions are shared separately",
    ),
    shared(
        "save_versions",
        Category::Saves,
        "save versions (#239): who, when and the hash of the file list; never paths, which are relative to each device's own folders",
        &[
            "id",
            "game_id",
            "account_id",
            "device_id",
            "session_id",
            "created_at_ms",
            "content_hash",
            "size_bytes",
            "file_count",
            "parents_json",
            "reason",
        ],
    ),
    local(
        "save_heads",
        "which version this device's save files match belongs to this device",
    ),
    local(
        "managed_tools",
        "managed helper program installations, executable paths and update state belong to this device",
    ),
    local(
        "remote_playtime",
        "each device reads its own stores and launchers",
    ),
    local(
        "store_accounts",
        "store sign-ins and account state are per device; secrets live in its keychain",
    ),
    local(
        "account_entitlements",
        "each device refetches its own store library",
    ),
    local(
        "achievement_sets",
        "each device refetches achievement sets for its sign-ins",
    ),
    local(
        "achievement_items",
        "each device refetches achievement definitions and progress",
    ),
    local(
        "game_updates",
        "installed release/update state belongs to this device",
    ),
    local("icon_checks", "executable icon checks are a local cache"),
    local(
        "sync_changes",
        "the change log is this device's record of what it has and still has to send",
    ),
    local(
        "sync_fields",
        "the clock each field's value here came with, for last-writer-wins",
    ),
    local("sync_seen", "this device's version vector"),
    local("sync_peers", "what each paired device has seen, for this device's exchanges"),
    local(
        "sync_game_ids",
        "which local game each other device's game is; ids differ per device",
    ),
    local(
        "paired_devices",
        "each device keeps its own pairings and the grants each side presents (credentials)",
    ),
    local("igdb_popularity", "IGDB popularity is a refetchable cache"),
    local("game_additions", "notification import journal is local"),
    local("notification_revision", "notification revisions are local"),
    local(
        "notifications",
        "activity feed, read state and failures belong to this device",
    ),
    local(
        "notification_members",
        "notification targets and entry bindings belong to this device",
    ),
    local(
        "settings",
        "settings are classified by namespace/key, never replicated as whole rows",
    ),
    local(
        "_sqlx_migrations",
        "schema installation metadata belongs to this database",
    ),
    local(
        "game_search",
        "derived full-text search index is rebuilt locally",
    ),
    local("game_search_data", "SQLite FTS shadow data is local"),
    local("game_search_idx", "SQLite FTS shadow index is local"),
    local("game_search_docsize", "SQLite FTS shadow lengths are local"),
    local(
        "game_search_config",
        "SQLite FTS shadow configuration is local",
    ),
];

/// Namespaces plus raw settings-table keys. A trailing `.` is an explicitly
/// local prefix for dynamically named keys; it never opts new core namespaces
/// into syncing. `settings.section.` is unwrapped by setting().
pub const SETTINGS: &[Rule] = &[
    shared(
        "core.appearance",
        Category::Appearance,
        "appearance choices follow the user; installed theme files stay local",
        &[
            "theme",
            "density",
            "cardSize",
            "uiScale",
            "showArtTitles",
            "motion",
            "reduceEffects",
            "accent",
            "customCss",
            "customCssEnabled",
        ],
    ),
    shared(
        "core.accounts",
        Category::Accounts,
        "the household's account rating-board preference",
        &["ratingBoard"],
    ),
    Rule {
        receive: Receive::RommServer,
        ..shared(
            "core.romm",
            Category::RommServers,
            "shared server/import definitions only; passwords/tokens never leave the keychain; cache and launch/schedule choices are per device",
            &[
                "servers.*.id",
                "servers.*.name",
                "servers.*.url",
                "servers.*.auth",
                "servers.*.username",
                "servers.*.allowInvalidCerts",
                "servers.*.importAll",
                "servers.*.platforms.*.slug",
                "servers.*.platforms.*.enabled",
                "servers.*.platforms.*.platform",
                "servers.*.collections.*",
            ],
        )
    },
    shared(
        "addons.records",
        Category::Addons,
        "optional installed catalogue add-on ids and versions; rejection errors and local update bookkeeping stay local",
        &["addons.*.version"],
    ),
    local(
        "core.general",
        "startup, cache directory, language and clock choices belong to this device",
    ),
    local("core.console", "console mode belongs to this device"),
    local("core.window", "window geometry belongs to this device"),
    local("core.storage", "storage choices belong to this device"),
    local(
        "core.discovery",
        "discovery paths and launcher configuration belong to this device",
    ),
    local(
        "core.installer",
        "installer choices and compatibility runners belong to this device",
    ),
    local(
        "core.launchers",
        "launcher availability belongs to this device",
    ),
    local(
        "core.emulation",
        "local emulation launch/display behavior, not a preferred-emulator hint",
    ),
    local(
        "core.appUpdates",
        "application update checks belong to this device",
    ),
    local(
        "core.updates",
        "installed game update checks belong to this device",
    ),
    local(
        "core.backups",
        "backup destinations and schedules belong to this device",
    ),
    local("core.onboarding", "setup completion belongs to this device"),
    local(
        "core.metadata",
        "online metadata/cache permission belongs to this device",
    ),
    local(
        "core.saves",
        "Ludusavi manifest source/cache configuration is local; save versions use the Saves category",
    ),
    local(
        "core.streaming",
        "hosting game streams installs and configures programs on this device",
    ),
    local(
        "core.storeAccounts",
        "store sign-in refresh preferences belong to this device",
    ),
    local(
        "core.achievements",
        "achievement refresh preferences belong to this device",
    ),
    local(
        "core.sync",
        "this device's category choices govern sending and applying; they do not overwrite another device's choices",
    ),
    local(
        "storage.move.",
        "move intents refer to this device's folders and drives",
    ),
    local(
        "sync.device",
        "this device's endpoint id, name and kind belong to it; peers learn them while pairing",
    ),
    local(
        "backup.state",
        "backup scheduler state and archive locations are local",
    ),
    local(
        "emulation.rom_exclusions",
        "ROM scan exclusions name local files",
    ),
    local(
        "discovery.import",
        "launcher playtime seeding is a per-device import choice",
    ),
    local(
        "plugins.",
        "plugin settings and compiled-cache keys may contain local paths and credentials",
    ),
];

/// Non-table resources needed by the exchange without inventing migrations
/// in this policy issue. Save-version storage is implemented by the Saves work.
pub const RESOURCES: &[Rule] = &[
    Rule {
        receive: Receive::ExistingEmulatorHint,
        ..shared(
            "preferred_emulators",
            Category::PreferredEmulators,
            "portable platform/definition-id hints, applied only if installed; never platform_settings emulator/profile UUID bindings",
            &["platformId", "definitionId"],
        )
    },
    shared(
        "installed_addons",
        Category::Addons,
        "optional installed add-on identities from the filesystem inventory; no install folders, grants or private data",
        &["id", "version", "kind"],
    ),
    shared(
        "save_blobs",
        Category::Saves,
        "save files and file lists by SHA-256 (#239), sent as blobs on the sync session; never their paths here",
        &["hash", "bytes"],
    ),
    local(
        "active_account",
        "the account signed in on this device is transient local state",
    ),
    local(
        "pin_throttle",
        "failed attempts and lockout deadlines belong to this device",
    ),
    local(
        "keychain",
        "RomM passwords/API tokens and store credentials never sync",
    ),
    local("igdb", "IGDB cache is refetched per device"),
    local(
        "hasheous",
        "Hasheous dumps/indexes are downloaded per device",
    ),
    local(
        "ludusavi",
        "Ludusavi manifest/indexes are downloaded per device",
    ),
    local(
        "romm_download_cache",
        "downloaded ROMs and cache paths belong to this device",
    ),
];

pub fn table(name: &str) -> Option<&'static Rule> {
    TABLES.iter().find(|rule| rule.name == name)
}
pub fn resource(name: &str) -> Option<&'static Rule> {
    RESOURCES.iter().find(|rule| rule.name == name)
}
pub fn setting(key: &str) -> Option<&'static Rule> {
    let key = key.strip_prefix("settings.section.").unwrap_or(key);
    SETTINGS.iter().find(|rule| rule.name == key).or_else(|| {
        SETTINGS
            .iter()
            .find(|rule| rule.name.ends_with('.') && key.starts_with(rule.name))
    })
}

fn matches(pattern: &str, path: &[&str]) -> bool {
    let pattern: Vec<_> = pattern.split('.').collect();
    pattern.len() == path.len()
        && pattern
            .iter()
            .zip(path)
            .all(|(expected, actual)| *expected == "*" || expected == actual)
}

fn decode(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let value = if key.ends_with("_json") {
                        value
                            .as_str()
                            .and_then(|value| serde_json::from_str(value).ok())
                            .unwrap_or_else(|| value.clone())
                    } else {
                        decode(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn manual_game_values(row: &mut Value) {
    let Some(current) = row.get("record_json").and_then(Value::as_object) else {
        if let Some(row) = row.as_object_mut() {
            row.remove("overrides_json");
        }
        return;
    };
    let mut manual = serde_json::Map::new();
    if let Some(markers) = row.get("overrides_json").and_then(Value::as_object) {
        for key in markers.keys() {
            if let Some(value) = current.get(key) {
                manual.insert(key.clone(), value.clone());
            }
        }
    }
    // Rating and notes have no automatic layer, hence no marker in SQL.
    for field in crate::model::GameField::ALL
        .into_iter()
        .filter(|field| !field.automatic())
    {
        if let Some(value) = current.get(field.key()) {
            manual.insert(field.key().into(), value.clone());
        }
    }
    row["overrides_json"] = Value::Object(manual);
}

fn project(rule: &Rule, value: &Value, path: &[&str]) -> Option<Value> {
    match value {
        Value::Object(object) => {
            let object: serde_json::Map<_, _> = object
                .iter()
                .filter_map(|(key, value)| {
                    let mut child = path.to_vec();
                    child.push(key);
                    Some((key.clone(), project(rule, value, &child)?))
                })
                .collect();
            if object.is_empty() && !has_descendants(rule, path) {
                None
            } else {
                Some(Value::Object(object))
            }
        }
        Value::Array(array) => {
            if !has_descendants(rule, path) {
                return None;
            }
            Some(Value::Array(
                array
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| {
                        let index = index.to_string();
                        let mut child = path.to_vec();
                        child.push(&index);
                        project(rule, value, &child)
                    })
                    .collect(),
            ))
        }
        _ if rule.field_segments(path).disposition != Disposition::Local => {
            if path.last() == Some(&"url") {
                let mut url = url::Url::parse(value.as_str()?).ok()?;
                // An endpoint is shared; inline userinfo/query tokens aren't.
                url.set_username("").ok()?;
                url.set_password(None).ok()?;
                url.set_query(None);
                url.set_fragment(None);
                Some(url.to_string().into())
            } else {
                Some(value.clone())
            }
        }
        _ => None,
    }
}

fn has_descendants(rule: &Rule, path: &[&str]) -> bool {
    rule.fields.iter().any(|pattern| {
        let pattern: Vec<_> = pattern.split('.').collect();
        pattern.len() > path.len()
            && pattern
                .iter()
                .zip(path)
                .all(|(expected, actual)| *expected == "*" || expected == actual)
    })
}
