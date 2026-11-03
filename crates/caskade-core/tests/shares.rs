//! One-off shares between cores on loopback with no relay transport (#246,
//! #249): a device that isn't paired receives a shared game from its link
//! and it installs as a game of its own, ready to play; Stop sharing fails
//! the link at once, also part-way through; an expired link says so; only
//! the game's folder is exposed; and recipients serve each other within the
//! share until it stops.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::{LibraryStore, ShareRow};
use caskade_core::devices::{Devices, MemoryKeyStore, Reach};
use caskade_core::install::{InstallOptions, InstallService, JobSnapshot, JobStatus};
use caskade_core::model::{
    Game, GameEntry, GameRuntime, InstallLocation, InstallState, LaunchSpec, StorageRoot,
};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::share::{ShareExpiry, ShareReach, ShareState, Shares};
use caskade_core::transfer::Transfers;
use caskade_installer::manifest::RootManifest;
use caskade_installer::pipeline::input;

struct Core {
    store: LibraryStore,
    devices: Devices,
    transfers: Transfers,
    install: InstallService,
    shares: Shares,
    games: PathBuf,
    dir: tempfile::TempDir,
}

/// Waits up to `seconds` for `check`.
async fn within<F, Fut>(what: &str, seconds: u64, check: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !check().await {
        assert!(Instant::now() < deadline, "{what} within {seconds} s");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn core(name: &str) -> Core {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(&dir.path().join("app")))
        .unwrap();
    // Loopback only, and no relay: whatever arrives came directly.
    let devices = Devices::with_reach(
        store.clone(),
        accounts,
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    devices.set_settings(settings.clone());
    let cache = dir.path().join("cache");
    let transfers = Transfers::new(store.clone(), devices.clone(), &cache)
        .await
        .unwrap();
    let games = dir.path().join("Games");
    std::fs::create_dir_all(&games).unwrap();
    let identity = RootManifest::new(format!("{}-root", name.to_lowercase())).unwrap();
    identity.write(&games).unwrap();
    let mut root = StorageRoot::from_manifest(games.canonicalize().unwrap(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let install = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 2,
            aliases: None,
        },
    )
    .with_settings(settings.clone())
    .with_host(input::host(Some("linux")).unwrap());
    install.set_peer_fetcher(Arc::new(transfers.clone()));
    let shares = Shares::new(
        store.clone(),
        devices.clone(),
        transfers.clone(),
        install.clone(),
        &cache,
    );
    devices.start();
    let core = Core {
        store,
        devices,
        transfers,
        install,
        shares,
        games: games.canonicalize().unwrap(),
        dir,
    };
    within("the endpoint starts", 20, || async {
        core.devices.status().await.this_device.listening
    })
    .await;
    core.devices.rename(name).await.unwrap();
    core
}

fn fixture_exe() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../caskade-installer/tests/fixtures/bin/fixture-game-windows-x86_64.exe")
}

/// Celeste, a Windows game added by hand on `core`, `big` bytes of it in
/// one file, with a link inside its folder to a file outside it.
async fn add_celeste(core: &Core, big: usize) -> (String, PathBuf) {
    let folder = core.dir.path().join("My Games").join("Celeste");
    std::fs::create_dir_all(folder.join("Content/Levels")).unwrap();
    std::fs::copy(fixture_exe(), folder.join("Celeste.exe")).unwrap();
    std::fs::write(folder.join("Content/Levels/1.bin"), vec![3u8; 50_000]).unwrap();
    let data: Vec<u8> = (0..big).map(|i| (i % 253) as u8).collect();
    std::fs::write(folder.join("Content/Celeste.pak"), data).unwrap();
    let secret = core.dir.path().join("My Games").join("passwords.txt");
    std::fs::write(&secret, b"not for sharing").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&secret, folder.join("Content/notes.txt")).unwrap();
    let game = core.store.upsert_game(&Game::new("Celeste")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, "manual".to_owned().into(), "celeste");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: folder.clone(),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: folder.join("Celeste.exe"),
        args: Vec::new(),
        cwd: None,
        env: Default::default(),
    });
    entry.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    let entry = core.store.upsert_entry(&entry).await.unwrap();
    (entry.id, folder)
}

async fn finished(service: &InstallService, id: &str) -> JobSnapshot {
    for _ in 0..1200 {
        let job = service.job(id).unwrap();
        if job.status.is_finished() || job.status == JobStatus::NeedsUser {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job {id} did not finish: {:?}", service.job(id));
}

async fn id(core: &Core) -> String {
    core.devices.status().await.this_device.id.unwrap()
}

/// Shares `entry` for a week once its files are read, and its link.
async fn shared(core: &Core, entry: &str) -> (String, String) {
    let share = core.shares.share(entry, ShareExpiry::Week).await.unwrap();
    within("the share's files are read", 30, || async {
        core.shares
            .list()
            .await
            .unwrap()
            .iter()
            .any(|record| record.id == share.id && record.state == ShareState::Ready)
    })
    .await;
    let link = core.shares.link(&share.id).await.unwrap();
    (share.id, link)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shared_game_arrives_from_its_link_and_is_ready_to_play() {
    let desk = core("Desk").await;
    let friend = core("Friend's PC").await;
    let (entry, folder) = add_celeste(&desk, 6 * 1024 * 1024).await;
    let (share, link) = shared(&desk, &entry).await;
    assert!(link.starts_with("caskade://receive?t="));

    // Not paired: the link alone opens it.
    assert!(friend.devices.status().await.devices.is_empty());
    let info = friend.shares.inspect(&link).await.unwrap();
    assert_eq!(info.title, "Celeste");
    assert_eq!(info.sharer, "Desk");
    assert_eq!(info.sharer_id, id(&desk).await);
    assert!(info.windows);
    assert_eq!(info.reach, ShareReach::Direct);
    assert!(info.size.unwrap() > 6 * 1024 * 1024);
    assert!(info.expires_at_ms.is_some());
    // Looking fetched nothing.
    assert!(friend.install.jobs().is_empty());

    let job = friend.shares.receive(&link, None).await.unwrap();
    let done = finished(&friend.install, &job.id).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");

    // A game of its own here, a managed install that starts its program.
    let game = done.game_id.expect("a game");
    let entries = friend.store.entries_for_game(&game).await.unwrap();
    let copy = entries
        .iter()
        .find(|entry| entry.source_id.as_str() == "caskade-storage")
        .expect("a managed release");
    let Some(InstallLocation::Managed { rel_dir, .. }) = &copy.location else {
        panic!("not managed: {:?}", copy.location)
    };
    assert!(matches!(copy.runtime, GameRuntime::Windows { .. }));
    let Some(LaunchSpec::Executable { path, .. }) = &copy.launch else {
        panic!("no program: {:?}", copy.launch)
    };
    assert!(path.ends_with("Celeste.exe"), "{path:?}");
    let installed = friend.games.join(rel_dir);
    assert!(installed.join(path).is_file() || path.is_file());
    for file in ["Celeste.exe", "Content/Levels/1.bin", "Content/Celeste.pak"] {
        assert_eq!(
            std::fs::read(installed.join(file)).unwrap(),
            std::fs::read(folder.join(file)).unwrap(),
            "{file}"
        );
    }
    // Nothing outside the game's folder came with it.
    assert!(!installed.join("Content/notes.txt").exists());
    let walked = walk(&installed);
    assert!(
        walked
            .iter()
            .all(|file| std::fs::read(file).unwrap() != b"not for sharing"),
        "{walked:?}"
    );
    let staging = friend.dir.path().join("cache/shares/incoming");
    assert!(!staging.exists() || std::fs::read_dir(&staging).unwrap().next().is_none());

    // The Desk lists who received it, and how far they got.
    within("the Desk sees the friend finish", 10, || async {
        let records = desk.shares.list().await.unwrap();
        let record = records.iter().find(|record| record.id == share).unwrap();
        record.recipients.len() == 1
            && record.recipients[0].name == "Friend's PC"
            && record.recipients[0].finished
    })
    .await;
    // And once it's here, the friend no longer serves it by default.
    within("the friend stops serving", 10, || async {
        friend.shares.received().iter().all(|share| !share.serving)
    })
    .await;
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_share_fails_its_link_at_once_also_part_way() {
    let desk = core("Desk").await;
    let friend = core("Friend's PC").await;
    let (entry, _) = add_celeste(&desk, 96 * 1024 * 1024).await;
    let (share, link) = shared(&desk, &entry).await;
    let job = friend.shares.receive(&link, None).await.unwrap();
    within("some of it arrives", 60, || async {
        friend
            .install
            .job(&job.id)
            .and_then(|job| job.progress)
            .is_some_and(|progress| progress.bytes > 1024 * 1024)
    })
    .await;

    let stopped = Instant::now();
    desk.shares.stop(&share).await.unwrap();
    let done = finished(&friend.install, &job.id).await;
    assert_eq!(done.status, JobStatus::Failed, "{done:?}");
    assert!(
        stopped.elapsed() < Duration::from_secs(10),
        "took {:?}",
        stopped.elapsed()
    );
    let reason = done.error.unwrap_or_default();
    assert!(reason.contains("stopped"), "{reason}");
    let progress = done.progress.map_or(0, |progress| progress.bytes);
    assert!(progress < 96 * 1024 * 1024, "it all arrived anyway");

    // The link is refused from now on, with the reason.
    let error = friend.shares.inspect(&link).await.unwrap_err().to_string();
    assert!(error.contains("doesn't work any more"), "{error}");
    let records = desk.shares.list().await.unwrap();
    assert_eq!(records[0].state, ShareState::Stopped);
    // Forgotten once stopped.
    desk.shares.forget(&share).await.unwrap();
    assert!(desk.shares.list().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_link_fails_with_a_clear_message() {
    let desk = core("Desk").await;
    let friend = core("Friend's PC").await;
    let (entry, _) = add_celeste(&desk, 1024).await;
    let (share, link) = shared(&desk, &entry).await;

    // The link says it expired: no need to ask the sharer.
    let row = desk.store.share(&share).await.unwrap().unwrap();
    let addr = desk
        .devices
        .link_addr(Duration::from_secs(1))
        .await
        .unwrap();
    let old = caskade_sync::share::ShareLink::new(
        addr,
        row.grant.parse().unwrap(),
        Some(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
    );
    let error = friend
        .shares
        .inspect(&old.to_url())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("expired on 2023-11-14"), "{error}");

    // And the sharer holds to its own expiry, whatever a link says.
    let soon = ShareRow {
        id: "soon".into(),
        expires_at_ms: Some(now_ms() + 1_500),
        grant: caskade_sync::Grant::generate().to_hex(),
        ..row
    };
    desk.store.insert_share(&soon).await.unwrap();
    desk.shares.resume().await.unwrap();
    let unexpiring = caskade_sync::share::ShareLink::new(
        desk.devices
            .link_addr(Duration::from_secs(1))
            .await
            .unwrap(),
        soon.grant.parse().unwrap(),
        None,
    );
    assert!(friend.shares.inspect(&unexpiring.to_url()).await.is_ok());
    tokio::time::sleep(Duration::from_millis(1_600)).await;
    let error = friend
        .shares
        .inspect(&unexpiring.to_url())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("doesn't work any more"), "{error}");
    within("the Desk shows it expired", 5, || async {
        desk.shares
            .list()
            .await
            .unwrap()
            .iter()
            .any(|record| record.id == "soon" && record.state == ShareState::Expired)
    })
    .await;
    // The first share still works.
    assert!(friend.shares.inspect(&link).await.is_ok());
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tokio::test(flavor = "multi_thread")]
async fn a_share_exposes_its_game_and_nothing_else() {
    let desk = core("Desk").await;
    let friend = core("Friend's PC").await;
    let (entry, _) = add_celeste(&desk, 1024 * 1024).await;
    let (_, link) = shared(&desk, &entry).await;
    // Another folder offered on the Desk, not shared with the friend.
    let other = desk.dir.path().join("Private");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("diary.txt"), b"dear diary").unwrap();
    let private = desk
        .transfers
        .offer_folder(&other, "test/private", |_, _| {})
        .await
        .unwrap();

    let job = friend.shares.receive(&link, None).await.unwrap();
    assert_eq!(
        finished(&friend.install, &job.id).await.status,
        JobStatus::Completed
    );
    // With the share's grant used, the friend still can't fetch anything
    // else the Desk has.
    let desk_id: caskade_sync::EndpointId = id(&desk).await.parse().unwrap();
    let addrs: Vec<std::net::SocketAddr> = desk
        .devices
        .link_addr(Duration::ZERO)
        .await
        .unwrap()
        .ip_addrs()
        .copied()
        .collect();
    let refused = tokio::time::timeout(
        Duration::from_secs(20),
        friend
            .transfers
            .fetch_direct(desk_id, &addrs, &private.root, "test/in", |_| {}),
    )
    .await
    .expect("a refused fetch ends");
    assert!(refused.is_err());
    // The share's grant opens a share session, nothing else.
    let row = desk.store.shares().await.unwrap().remove(0);
    let grant: caskade_sync::Grant = row.grant.parse().unwrap();
    let addr = desk.devices.link_addr(Duration::ZERO).await.unwrap();
    for purpose in [
        caskade_sync::Purpose::Sync,
        caskade_sync::Purpose::Copy,
        caskade_sync::Purpose::Pair,
    ] {
        assert!(
            friend
                .devices
                .connect_to(addr.clone(), purpose, Some(grant))
                .await
                .is_err(),
            "{purpose:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_third_recipient_receives_from_the_sharer_and_the_first_two_until_it_stops() {
    let desk = core("Desk").await;
    let ann = core("Ann").await;
    let bo = core("Bo").await;
    let cy = core("Cy").await;
    // Thirty files, so the third's fetch has requests to spread.
    let (entry, folder) = add_celeste(&desk, 1024).await;
    for n in 0..30 {
        let bytes: Vec<u8> = (0..1024 * 1024).map(|i| ((i + n * 7) % 251) as u8).collect();
        std::fs::write(folder.join(format!("Content/{n:02}.pak")), bytes).unwrap();
    }
    let (share, link) = shared(&desk, &entry).await;

    // Ann and Bo receive it from the Desk, and keep helping after.
    for recipient in [&ann, &bo] {
        let job = recipient.shares.receive(&link, None).await.unwrap();
        recipient.shares.keep_helping(&job.id, true).unwrap();
        assert_eq!(
            finished(&recipient.install, &job.id).await.status,
            JobStatus::Completed
        );
        within("it serves the share", 10, || async {
            recipient.shares.received().iter().any(|share| share.serving)
        })
        .await;
    }
    within("the Desk knows both serve", 10, || async {
        let records = desk.shares.list().await.unwrap();
        records[0].recipients.iter().filter(|r| r.serving).count() == 2
    })
    .await;

    // Cy receives from all three at once. The Desk forgets what it sent Cy
    // once Cy leaves, so it's counted while Cy receives.
    let root = desk.store.share(&share).await.unwrap().unwrap().root.unwrap();
    let cy_id: caskade_sync::EndpointId = id(&cy).await.parse().unwrap();
    let from_desk = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counting = {
        let (transfers, root, from_desk) = (desk.transfers.clone(), root.clone(), from_desk.clone());
        tokio::spawn(async move {
            loop {
                from_desk.fetch_max(
                    transfers.sent(cy_id, &root),
                    std::sync::atomic::Ordering::Relaxed,
                );
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
    };
    let job = cy.shares.receive(&link, None).await.unwrap();
    assert_eq!(
        finished(&cy.install, &job.id).await.status,
        JobStatus::Completed
    );
    counting.abort();
    // Each sent whole files, not just the collection's list of them.
    let file = 1024 * 1024;
    let from_desk = from_desk.load(std::sync::atomic::Ordering::Relaxed);
    assert!(from_desk >= file, "the Desk sent Cy {from_desk} bytes");
    for (name, provider) in [("Ann", &ann), ("Bo", &bo)] {
        let sent = provider.transfers.sent(cy_id, &root);
        assert!(sent >= file, "{name} sent Cy {sent} bytes");
    }

    // Stopped on the Desk: Ann and Bo stop serving it too.
    desk.shares.stop(&share).await.unwrap();
    for (name, recipient) in [("Ann", &ann), ("Bo", &bo)] {
        within(&format!("{name} stops serving"), 10, || async {
            recipient.shares.received().iter().all(|share| !share.serving)
        })
        .await;
    }
}
