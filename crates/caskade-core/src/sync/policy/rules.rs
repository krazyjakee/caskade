use super::{Category, Condition, Receive, Rule, SyncSettings};

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
        "play history and the device it was played on travel; process tracking, heartbeat and local entry binding do not",
        &[
            "id",
            "game_id",
            "started_at_ms",
            "ended_at_ms",
            "duration_ms",
            "device_id",
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
    local(
        "sync_peers",
        "what each paired device has seen, for this device's exchanges",
    ),
    local(
        "sync_game_ids",
        "which local game each other device's game is; ids differ per device",
    ),
    local(
        "paired_devices",
        "each device keeps its own pairings and the grants each side presents (credentials)",
    ),
    local(
        "shares",
        "one-off shares belong to the device that shares; their grants are credentials and never leave it",
    ),
    local(
        "share_recipients",
        "who received this device's shares, and how far each got, belongs to this device",
    ),
    local("igdb_popularity", "IGDB popularity is a refetchable cache"),
    local("game_additions", "notification import journal is local"),
    local("notification_revision", "notification revisions are local"),
    local(
        "notifications",
        "activity feed, read state and failures belong to this device",
    ),
    local(
        "paused_jobs",
        "paused installs and their downloads belong to this device's storage",
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
        "appearance choices follow the user; the UI scale (it suits each screen) and installed theme files stay local",
        &[
            "theme",
            "density",
            "cardSize",
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
        "core.ports",
        "whether game pages list ports, and where their plans come from, belong to this device",
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
        SyncSettings::NAMESPACE,
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
        "streaming.ownHost",
        "this device's own streaming host is no other device's",
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
        "installed_copies",
        Category::Library,
        "each device's installed copies of synced games (#232): release key, size, build OS, location kind and whether it can be copied; never paths, which stay on their device",
        &[
            "device", "game", "entry", "release", "size", "os", "location", "copyable",
        ],
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
