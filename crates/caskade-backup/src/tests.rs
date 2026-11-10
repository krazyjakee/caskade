//! Backups of a file library under a temporary profile.

use super::*;
use caskade_core::model::{Artwork, ArtworkKind, Game};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio_util::sync::CancellationToken;

pub(super) const SECRET: &str = "0123456789abcdef-component-cache-secret";

pub(super) struct Profile {
    pub _temp: tempfile::TempDir,
    pub paths: AppPaths,
    pub store: LibraryStore,
    pub backups: Backups,
    pub destination: PathBuf,
}

pub(super) async fn profile() -> Profile {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    paths.create().unwrap();
    let store = LibraryStore::open_default(&paths).await.unwrap();
    let destination = temp.path().join("backups");
    std::fs::create_dir(&destination).unwrap();
    Profile {
        backups: Backups::new(store.clone(), paths.clone()),
        _temp: temp,
        paths,
        store,
        destination,
    }
}

/// A game with a cover the user picked and one from IGDB, a few settings
/// and a plugin's files.
pub(super) async fn fill(profile: &Profile) {
    let store = &profile.store;
    for index in 0..20 {
        store
            .upsert_game(&Game::new(format!("Game {index}")))
            .await
            .unwrap();
    }
    let game = Game::new("Covered");
    store.upsert_game(&game).await.unwrap();
    for (hash, source) in [(hash('a'), "user:picked"), (hash('b'), "igdb:co1")] {
        let dir = profile.paths.cache.join("artwork").join(&hash);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("original.webp"), hash.as_bytes()).unwrap();
        std::fs::write(dir.join("entry.json"), b"{}").unwrap();
        std::fs::write(dir.join(".lock"), b"").unwrap();
        store
            .upsert_artwork(&Artwork::new(
                &game.id,
                ArtworkKind::Cover,
                &hash,
                source,
                format!("{hash}/original.webp"),
                1,
                1,
            ))
            .await
            .unwrap();
    }
    store
        .set_setting("plugins.component-cache-key", &serde_json::json!(SECRET))
        .await
        .unwrap();
    store
        .set_setting("storage.move.01", &serde_json::json!({"intent": SECRET}))
        .await
        .unwrap();
    store
        .set_setting("streaming.ownHost", &serde_json::json!(SECRET))
        .await
        .unwrap();
    store
        .set_setting("app.kept", &serde_json::json!("kept"))
        .await
        .unwrap();
    store.save_icon_check(&game.id, "signature").await.unwrap();
    let plugin = profile.paths.data.join("plugins/fixture.plugin");
    std::fs::create_dir_all(plugin.join("ui")).unwrap();
    std::fs::write(plugin.join("plugin.json"), b"{\"id\":\"fixture.plugin\"}").unwrap();
    std::fs::write(plugin.join("ui/index.js"), b"export {}").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc", plugin.join("outside")).unwrap();
}

pub(super) fn hash(c: char) -> String {
    std::iter::repeat_n(c, 64).collect()
}

pub(super) fn options(profile: &Profile, components: &[Component]) -> ExportOptions {
    ExportOptions {
        destination: profile.destination.clone(),
        components: components.iter().copied().collect(),
        set_id: None,
    }
}

pub(super) fn quiet() -> ProgressFn {
    Arc::new(|_| {})
}

/// The destination's file names.
pub(super) fn listing(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

/// A read-only connection to the live library.
pub(super) async fn live(paths: &AppPaths) -> sqlx::SqliteConnection {
    use sqlx::ConnectOptions;
    sqlx::sqlite::SqliteConnectOptions::new()
        .filename(paths.database())
        .read_only(true)
        .connect()
        .await
        .unwrap()
}

/// The backup's database, extracted and opened.
async fn database(archive: &Path, into: &Path) -> sqlx::SqliteConnection {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive).unwrap()).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(DATABASE)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let file = into.join("restored.sqlite");
    std::fs::write(&file, &bytes).unwrap();
    snapshot::open(&file).await.unwrap()
}

#[tokio::test]
async fn a_backup_holds_the_library_and_chosen_components_without_secrets() {
    let profile = profile().await;
    fill(&profile).await;
    let plan = profile
        .backups
        .plan(&options(
            &profile,
            &[Component::Artwork, Component::Plugins],
        ))
        .await
        .unwrap();
    assert!(plan.destination_exists && plan.available_bytes.is_some());
    assert_eq!(plan.excluded, Exclusion::ALL);
    let planned = |id| plan.components.iter().find(|c| c.id == id).unwrap().clone();
    assert!(planned(Component::Library).selected);
    assert_eq!(
        (
            planned(Component::Artwork).files,
            planned(Component::Artwork).selected
        ),
        (2, true)
    );
    assert_eq!(
        (
            planned(Component::DownloadedArtwork).files,
            planned(Component::DownloadedArtwork).selected
        ),
        (2, false)
    );
    assert_eq!(planned(Component::Plugins).files, 2);

    let exported = profile
        .backups
        .export(
            options(&profile, &[Component::Artwork, Component::Plugins]),
            quiet(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let name = exported.path.file_name().unwrap().to_string_lossy();
    assert!(name.starts_with(PREFIX) && name.ends_with(".zip"), "{name}");
    assert_eq!(
        listing(&profile.destination),
        BTreeSet::from([name.to_string()])
    );
    let manifest = &exported.manifest;
    assert_eq!(manifest.format, FORMAT);
    assert_eq!(manifest.app_version, caskade_core::VERSION);
    let newest = caskade_core::db::MIGRATOR
        .iter()
        .map(|m| m.version)
        .max()
        .unwrap();
    assert_eq!(manifest.schema_version, newest);
    let paths: BTreeSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        BTreeSet::from([
            DATABASE,
            &*format!("artwork/{}/entry.json", hash('a')),
            &*format!("artwork/{}/original.webp", hash('a')),
            "plugins/fixture.plugin/plugin.json",
            "plugins/fixture.plugin/ui/index.js",
        ])
    );
    assert_eq!(
        verify(&exported.path, &CancellationToken::new(), &|_| {}).unwrap(),
        *manifest
    );
    let rows = |table: &str| {
        manifest
            .tables
            .iter()
            .find(|t| t.name == table)
            .map(|t| t.rows)
    };
    assert_eq!(rows("games"), Some(21));
    assert_eq!(rows("artwork"), Some(2));
    assert_eq!(rows("icon_checks"), Some(0));
    assert_eq!(rows("accounts"), Some(1));
    assert_eq!(rows("_sqlx_migrations"), None);

    // The snapshot: the same games and ids, the secrets gone from every
    // page.
    let temp = tempfile::tempdir().unwrap();
    let mut restored = database(&exported.path, temp.path()).await;
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM games ORDER BY id")
        .fetch_all(&mut restored)
        .await
        .unwrap();
    let live: Vec<String> = sqlx::query_scalar("SELECT id FROM games ORDER BY id")
        .fetch_all(&mut live(&profile.paths).await)
        .await
        .unwrap();
    assert_eq!(ids, live);
    let keys: Vec<String> = sqlx::query_scalar("SELECT key FROM settings ORDER BY key")
        .fetch_all(&mut restored)
        .await
        .unwrap();
    assert!(keys.contains(&"app.kept".to_owned()), "{keys:?}");
    assert!(!keys.iter().any(|k| k.starts_with("plugins.")
        || k.starts_with("storage.")
        || k.starts_with("streaming.")));
    let raw = std::fs::read(temp.path().join("restored.sqlite")).unwrap();
    assert!(!raw.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()));
}

#[tokio::test]
async fn a_backup_taken_during_writes_is_one_point_in_time() {
    let profile = profile().await;
    let stop = Arc::new(AtomicBool::new(false));
    let writer = tokio::spawn({
        let (store, stop) = (profile.store.clone(), stop.clone());
        async move {
            let mut written = 0;
            while !stop.load(Ordering::Relaxed) {
                store
                    .upsert_game(&Game::new(format!("Busy {written}")))
                    .await
                    .unwrap();
                written += 1;
                tokio::task::yield_now().await;
            }
            written
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let exported = profile
        .backups
        .export(options(&profile, &[]), quiet(), CancellationToken::new())
        .await
        .unwrap();
    stop.store(true, Ordering::Relaxed);
    let written = writer.await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut restored = database(&exported.path, temp.path()).await;
    let games: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM games")
        .fetch_one(&mut restored)
        .await
        .unwrap();
    // The search index agrees with the games: one transaction's view.
    let indexed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM game_search")
        .fetch_one(&mut restored)
        .await
        .unwrap();
    let recorded = exported
        .manifest
        .tables
        .iter()
        .find(|t| t.name == "games")
        .unwrap()
        .rows;
    assert!(games > 0 && games <= written);
    assert_eq!((games, indexed), (recorded as i64, games));
    snapshot::check(&mut restored).await.unwrap();
}

/// A good backup, then `fault`: the good backup is all that's left.
async fn fails_cleanly(
    setup: impl FnOnce(&Backups),
    cancel: CancellationToken,
    progress: ProgressFn,
) -> (Profile, BackupError) {
    let profile = profile().await;
    fill(&profile).await;
    let good = profile
        .backups
        .export(options(&profile, &[]), quiet(), CancellationToken::new())
        .await
        .unwrap();
    // Not the same second's name.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    setup(&profile.backups);
    let error = profile
        .backups
        .export(
            options(&profile, &[Component::Artwork, Component::Plugins]),
            progress,
            cancel,
        )
        .await
        .unwrap_err();
    let good_name = good
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    if !profile.backups.faults().crash {
        assert_eq!(listing(&profile.destination), BTreeSet::from([good_name]));
    }
    verify(&good.path, &CancellationToken::new(), &|_| {}).unwrap();
    (profile, error)
}

#[tokio::test]
async fn a_full_disk_leaves_the_last_backup_alone() {
    let (_, error) = fails_cleanly(
        |backups| caskade_core::lock(&backups.faults).disk_full_after = Some(2048),
        CancellationToken::new(),
        quiet(),
    )
    .await;
    assert!(
        matches!(&error, BackupError::Io(e) if e.kind() == std::io::ErrorKind::StorageFull),
        "{error:?}"
    );
}

#[tokio::test]
async fn cancelling_leaves_the_last_backup_alone() {
    let cancel = CancellationToken::new();
    let progress: ProgressFn = Arc::new({
        let cancel = cancel.clone();
        move |progress| {
            if progress.phase == Phase::Writing {
                cancel.cancel();
            }
        }
    });
    let (_, error) = fails_cleanly(|_| {}, cancel, progress).await;
    assert!(matches!(error, BackupError::Cancelled), "{error:?}");
}

#[tokio::test]
async fn too_little_space_is_refused_before_writing() {
    let (_, error) = fails_cleanly(
        |backups| caskade_core::lock(&backups.faults).available = Some(1024),
        CancellationToken::new(),
        quiet(),
    )
    .await;
    assert!(
        matches!(
            error,
            BackupError::NoSpace {
                available: 1024,
                ..
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn the_next_backup_clears_what_a_crash_left() {
    let (profile, _) = fails_cleanly(
        |backups| caskade_core::lock(&backups.faults).crash = true,
        CancellationToken::new(),
        quiet(),
    )
    .await;
    let left = listing(&profile.destination);
    assert_eq!(
        left.iter()
            .filter(|n| n.starts_with(".caskade-backup-"))
            .count(),
        2
    );
    caskade_core::lock(&profile.backups.faults).crash = false;
    profile
        .backups
        .export(options(&profile, &[]), quiet(), CancellationToken::new())
        .await
        .unwrap();
    let after = listing(&profile.destination);
    assert_eq!(after.len(), 2, "{after:?}");
    assert!(after.iter().all(|n| n.starts_with(PREFIX)));
}

#[tokio::test]
async fn a_missing_destination_or_a_running_backup_is_refused() {
    let profile = profile().await;
    let mut missing = options(&profile, &[]);
    missing.destination = profile.destination.join("unplugged");
    let error = profile
        .backups
        .export(missing, quiet(), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(
        matches!(error, BackupError::DestinationMissing(_)),
        "{error:?}"
    );
    let _held = profile.backups.running.try_lock().unwrap();
    let error = profile
        .backups
        .clone()
        .export(options(&profile, &[]), quiet(), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, BackupError::Busy), "{error:?}");
}

#[tokio::test]
async fn the_default_destination_is_created() {
    let profile = profile().await;
    let mut options = options(&profile, &[]);
    options.destination = profile.backups.default_destination();
    let exported = profile
        .backups
        .export(options, quiet(), CancellationToken::new())
        .await
        .unwrap();
    assert!(
        exported
            .path
            .starts_with(profile.paths.data.join("backups"))
    );
}

/// Rewrites a backup's zip through `change`, which gets each entry's name
/// and bytes and returns what to write instead (`None` drops it).
fn rewrite(
    from: &Path,
    to: &Path,
    change: impl Fn(&str, Vec<u8>) -> Option<(String, Vec<u8>)>,
    extra: &[(&str, &[u8])],
) {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(from).unwrap()).unwrap();
    let mut out = zip::ZipWriter::new(std::fs::File::create(to).unwrap());
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if let Some((name, bytes)) = change(&name, bytes) {
            out.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(&bytes).unwrap();
        }
    }
    for (name, bytes) in extra {
        out.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(bytes).unwrap();
    }
    out.finish().unwrap();
}

#[tokio::test]
async fn damaged_unsafe_and_newer_archives_are_refused() {
    let profile = profile().await;
    fill(&profile).await;
    let good = profile
        .backups
        .export(
            options(&profile, &[Component::Plugins]),
            quiet(),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .path;
    let temp = tempfile::tempdir().unwrap();
    let bad = temp.path().join("bad.zip");
    let check = || verify(&bad, &CancellationToken::new(), &|_| {});
    fn manifest_with(
        edit: impl Fn(&mut serde_json::Value),
    ) -> impl Fn(&str, Vec<u8>) -> Option<(String, Vec<u8>)> {
        move |name: &str, bytes: Vec<u8>| {
            if name == MANIFEST {
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                edit(&mut value);
                Some((name.to_owned(), serde_json::to_vec(&value).unwrap()))
            } else {
                Some((name.to_owned(), bytes))
            }
        }
    }

    // A changed byte.
    rewrite(
        &good,
        &bad,
        |name, mut bytes| {
            if name == "plugins/fixture.plugin/plugin.json" {
                bytes[0] ^= 1;
            }
            Some((name.to_owned(), bytes))
        },
        &[],
    );
    assert!(matches!(check(), Err(BackupError::Damaged(_))));
    // An entry the manifest doesn't list.
    rewrite(
        &good,
        &bad,
        |n, b| Some((n.to_owned(), b)),
        &[("plugins/x/evil.js", b"1")],
    );
    assert!(matches!(check(), Err(BackupError::Damaged(_))));
    // A listed file that isn't there.
    rewrite(
        &good,
        &bad,
        |name, bytes| {
            (name != "plugins/fixture.plugin/ui/index.js").then(|| (name.to_owned(), bytes))
        },
        &[],
    );
    assert!(matches!(check(), Err(BackupError::Damaged(_))));
    // Paths out of their folder.
    for path in [
        "plugins/../../etc/passwd",
        "/plugins/a",
        "plugins/a\\b",
        "themes/a",
        "plugins",
    ] {
        rewrite(
            &good,
            &bad,
            manifest_with(|value: &mut serde_json::Value| {
                value["files"][1]["path"] = path.into();
            }),
            &[],
        );
        assert!(matches!(check(), Err(BackupError::Damaged(_))), "{path}");
    }
    // A newer format.
    rewrite(
        &good,
        &bad,
        manifest_with(|value: &mut serde_json::Value| value["format"] = (FORMAT + 1).into()),
        &[],
    );
    assert!(matches!(check(), Err(BackupError::Newer)));
    // No library.
    rewrite(
        &good,
        &bad,
        manifest_with(|value: &mut serde_json::Value| {
            value["files"]
                .as_array_mut()
                .unwrap()
                .retain(|f| f["path"] != DATABASE);
        }),
        &[],
    );
    assert!(matches!(check(), Err(BackupError::Damaged(_))));
    // Not a zip, and not a backup.
    std::fs::write(&bad, b"not a zip").unwrap();
    assert!(matches!(check(), Err(BackupError::Damaged(_))));
    rewrite(
        &good,
        &bad,
        |name, b| (name != MANIFEST).then(|| (name.to_owned(), b)),
        &[],
    );
    assert!(matches!(check(), Err(BackupError::Damaged(_))));
}

#[tokio::test]
async fn every_excluded_table_exists() {
    let profile = profile().await;
    let names: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table'")
            .fetch_all(&mut live(&profile.paths).await)
            .await
            .unwrap();
    for (table, _) in policy::EXCLUDED_TABLES {
        assert!(names.iter().any(|n| n == table), "{table}");
    }
}

#[test]
fn stamps_are_utc_and_sort() {
    assert_eq!(export::stamp(0), "19700101-000000");
    assert_eq!(export::stamp(1_793_316_600_000), "20261029-233000");
    assert_eq!(export::stamp(951_782_400_000), "20000229-000000");
}
