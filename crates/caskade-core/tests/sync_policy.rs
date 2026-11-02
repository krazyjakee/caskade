use caskade_core::{
    AppPaths,
    db::{LibraryStore, MIGRATOR},
    settings::{SectionDefinition, SettingsService},
    sync::policy::{self, Category, CategoryState, Disposition, Receive, SyncSettings},
};
use serde_json::{Value, json};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::{collections::BTreeSet, path::Path};

async fn database() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

async fn settings() -> SettingsService {
    let settings = SettingsService::new(LibraryStore::in_memory().await.unwrap());
    settings
        .register_core(&AppPaths::under(Path::new("fixture")))
        .unwrap();
    caskade_core::romm::register_settings(&settings).unwrap();
    caskade_core::backup::register_settings(&settings).unwrap();
    caskade_core::store_accounts::register_settings(&settings).unwrap();
    settings
}

fn unclassified_namespaces(settings: &SettingsService) -> Vec<String> {
    settings
        .sections()
        .unwrap()
        .into_iter()
        .filter(|section| policy::setting(&section.namespace).is_none())
        .map(|section| section.namespace)
        .collect()
}

async fn unclassified_tables(pool: &SqlitePool) -> Vec<String> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    tables
        .into_iter()
        .filter(|table| policy::table(table).is_none())
        .collect()
}

#[tokio::test]
async fn every_table_from_the_full_migration_list_is_classified() {
    let pool = database().await;
    let missing = unclassified_tables(&pool).await;
    assert!(
        missing.is_empty(),
        "unclassified migrated tables: {missing:?}"
    );
    for rule in policy::TABLES {
        let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
            .bind(rule.name)
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(!columns.is_empty(), "stale policy table: {}", rule.name);
        for path in rule.fields {
            assert!(
                columns
                    .iter()
                    .any(|column| column == path.split('.').next().unwrap()),
                "{}.{} names no column",
                rule.name,
                path
            );
        }
    }
    // Exercise the failure detector, not only today's positive inventory.
    sqlx::query("CREATE TABLE future_unclassified(id TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(unclassified_tables(&pool).await, ["future_unclassified"]);
}

#[tokio::test]
async fn every_registered_namespace_is_classified_and_new_ones_fail_the_guard() {
    let settings = settings().await;
    assert!(unclassified_namespaces(&settings).is_empty());
    settings
        .register(
            SectionDefinition::new(
                "core.futureUnclassified",
                1,
                json!({"type":"object"}),
                json!({}),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        unclassified_namespaces(&settings),
        ["core.futureUnclassified"]
    );
}

/// Catches namespaces in separately registered services too, even if a new
/// service hasn't been added to the registration fixture above yet.
#[test]
fn production_core_namespace_literals_have_no_unclassified_names() {
    fn visit(dir: &Path, missing: &mut BTreeSet<String>) {
        let pattern =
            regex::Regex::new(r#""((?:core|discovery)\.[A-Za-z][A-Za-z0-9_.]*)""#).unwrap();
        let tests = regex::Regex::new(r"#\[cfg\(test\)\]\s*mod tests\s*\{").unwrap();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().unwrap() != "tests" {
                    visit(&path, missing);
                }
                continue;
            }
            let name = path.file_name().unwrap().to_str().unwrap();
            if path.extension().is_none_or(|extension| extension != "rs")
                || name == "tests.rs"
                || name.ends_with("_tests.rs")
            {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            // A top-level `#[cfg(test)] mod tests;` declaration must not
            // hide the production code following it. Inline test modules
            // live at the end of core source files.
            let production = &source[..tests
                .find(&source)
                .map_or(source.len(), |found| found.start())];
            for found in pattern.captures_iter(production) {
                if policy::setting(&found[1]).is_none() {
                    missing.insert(format!("{}: {}", path.display(), &found[1]));
                }
            }
        }
    }
    let mut missing = BTreeSet::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut missing,
    );
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../caskade-sources/src"),
        &mut missing,
    );
    assert!(
        missing.is_empty(),
        "unclassified production namespaces: {missing:#?}"
    );
}

#[test]
fn declarations_are_unique_have_reasons_and_default_to_local_for_unknown_fields() {
    for rules in [policy::TABLES, policy::SETTINGS, policy::RESOURCES] {
        let mut names = BTreeSet::new();
        for rule in rules {
            assert!(names.insert(rule.name), "duplicate {}", rule.name);
            assert!(!rule.reason.is_empty());
            assert_eq!(rule.category.is_some(), !rule.fields.is_empty());
            assert_eq!(
                rule.field("newUndeclaredField").disposition,
                Disposition::Local
            );
        }
    }
    assert!(policy::table("unknown_table").is_none());
    assert_eq!(
        policy::setting("discovery.import")
            .unwrap()
            .classification()
            .disposition,
        Disposition::Local
    );
    assert!(policy::setting("core.futureSetting").is_none());
    assert!(policy::setting("plugins.component-cache-key").is_some());
    assert_eq!(
        policy::setting("storage.move.any-device-intent")
            .unwrap()
            .classification()
            .disposition,
        Disposition::Local
    );
    assert!(std::ptr::eq(
        policy::setting("settings.section.core.romm").unwrap(),
        policy::setting("core.romm").unwrap()
    ));
}

#[test]
fn category_defaults_and_both_direction_gates_are_explicit() {
    let mut settings = SyncSettings::default();
    assert_eq!(settings.categories.len(), Category::ALL.len());
    for category in Category::ALL {
        assert_eq!(
            settings.allows(category),
            category.disposition() == Disposition::Synced
        );
    }
    settings.categories.clear();
    assert!(settings.allows(Category::Saves));
    assert!(!settings.allows(Category::LaunchActions));
    let rule = policy::table("games").unwrap();
    let row = json!({"id":"game", "favorite":true});
    settings
        .categories
        .insert(Category::Library, CategoryState::ThisDeviceOnly);
    assert!(rule.project(&row, &settings).is_none());
    settings
        .categories
        .insert(Category::Library, CategoryState::Sync);
    assert_eq!(rule.project(&row, &settings), Some(row));
    assert_eq!(
        policy::resource("save_versions").unwrap().category,
        Some(Category::Saves)
    );
    assert_eq!(
        policy::table("account_game_approvals").unwrap().receive,
        Receive::AccountApproval
    );
}

#[tokio::test]
async fn category_choices_are_registered_validated_and_persisted_as_device_settings() {
    let settings = settings().await;
    let defaults = settings.get("core.sync").await.unwrap();
    assert_eq!(
        defaults.value,
        serde_json::to_value(SyncSettings::default()).unwrap()
    );
    let value = json!({"categories":{"library":"thisDeviceOnly","launchActions":"sync"}});
    settings
        .set("core.sync", value.clone(), Some(0))
        .await
        .unwrap();
    assert_eq!(settings.get("core.sync").await.unwrap().value, value);
    for invalid in [
        json!({"categories":{"library":"sometimes"}}),
        json!({"categories":{"unknown":"sync"}}),
    ] {
        assert!(settings.set("core.sync", invalid, None).await.is_err());
    }
    assert_eq!(
        policy::setting("core.sync")
            .unwrap()
            .classification()
            .disposition,
        Disposition::Local
    );
}

fn all_categories() -> SyncSettings {
    SyncSettings {
        categories: Category::ALL
            .into_iter()
            .map(|category| (category, CategoryState::Sync))
            .collect(),
    }
}

#[test]
fn romm_splits_shared_server_fields_from_device_bindings_schedules_and_secrets() {
    let rule = policy::setting("core.romm").unwrap();
    let row = json!({"servers":[{
        "id":"home","name":"Home","url":"https://inline-user:fixture-secret@romm.example/prefix?token=fixture-secret#fixture-secret",
        "auth":"password","username":"player","allowInvalidCerts":true,"importAll":false,
        "platforms":[{"slug":"snes","enabled":true,"platform":"nintendo_snes","emulatorId":"local-emulator","profileId":"local-profile"}],
        "collections":[5,9],"syncOnStartup":false,"syncInterval":"1h",
        "password":"fixture-secret","apiToken":"fixture-secret","futureSecret":"fixture-secret"
    }],"cache":{"dir":"/home/local/roms","limitGb":5,"verify":false}});
    let shared = rule.project(&row, &SyncSettings::default()).unwrap();
    assert_eq!(
        shared,
        json!({"servers":[{
            "id":"home","name":"Home","url":"https://romm.example/prefix","auth":"password","username":"player",
            "allowInvalidCerts":true,"importAll":false,"platforms":[{"slug":"snes","enabled":true,"platform":"nintendo_snes"}],"collections":[5,9]
        }]})
    );
    for field in [
        "cache.dir",
        "servers.0.password",
        "servers.0.apiToken",
        "servers.0.platforms.0.emulatorId",
        "servers.0.platforms.0.profileId",
        "servers.0.syncOnStartup",
        "servers.0.syncInterval",
    ] {
        assert_eq!(rule.field(field).disposition, Disposition::Local, "{field}");
    }
    assert_eq!(rule.receive, Receive::RommServer);
    for resource in ["keychain", "romm_download_cache"] {
        assert_eq!(
            policy::resource(resource)
                .unwrap()
                .classification()
                .disposition,
            Disposition::Local
        );
    }
    assert_eq!(
        policy::table("romm_roms")
            .unwrap()
            .classification()
            .disposition,
        Disposition::Local
    );
}

#[test]
fn row_conditions_keep_automatic_matches_artwork_and_non_game_scripts_local() {
    let settings = all_categories();
    for (name, shared, local) in [
        (
            "igdb_matches",
            json!({"game_id":"g","igdb_id":123,"source":"manual"}),
            json!({"game_id":"g","igdb_id":123,"source":"hasheous"}),
        ),
        (
            "artwork",
            json!({"id":"a","record_json":{"source":"user"}}),
            json!({"id":"a","record_json":{"source":"igdb:123"}}),
        ),
        (
            "achievement_links",
            json!({"entry_id":"e","record_json":{"method":{"type":"manual"}}}),
            json!({"entry_id":"e","record_json":{"method":{"type":"hash","hash":"abc"}}}),
        ),
    ] {
        let rule = policy::table(name).unwrap();
        assert!(rule.project(&shared, &settings).is_some(), "{name}");
        assert!(rule.project(&local, &settings).is_none(), "{name}");
    }
    let rule = policy::table("lifecycle_scripts").unwrap();
    let script = json!({"id":"s","scope":"game","game_id":"g","record_json":{"id":"s","name":"Hook","enabled":true,"scope":{"type":"game","gameId":"g"},"command":{"type":"shell","shell":"sh","script":"echo ready"},"cwd":"/home/local","timeoutSecs":60}});
    assert!(rule.project(&script, &SyncSettings::default()).is_none());
    let shared = rule.project(&script, &settings).unwrap();
    assert_eq!(shared["record_json"]["enabled"], false);
    assert!(shared["record_json"].get("cwd").is_none());
    assert_eq!(rule.receive, Receive::DisableGameScript);
    for scope in ["global", "profile"] {
        let mut local = script.clone();
        local["scope"] = scope.into();
        assert!(rule.project(&local, &settings).is_none());
    }
    assert_eq!(
        policy::table("platform_settings")
            .unwrap()
            .classification()
            .disposition,
        Disposition::Local
    );
    assert_eq!(
        policy::resource("preferred_emulators").unwrap().receive,
        Receive::ExistingEmulatorHint
    );
}

fn assert_portable(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                assert!(
                    ![
                        "password",
                        "apiToken",
                        "accessToken",
                        "cachedPath",
                        "external_path",
                        "cwd",
                        "program"
                    ]
                    .contains(&key.as_str()),
                    "local/secret field leaked: {key}"
                );
                assert_portable(value);
            }
        }
        Value::Array(array) => array.iter().for_each(assert_portable),
        Value::String(text) => {
            let bytes = text.as_bytes();
            let windows = bytes.len() > 2
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && b"/\\".contains(&bytes[2]);
            assert!(
                !text.starts_with(['/', '\\']) && !windows && !text.starts_with("file:"),
                "absolute path leaked: {text}"
            );
            assert!(!text.contains("fixture-secret"), "credential leaked");
        }
        _ => {}
    }
}

/// Every default-synced and optional table has a fixture, including hazardous
/// fields in mixed rows. Values use SQL column names and decoded JSON payloads.
#[test]
fn synced_fields_of_fixture_rows_never_include_paths_executables_or_credentials() {
    let settings = all_categories();
    let fixtures = [
        (
            "games",
            json!({"id":"g","title":"automatic","record_json":{"title":"My title","tags":["co-op"],"notes":"My notes","path":"/home/local/game","password":"fixture-secret"},"favorite":true,"hidden":false,"overrides_json":{"title":"automatic title","tags":["automatic tags"],"futurePath":"C:\\Games\\local"}}),
        ),
        (
            "play_sessions",
            json!({"id":"s","game_id":"g","entry_id":"local-entry","started_at_ms":10,"ended_at_ms":20,"duration_ms":10,"heartbeat_at_ms":20,"tracking_json":{"executable":"/usr/bin/game","password":"fixture-secret"}}),
        ),
        (
            "artwork",
            json!({"id":"a","game_id":"g","kind":"cover","content_hash":"abc","record_json":{"source":"user","cachedPath":"/home/local/cover.png","thumbnails":[{"path":"C:\\cache\\small.png"}],"width":100,"height":200}}),
        ),
        ("artwork_choices", json!({"game_id":"g","kind":"cover"})),
        (
            "collections",
            json!({"id":"c","name":"My games","kind":"smart","account_id":"a","query_json":{"query":"co-op","filter":"all","futurePath":"/etc/local"}}),
        ),
        (
            "collection_members",
            json!({"collection_id":"c","game_id":"g","position":1}),
        ),
        ("kept_apart", json!({"game_id":"g"})),
        (
            "igdb_matches",
            json!({"game_id":"g","igdb_id":123,"source":"manual","applied_json":{"path":"/tmp/local"}}),
        ),
        (
            "achievement_links",
            json!({"entry_id":"e","provider":"retroachievements","record_json":{"method":{"type":"manual"},"productId":"123","futurePath":"\\\\server\\share"}}),
        ),
        (
            "accounts",
            json!({"id":"a","name":"Player","avatar":"avatar:rocket","role":"standard","pin_hash":"$argon2id$fixture-hash","permissions_json":{"settings":["appearance"],"install":false,"password":"fixture-secret"},"pin_failures":5,"pin_lockouts":2,"pin_locked_until_ms":12345,"active":true,"password":"fixture-secret"}),
        ),
        (
            "account_game_approvals",
            json!({"account_id":"a","game_id":"g"}),
        ),
        (
            "game_actions",
            json!({"id":"action","entry_id":"e","record_json":{"name":"Play","kind":"play","target":{"type":"executable","path":"C:\\Games\\game.exe","cwd":"/home/local","args":["/home/local/file"],"env":{"PASSWORD":"fixture-secret"}},"tracking":{"mode":"folder","folder":"/home/local"}}}),
        ),
        (
            "entry_action_settings",
            json!({"entry_id":"e","record_json":{"ask":true,"defaultAction":"a","builtIn":{"name":"Play","tracking":{"folder":"/home/local","names":["/usr/bin/game"]}}}}),
        ),
        (
            "lifecycle_scripts",
            json!({"id":"s","scope":"game","game_id":"g","record_json":{"enabled":true,"command":{"type":"shell","shell":"sh","script":"echo ready","program":"/bin/sh"},"cwd":"/home/local"}}),
        ),
        (
            "plugins",
            json!({"id":"dev.example.stats","enabled":true,"grants_json":{"fs":["/home/local"],"password":"fixture-secret"}}),
        ),
    ];
    let mut covered = BTreeSet::new();
    for (name, mut row) in fixtures {
        // New unknown fields and encoded SQL JSON must be denied too.
        row["futureExecutable"] = "/usr/local/bin/private".into();
        if let Some(object) = row.as_object_mut() {
            for (key, value) in object.iter_mut() {
                if key.ends_with("_json") {
                    *value = Value::String(value.to_string());
                }
            }
        }
        let shared = policy::table(name)
            .unwrap()
            .project(&row, &settings)
            .unwrap();
        assert_portable(&shared);
        covered.insert(name);
    }
    let declared: BTreeSet<_> = policy::TABLES
        .iter()
        .filter(|rule| rule.category.is_some())
        .map(|rule| rule.name)
        .collect();
    assert_eq!(
        covered, declared,
        "every transferable table needs a path/credential fixture"
    );
    for (name, row) in [
        (
            "core.appearance",
            json!({"theme":"caskade.default","accent":"#ff8800","futureExecutable":"/bin/private"}),
        ),
        (
            "core.accounts",
            json!({"ratingBoard":"pegi","activeAccount":"/home/local","password":"fixture-secret"}),
        ),
        (
            "addons.records",
            json!({"addons":{"dev.example.stats":{"version":"1.0.0","rejected":{"message":"/home/local"}}}}),
        ),
    ] {
        assert_portable(
            &policy::setting(name)
                .unwrap()
                .project(&row, &settings)
                .unwrap(),
        );
    }
    assert_portable(
        &policy::resource("save_versions")
            .unwrap()
            .project(
                &json!({"id":"v","gameId":"g","contentHash":"abc","localPath":"/home/local/save"}),
                &settings,
            )
            .unwrap(),
    );
    for (name, row) in [
        (
            "preferred_emulators",
            json!({"platformId":"nintendo_snes","definitionId":"snes9x","path":"/bin/snes9x","profileId":"local"}),
        ),
        (
            "installed_addons",
            json!({"id":"dev.example.stats","version":"1.0.0","kind":"plugin","dir":"C:\\plugins","password":"fixture-secret"}),
        ),
    ] {
        assert_portable(
            &policy::resource(name)
                .unwrap()
                .project(&row, &settings)
                .unwrap(),
        );
    }
}

#[test]
fn empty_lists_preserve_user_clears_without_widening_the_allowlist() {
    let row = json!({"id":"g","overrides_json":{"tags":["auto"],"aliases":["auto"],"futurePath":[]},"record_json":{"tags":[],"aliases":[]}});
    let projected = policy::table("games")
        .unwrap()
        .project(&row, &SyncSettings::default())
        .unwrap();
    assert_eq!(
        projected,
        json!({"id":"g","overrides_json":{"tags":[],"aliases":[]}})
    );
}

#[test]
fn dotted_addon_ids_remain_one_map_key_and_keep_the_installed_version() {
    let rule = policy::setting("addons.records").unwrap();
    let row = json!({"addons":{"dev.example.stats":{"version":"1.0.0","path":"/home/local","password":"fixture-secret"}}});
    assert_eq!(
        rule.field_segments(&["addons", "dev.example.stats", "version"])
            .disposition,
        Disposition::Optional
    );
    assert_eq!(
        rule.project(&row, &all_categories()),
        Some(json!({"addons":{"dev.example.stats":{"version":"1.0.0"}}}))
    );
}

#[tokio::test]
async fn game_projection_uses_manual_values_and_never_the_automatic_layer_in_sql() {
    use caskade_core::model::{
        Game, GameEdit, GameEditOutcome, GameField, ListEdit, NotesEdit, RatingEdit, TextEdit,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.sqlite");
    let store = LibraryStore::open(&path).await.unwrap();
    let mut game = Game::new("Automatic source title");
    game.description = "Automatic source description".into();
    game.tags = vec!["Automatic source tag".into()];
    store.upsert_game(&game).await.unwrap();
    let edit = GameEdit {
        title: Some(TextEdit::Set {
            value: "My title".into(),
        }),
        tags: Some(ListEdit::Set { values: Vec::new() }),
        rating: Some(RatingEdit { value: Some(90) }),
        notes: Some(NotesEdit {
            value: "My notes".into(),
        }),
        ..Default::default()
    };
    assert!(matches!(
        store.edit_game(&game.id, &edit, None).await.unwrap(),
        GameEditOutcome::Saved { .. }
    ));
    let db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(path))
        .await
        .unwrap();
    let (record, markers): (String, String) =
        sqlx::query_as("SELECT record_json,overrides_json FROM games WHERE id=?")
            .bind(&game.id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&markers).unwrap()["title"],
        "Automatic source title"
    );
    let row = json!({"id":game.id,"record_json":record,"overrides_json":markers});
    let rule = policy::table("games").unwrap();
    let shared = rule.project(&row, &SyncSettings::default()).unwrap();
    assert_eq!(shared["overrides_json"]["title"], "My title");
    assert_eq!(shared["overrides_json"]["tags"], json!([]));
    assert_eq!(shared["overrides_json"]["rating"], 90);
    assert_eq!(shared["overrides_json"]["notes"], "My notes");
    assert!(shared["overrides_json"].get("description").is_none());
    assert!(!shared.to_string().contains("Automatic source"));
    assert_eq!(rule.receive, Receive::GameEdits);
    for field in GameField::ALL {
        assert!(
            rule.fields.iter().any(|path| path
                .strip_prefix("overrides_json.")
                .and_then(|path| path.split('.').next())
                == Some(field.key())),
            "unclassified editable game field: {field:?}"
        );
    }
}
