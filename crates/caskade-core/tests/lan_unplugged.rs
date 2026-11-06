//! Two devices on a network of their own with no way to the internet (#10,
//! #15, #20). Each test here is one device: `scripts/lan-unplugged.sh` starts
//! a process per device, each in a network namespace of its own on a bridge
//! whose router has no uplink, with the settings the app uses (local
//! discovery, n0's lookup and relays, neither of which can answer). Unlike the
//! loopback tests, they find each other over mDNS on a real interface.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use caskade_core::devices::{DeviceLink, Devices, PairingState, Reach};
use caskade_core::install::{JobStatus, PeerCopyTarget};
use caskade_core::model::{
    CollectionKind, Game, GameEdit, InstallLocation, PlaySession, SaveCollection, TextEdit,
};
use caskade_core::sync::change::Entity;

use crate::cores::{core_with, within};
use crate::game_copy::{add_celeste, finished};

/// This process's device, and notes passed to the other device's process in
/// place of the two people comparing screens.
pub struct Peer {
    pub me: String,
    pub other: String,
    dir: PathBuf,
}

impl Peer {
    pub fn from_env() -> Self {
        let var = |name| std::env::var(name).expect("run by scripts/lan-unplugged.sh");
        Self {
            me: var("CASKADE_LAN_ME"),
            other: var("CASKADE_LAN_OTHER"),
            dir: var("CASKADE_LAN_NOTES").into(),
        }
    }

    pub fn say(&self, step: &str, value: &str) {
        let note = self.dir.join(format!("{}-{step}", self.me));
        let partial = note.with_extension("partial");
        std::fs::write(&partial, value).unwrap();
        std::fs::rename(partial, note).unwrap();
    }

    pub async fn heard(&self, step: &str) -> String {
        self.heard_within(step, 120).await
    }

    async fn heard_within(&self, step: &str, seconds: u64) -> String {
        let note = self.dir.join(format!("{}-{step}", self.other));
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            if let Ok(value) = std::fs::read_to_string(&note) {
                return value;
            }
            assert!(Instant::now() < deadline, "{} said {step}", self.other);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// Finds the other device nearby and pairs by matching codes; `starts` says
/// which side presses Pair. Answers the other device's id.
pub async fn pair_by_code(devices: &Devices, peer: &Peer, starts: bool) -> String {
    within("the other device shows nearby", 60, || async {
        let status = devices.status().await;
        status.nearby.iter().any(|d| d.name == peer.other)
    })
    .await;
    let status = devices.status().await;
    let nearby = status.nearby.iter().find(|d| d.name == peer.other).unwrap();
    assert!(!nearby.addresses.is_empty(), "{nearby:?}");
    let id = nearby.id.clone();
    if starts {
        let pairing = devices.pair_nearby(&id).await.unwrap();
        assert_eq!(pairing.state, PairingState::Waiting);
        peer.say("code", &pairing.code);
    } else {
        within("the request arrives", 60, || async {
            devices.status().await.pairing.is_some()
        })
        .await;
        let pairing = devices.status().await.pairing.unwrap();
        assert!(pairing.incoming);
        assert_eq!(pairing.code, peer.heard("code").await, "the codes match");
    }
    devices.confirm_pairing(&id).await.unwrap();
    within("both are paired", 60, || async {
        devices.status().await.devices.iter().any(|d| d.id == id)
    })
    .await;
    id
}

/// Both ways: a pairing by code, an edit, a collection and a play session,
/// then a game copied from the Desk.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "one device of two, on a network of its own: scripts/lan-unplugged.sh"]
async fn two_devices_pair_sync_and_copy() {
    let peer = Peer::from_env();
    let is_desk = peer.me == "Desk";
    let device = core_with(&peer.me, Reach::network(false, None)).await;
    let other = pair_by_code(&device.devices, &peer, is_desk).await;
    let _sync = device.sync();
    within("a first sync", 60, || async {
        let status = device.devices.status().await;
        status
            .devices
            .iter()
            .any(|d| d.id == other && d.last_sync_ms.is_some())
    })
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let link = device.devices.status().await.devices[0].link;
    assert_eq!(link, DeviceLink::Lan);

    // What each side changes.
    let snapshot = device.store.sync_snapshot().await.unwrap();
    let game = snapshot.games.into_iter().next().unwrap();
    if is_desk {
        let edit = GameEdit {
            description: Some(TextEdit::Set {
                value: "Escape the Underworld".to_owned(),
            }),
            ..GameEdit::default()
        };
        device.store.edit_game(&game, &edit, None).await.unwrap();
    } else {
        device.store.set_game_favorite(&game, true).await.unwrap();
    }
    let collection = device
        .store
        .save_collection(
            &device.account,
            &SaveCollection {
                id: None,
                name: format!("{} picks", peer.me),
                description: String::new(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
    let entry = device.store.sync_game_entry_stores(&game).await.unwrap()[0]
        .0
        .clone();
    let mut session = PlaySession::new(&game, &entry, 1_000_000);
    session.ended_at_ms = Some(1_600_000);
    session.duration_ms = 600_000;
    let session = device.store.upsert_session(&session).await.unwrap();
    peer.say("changes", &format!("{}\n{}", collection.id, session.id));

    // What arrives from the other side.
    let changes = peer.heard("changes").await;
    let (theirs, their_session) = changes.split_once('\n').unwrap();
    within("the other side's changes arrive", 60, || async {
        let row = device.store.game(&game).await.unwrap().unwrap();
        let edited = if is_desk {
            row.favorite
        } else {
            row.description == "Escape the Underworld"
        };
        let snapshot = device.store.sync_snapshot().await.unwrap();
        let sessions = device.store.sessions_for_game(&game, 10).await.unwrap();
        edited
            && snapshot
                .values
                .contains_key(&(Entity::Collection, theirs.to_owned(), String::new()))
            && sessions.iter().any(|s| s.id == their_session)
    })
    .await;

    // A game copied over the same network.
    if is_desk {
        let (_, entry, _) = add_celeste(&device).await;
        peer.say("celeste", &entry);
        peer.heard("copied").await;
    } else {
        let entry_id = peer.heard("celeste").await;
        let game = device
            .store
            .upsert_game(&Game::new("Celeste"))
            .await
            .unwrap();
        let job = device
            .install
            .copy_from_device(
                PeerCopyTarget {
                    device_id: other.clone(),
                    device_name: peer.other.clone(),
                    entry_id,
                    game_id: game.id.clone(),
                    title: "Celeste".into(),
                },
                None,
            )
            .await
            .unwrap();
        let done = finished(&device.install, &job.id).await;
        assert_eq!(done.status, JobStatus::Completed, "{done:?}");
        let entries = device.store.entries_for_game(&game.id).await.unwrap();
        let copy = entries
            .iter()
            .find(|entry| entry.source_id.as_str() == "caskade-storage")
            .expect("a managed release");
        let Some(InstallLocation::Managed { rel_dir, .. }) = &copy.location else {
            panic!("not managed: {:?}", copy.location)
        };
        let pak = std::fs::read(device.games.join(rel_dir).join("Content/Celeste.pak")).unwrap();
        assert_eq!(pak.len(), 6 * 1024 * 1024);
        assert!(pak.iter().enumerate().all(|(i, b)| *b == (i % 253) as u8));
        peer.say("copied", "yes");
    }
    eprintln!("{}: paired, synced both ways and copied", peer.me);
}

/// N1 without two PCs: a large game copied from one device's process to the
/// other's over a virtual network, timed as Copy to this computer's job runs.
/// `scripts/lan-unplugged.sh --speed`, in release. `CASKADE_COPY_MIB` sizes
/// it: a big archive and a thousand small files, as a game has.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "one device of two, on a network of its own: scripts/lan-unplugged.sh --speed"]
async fn copy_speed() {
    let mebibyte = 1024 * 1024;
    let total: usize = std::env::var("CASKADE_COPY_MIB")
        .ok()
        .and_then(|size| size.parse().ok())
        .unwrap_or(2048);
    let peer = Peer::from_env();
    let is_desk = peer.me == "Desk";
    let device = core_with(&peer.me, Reach::network(false, None)).await;
    let other = pair_by_code(&device.devices, &peer, is_desk).await;
    let _sync = device.sync();
    within("a first sync", 60, || async {
        let status = device.devices.status().await;
        status
            .devices
            .iter()
            .any(|d| d.id == other && d.last_sync_ms.is_some())
    })
    .await;
    let link = device.devices.status().await.devices[0].link;
    assert_eq!(link, DeviceLink::Lan);

    if is_desk {
        let (_, entry, folder) = add_celeste(&device).await;
        let pack = folder.join("Content/Data.pak");
        let mut file = std::fs::File::create(pack).unwrap();
        for block in 0..total.saturating_sub(100) {
            let bytes: Vec<u8> = (0..mebibyte).map(|i| ((i ^ block) % 251) as u8).collect();
            std::io::Write::write_all(&mut file, &bytes).unwrap();
        }
        for index in 0..1000 {
            let name = folder.join(format!("Content/Levels/{index}.bin"));
            std::fs::write(name, vec![(index % 251) as u8; 100 * 1024]).unwrap();
        }
        peer.say("game", &entry);
        peer.heard_within("copied", 1800).await;
    } else {
        let entry_id = peer.heard_within("game", 300).await;
        let game = device
            .store
            .upsert_game(&Game::new("Celeste"))
            .await
            .unwrap();
        let started = Instant::now();
        let job = device
            .install
            .copy_from_device(
                PeerCopyTarget {
                    device_id: other.clone(),
                    device_name: peer.other.clone(),
                    entry_id,
                    game_id: game.id.clone(),
                    title: "Celeste".into(),
                },
                None,
            )
            .await
            .unwrap();
        // Three parts: the sender reading and hashing, the transfer, and
        // moving what arrived into the install.
        let (mut receiving, mut placing) = (None, None);
        let done = loop {
            let job = device.install.job(&job.id).unwrap();
            if job.status.is_finished() || job.status == JobStatus::NeedsUser {
                break job;
            }
            let from = job
                .progress
                .as_ref()
                .is_some_and(|progress| progress.message.starts_with("From"));
            if receiving.is_none() && from {
                receiving = Some(Instant::now());
            }
            if receiving.is_some() && placing.is_none() && !from {
                placing = Some(Instant::now());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let finished_at = Instant::now();
        assert_eq!(done.status, JobStatus::Completed, "{done:?}");
        let entries = device.store.entries_for_game(&game.id).await.unwrap();
        let copy = entries
            .iter()
            .find(|entry| entry.source_id.as_str() == "caskade-storage")
            .expect("a managed release");
        let Some(InstallLocation::Managed { rel_dir, .. }) = &copy.location else {
            panic!("not managed: {:?}", copy.location)
        };
        let size = std::fs::metadata(device.games.join(rel_dir).join("Content/Data.pak"))
            .unwrap()
            .len();
        let bytes = size as f64 + 106.0 * mebibyte as f64;
        let receiving = receiving.expect("the transfer was seen");
        let placing = placing.unwrap_or(finished_at);
        let transfer = placing.duration_since(receiving).as_secs_f64();
        eprintln!(
            "copy_speed: {:.0} MiB; sender preparing {:.1} s; transfer {transfer:.1} s = {:.0} MB/s; placing {:.1} s; whole job {:.1} s = {:.0} MB/s",
            bytes / mebibyte as f64,
            receiving.duration_since(started).as_secs_f64(),
            bytes / transfer / 1e6,
            finished_at.duration_since(placing).as_secs_f64(),
            finished_at.duration_since(started).as_secs_f64(),
            bytes / finished_at.duration_since(started).as_secs_f64() / 1e6,
        );
        peer.say("copied", "yes");
    }
}
