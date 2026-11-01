//! Restoring backups into isolated profiles (#192).

use super::{
    tests::{Profile, fill, hash, live, options, profile, quiet},
    *,
};
use crate::model::{Game, GameEntry, InstallLocation, SourceId, StorageRoot};
use std::{collections::BTreeSet, path::Path};
use tokio_util::sync::CancellationToken;

async fn ids(paths: &AppPaths) -> Vec<String> {
    sqlx::query_scalar("SELECT id FROM games ORDER BY id")
        .fetch_all(&mut live(paths).await)
        .await
        .unwrap()
}

async fn titles(paths: &AppPaths) -> Vec<String> {
    sqlx::query_scalar("SELECT title FROM games ORDER BY title")
        .fetch_all(&mut live(paths).await)
        .await
        .unwrap()
}

/// A filled profile's backup, with every component.
async fn backup() -> (Profile, Exported) {
    let source = profile().await;
    fill(&source).await;
    source
        .store
        .upsert_plugin_state(&crate::db::PluginRow {
            id: "fixture.plugin".into(),
            enabled: true,
            grants: serde_json::json!({"permissions": {"fs": []}}),
        })
        .await
        .unwrap();
    let exported = source
        .backups
        .export(
            options(&source, &Component::ALL),
            quiet(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    (source, exported)
}

/// Another profile, with a game of its own and a plugin of its own.
async fn other() -> Profile {
    let target = profile().await;
    target
        .store
        .upsert_game(&Game::new("Only here"))
        .await
        .unwrap();
    let plugin = target.paths.data.join("plugins/local.plugin");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.json"), b"{}").unwrap();
    target
}

fn restore_options(archive: &Path, components: &[Component]) -> RestoreOptions {
    RestoreOptions {
        archive: archive.to_owned(),
        components: components.iter().copied().collect(),
    }
}

/// Stops using the profile's library, as the restart does.
async fn restart(profile: &Profile) {
    profile.store.close().await;
}

async fn plugin_grants(paths: &AppPaths) -> Vec<(String, bool, String)> {
    sqlx::query_as("SELECT id, enabled, grants_json FROM plugins ORDER BY id")
        .fetch_all(&mut live(paths).await)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_restore_replaces_the_profile_after_a_restart() {
    let (source, exported) = backup().await;
    let target = other().await;

    let inspection = target
        .backups
        .inspect(&exported.path, quiet(), CancellationToken::new())
        .await
        .unwrap();
    assert!(!inspection.needs_migration && !inspection.other_os);
    assert!(inspection.manifest.files.is_empty());
    assert_eq!(inspection.components.len(), Component::ALL.len());

    let journal = target
        .backups
        .stage_restore(
            restore_options(&exported.path, &[Component::Plugins, Component::Artwork]),
            quiet(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(journal.state, RestoreState::Staged);
    assert_eq!(
        journal.components,
        BTreeSet::from([Component::Library, Component::Artwork, Component::Plugins])
    );
    // Nothing changed yet but the artwork cache.
    assert_eq!(titles(&target.paths).await, ["Only here"]);
    assert!(target.paths.data.join("plugins/local.plugin").exists());
    assert!(
        target
            .paths
            .cache
            .join("artwork")
            .join(hash('a'))
            .join("original.webp")
            .exists()
    );
    assert_eq!(
        target.backups.staged_restore().unwrap().unwrap().state,
        RestoreState::Staged
    );

    restart(&target).await;
    let applied = restore::apply_pending(&target.paths).unwrap().unwrap();
    assert_eq!(applied.state, RestoreState::Applied);
    let reopened = LibraryStore::open_default(&target.paths).await.unwrap();
    restore::finish(&target.paths).unwrap();
    // Replaced, not merged: the same games and ids, the local one gone.
    assert_eq!(ids(&target.paths).await, ids(&source.paths).await);
    assert!(
        !titles(&target.paths)
            .await
            .contains(&"Only here".to_owned())
    );
    assert!(
        target
            .paths
            .data
            .join("plugins/fixture.plugin/ui/index.js")
            .exists()
    );
    assert!(!target.paths.data.join("plugins/local.plugin").exists());
    // The plugin's files came back with it: its grants stand.
    assert!(plugin_grants(&target.paths).await[0].1);
    assert!(!restore::folder(&target.paths).join("rollback").exists());
    assert!(restore::pending(&target.paths).unwrap().is_none());
    let outcome = restore::take_outcome(&target.paths).unwrap();
    assert_eq!(outcome.error, None);
    assert!(restore::take_outcome(&target.paths).is_none());
    reopened.close().await;
}

#[tokio::test]
async fn without_their_files_restored_plugins_ask_again() {
    let (_source, exported) = backup().await;
    let target = other().await;
    target
        .backups
        .stage_restore(
            restore_options(&exported.path, &[]),
            quiet(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    restart(&target).await;
    restore::apply_pending(&target.paths).unwrap();
    restore::finish(&target.paths).unwrap();
    assert_eq!(
        plugin_grants(&target.paths).await,
        [("fixture.plugin".to_owned(), false, "null".to_owned())]
    );
    // Components left out stay as they were.
    assert!(target.paths.data.join("plugins/local.plugin").exists());
}

#[tokio::test]
async fn a_library_that_wont_open_is_rolled_back() {
    let (_source, exported) = backup().await;
    let target = other().await;
    target
        .backups
        .stage_restore(
            restore_options(&exported.path, &[Component::Plugins, Component::Themes]),
            quiet(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    restart(&target).await;
    assert!(!target.paths.data.join("themes").exists());
    restore::apply_pending(&target.paths).unwrap();
    assert!(target.paths.data.join("themes").is_dir());
    restore::roll_back(&target.paths, "the library didn't open").unwrap();
    assert_eq!(titles(&target.paths).await, ["Only here"]);
    assert!(target.paths.data.join("plugins/local.plugin").exists());
    assert!(!target.paths.data.join("themes").exists());
    assert!(!target.paths.data.join("plugins/fixture.plugin").exists());
    let outcome = restore::take_outcome(&target.paths).unwrap();
    assert_eq!(outcome.error.as_deref(), Some("the library didn't open"));
    assert!(restore::pending(&target.paths).unwrap().is_none());
}

#[tokio::test]
async fn an_interrupted_apply_or_rollback_carries_on_at_the_next_start() {
    let (source, exported) = backup().await;
    let target = other().await;
    target
        .backups
        .stage_restore(
            restore_options(&exported.path, &[Component::Plugins]),
            quiet(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    restart(&target).await;
    // A crash after the old library moved aside, before the new one came.
    let dir = restore::folder(&target.paths);
    let mut journal = restore::pending(&target.paths).unwrap().unwrap();
    journal.state = RestoreState::Applying;
    std::fs::write(
        dir.join("journal.json"),
        serde_json::to_vec(&journal).unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("rollback")).unwrap();
    std::fs::rename(target.paths.database(), dir.join("rollback/library.sqlite")).unwrap();
    restore::apply_pending(&target.paths).unwrap().unwrap();
    assert_eq!(ids(&target.paths).await, ids(&source.paths).await);

    // A crash halfway through rolling back.
    let mut journal = restore::pending(&target.paths).unwrap().unwrap();
    journal.state = RestoreState::RollingBack;
    std::fs::write(
        dir.join("journal.json"),
        serde_json::to_vec(&journal).unwrap(),
    )
    .unwrap();
    std::fs::remove_dir_all(target.paths.data.join("plugins")).unwrap();
    std::fs::rename(
        dir.join("rollback/plugins"),
        target.paths.data.join("plugins"),
    )
    .unwrap();
    assert!(restore::apply_pending(&target.paths).unwrap().is_none());
    assert_eq!(titles(&target.paths).await, ["Only here"]);
    assert!(target.paths.data.join("plugins/local.plugin").exists());
    assert!(
        restore::take_outcome(&target.paths)
            .unwrap()
            .error
            .is_some()
    );
}

#[tokio::test]
async fn a_library_that_cant_open_can_still_be_restored() {
    let (source, exported) = backup().await;
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    paths.create().unwrap();
    std::fs::write(paths.database(), b"this is not a database").unwrap();
    assert!(LibraryStore::open_default(&paths).await.is_err());
    restore::stage(
        &paths,
        &restore_options(&exported.path, &[]),
        &CancellationToken::new(),
        &quiet(),
    )
    .await
    .unwrap();
    restore::apply_pending(&paths).unwrap().unwrap();
    LibraryStore::open_default(&paths)
        .await
        .unwrap()
        .close()
        .await;
    restore::finish(&paths).unwrap();
    assert_eq!(ids(&paths).await, ids(&source.paths).await);
}

#[tokio::test]
async fn missing_locations_and_games_are_reported() {
    let source = profile().await;
    let gone = source._temp.path().join("unplugged");
    source
        .store
        .upsert_root(&StorageRoot {
            id: "01a1aaaa-0000-7000-8000-000000000001".into(),
            path: gone.clone(),
            is_default: false,
            label: None,
        })
        .await
        .unwrap();
    let game = Game::new("Elsewhere");
    source.store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "elsewhere");
    entry.location = Some(InstallLocation::External {
        path: source._temp.path().join("no such game"),
    });
    source.store.upsert_entry(&entry).await.unwrap();
    let exported = source
        .backups
        .export(options(&source, &[]), quiet(), CancellationToken::new())
        .await
        .unwrap();
    let inspection = source
        .backups
        .inspect(&exported.path, quiet(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(inspection.missing_roots, [gone]);
    assert_eq!(inspection.missing_games, 1);
}

/// An archive of `database` claiming `schema`.
fn handmade(database: &Path, schema: i64, into: &Path) -> std::path::PathBuf {
    let manifest = Manifest {
        format: FORMAT,
        app_version: "0.0.1".into(),
        schema_version: schema,
        created_at_ms: 0,
        os: std::env::consts::OS.into(),
        set_id: None,
        components: vec![ComponentSummary {
            id: Component::Library,
            files: 0,
            bytes: 0,
        }],
        tables: Vec::new(),
        excluded: Exclusion::ALL.to_vec(),
        files: Vec::new(),
    };
    let path = into.join("handmade.zip");
    archive::write(
        std::fs::File::create(&path).unwrap(),
        &[archive::Source {
            path: DATABASE.into(),
            component: Component::Library,
            file: database.to_owned(),
        }],
        manifest,
        &CancellationToken::new(),
        &|_| {},
    )
    .unwrap();
    path
}

/// A database migrated only up to `version`, with one game.
async fn old_database(file: &Path, version: i64) {
    use sqlx::ConnectOptions;
    let mut connection = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(file)
        .create_if_missing(true)
        .connect()
        .await
        .unwrap();
    crate::db::MIGRATOR
        .run_to(version, &mut connection)
        .await
        .unwrap();
    let game = Game::new("From long ago");
    sqlx::query(
        "INSERT INTO games(id,title,sort_title,aliases_json,tags_json,favorite,hidden,record_json) \
         VALUES (?,?,?,'[]','[]',0,0,?)",
    )
    .bind(&game.id)
    .bind(&game.title)
    .bind(&game.title)
    .bind(serde_json::to_string(&game).unwrap())
    .execute(&mut connection)
    .await
    .unwrap();
    sqlx::Connection::close(connection).await.unwrap();
}

#[tokio::test]
async fn an_older_backup_is_migrated_and_a_newer_one_refused() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("old.sqlite");
    old_database(&old, 14).await;
    let archive_path = handmade(&old, 14, temp.path());
    let target = other().await;
    let inspection = target
        .backups
        .inspect(&archive_path, quiet(), CancellationToken::new())
        .await
        .unwrap();
    assert!(inspection.needs_migration);
    target
        .backups
        .stage_restore(
            restore_options(&archive_path, &[]),
            quiet(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    restart(&target).await;
    restore::apply_pending(&target.paths).unwrap();
    let store = LibraryStore::open_default(&target.paths).await.unwrap();
    restore::finish(&target.paths).unwrap();
    assert_eq!(titles(&target.paths).await, ["From long ago"]);
    store.close().await;

    // The manifest claims a newer schema.
    let newest = crate::db::MIGRATOR.iter().map(|m| m.version).max().unwrap();
    let newer = handmade(&old, newest + 1, temp.path());
    let fresh = profile().await;
    let error = fresh
        .backups
        .inspect(&newer, quiet(), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, BackupError::Newer), "{error:?}");
    // Its database has a migration this Caskade doesn't.
    let mut connection = snapshot::open(&old).await.unwrap();
    sqlx::query(
        "INSERT INTO _sqlx_migrations(version,description,installed_on,success,checksum,execution_time) \
         VALUES (99999,'future',CURRENT_TIMESTAMP,1,x'00',0)",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    sqlx::Connection::close(connection).await.unwrap();
    let sneaky = handmade(&old, 14, temp.path());
    let error = fresh
        .backups
        .stage_restore(
            restore_options(&sneaky, &[]),
            quiet(),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, BackupError::Newer), "{error:?}");
    assert!(restore::pending(&fresh.paths).unwrap().is_none());
}

#[tokio::test]
async fn a_cancelled_or_damaged_restore_changes_nothing() {
    let (_source, exported) = backup().await;
    let target = other().await;
    let cancel = CancellationToken::new();
    let progress: ProgressFn = std::sync::Arc::new({
        let cancel = cancel.clone();
        move |progress| {
            if progress.phase == Phase::Extracting {
                cancel.cancel();
            }
        }
    });
    let error = target
        .backups
        .stage_restore(
            restore_options(&exported.path, &[Component::Plugins]),
            progress,
            cancel,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, BackupError::Cancelled), "{error:?}");
    assert!(restore::pending(&target.paths).unwrap().is_none());
    assert!(!restore::folder(&target.paths).join("staged").exists());

    let temp = tempfile::tempdir().unwrap();
    let damaged = temp.path().join("damaged.zip");
    let mut bytes = std::fs::read(&exported.path).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xff;
    std::fs::write(&damaged, bytes).unwrap();
    let error = target
        .backups
        .stage_restore(
            restore_options(&damaged, &[]),
            quiet(),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, BackupError::Damaged(_)), "{error:?}");
    restart(&target).await;
    assert!(restore::apply_pending(&target.paths).unwrap().is_none());
    assert_eq!(titles(&target.paths).await, ["Only here"]);
}
