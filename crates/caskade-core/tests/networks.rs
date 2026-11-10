//! The internet `scripts/two-networks.sh` builds in network namespaces, as
//! its scenarios see it: the relay on it, the meter of what passes through
//! that relay, and a game to copy across and play. The scenarios are in
//! `two_networks`, here and in caskade-share's tests, which include this file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use caskade_core::devices::Reach;
use caskade_core::install::JobSnapshot;
use caskade_core::launch::LaunchService;
use caskade_core::model::{
    Game, GameEntry, GameRuntime, InstallLocation, InstallState, LaunchSpec,
};
use caskade_sync::Relays;
use caskade_sync::testing::relays_at;

use crate::cores::{Core, within};

pub const MIB: usize = 1024 * 1024;
/// More than a session's frames and a relay's keepalives come to in a run,
/// and a sixtieth of the game: a game that passed through the relay shows.
pub const RELAY_ALLOWANCE: u64 = MIB as u64;

pub fn relays() -> Relays {
    relays_at(&std::env::var("CASKADE_NET_RELAY").expect("run by scripts/two-networks.sh"))
}

/// The app's reach, with a relay on this internet for n0's and no lookup on
/// n0's DNS: links carry the relay and the addresses.
pub fn reach() -> Reach {
    Reach {
        relays: relays(),
        ip: caskade_sync::Ip::Any { port: None },
        internet_lookup: false,
        lan: Some(caskade_sync::lan::SERVICE.to_owned()),
    }
}

pub fn expected() -> String {
    std::env::var("CASKADE_NET_EXPECT").unwrap_or_default()
}

/// How long a scenario gives something that has to happen. Short in the runs
/// meant to fail, so they don't take a timeout each.
pub fn patience() -> Duration {
    let seconds = std::env::var("CASKADE_NET_PATIENCE").unwrap_or_default();
    Duration::from_secs(seconds.parse().unwrap_or(60))
}

/// Bytes through the relay so far, both ways, as iptables in the internet's
/// namespace counts them, so iroh's word isn't taken for it.
pub fn relay_bytes() -> u64 {
    let meter = Path::new(&std::env::var("CASKADE_LAN_NOTES").unwrap()).join("relay-bytes");
    std::fs::read_to_string(meter)
        .expect("the script keeps the meter")
        .trim()
        .parse()
        .expect("a number")
}

pub async fn online<X>(device: &Core<X>) {
    within("a relay answers", 30, || async {
        device.devices.status().await.this_device.internet == Some(true)
    })
    .await;
}

/// A Linux game: a program that leaves a mark beside itself when it runs,
/// and `size` bytes of data.
pub async fn add_game<X>(device: &Core<X>, size: usize) -> (String, PathBuf) {
    use std::os::unix::fs::PermissionsExt as _;
    let folder = device.dir.path().join("My Games").join("Celeste");
    std::fs::create_dir_all(folder.join("Content")).unwrap();
    let program = folder.join("celeste.sh");
    std::fs::write(
        &program,
        "#!/bin/sh\necho played > \"$(dirname \"$0\")/played\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let data: Vec<u8> = (0..size).map(|i| (i % 253) as u8).collect();
    std::fs::write(folder.join("Content/Celeste.pak"), data).unwrap();
    let game = device
        .store
        .upsert_game(&Game::new("Celeste"))
        .await
        .unwrap();
    let mut entry = GameEntry::new(&game.id, "manual".to_owned().into(), "celeste");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: folder.clone(),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: program,
        args: Vec::new(),
        cwd: None,
        env: Default::default(),
    });
    entry.runtime = GameRuntime::Native;
    let entry = device.store.upsert_entry(&entry).await.unwrap();
    (entry.id, folder)
}

/// Follows `job` until it ends or `wait` passes, logging what it says.
pub async fn follow<X>(device: &Core<X>, job: &str, wait: Duration) -> JobSnapshot {
    let started = Instant::now();
    let mut last = String::new();
    loop {
        let snapshot = device.install.job(job).unwrap();
        let progress = snapshot.progress.as_ref();
        let message = progress.map_or("", |progress| progress.message.as_str());
        let now = format!("{:?}: {message}", snapshot.status);
        if now != last {
            eprintln!("{:.1?} {now}", started.elapsed());
            last = now;
        }
        if snapshot.status.is_finished() || started.elapsed() > wait {
            return snapshot;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Plays what `done` installed and checks the program really ran.
pub async fn plays<X>(device: &Core<X>, done: &JobSnapshot) {
    let game = done.game_id.as_ref().expect("a game");
    let entries = device.store.entries_for_game(game).await.unwrap();
    let copy = entries
        .iter()
        .find(|entry| entry.source_id.as_str() == "caskade-storage")
        .expect("a managed release");
    let Some(InstallLocation::Managed { rel_dir, .. }) = &copy.location else {
        panic!("not managed: {:?}", copy.location)
    };
    let installed = device.games.join(rel_dir);
    let pak = std::fs::read(installed.join("Content/Celeste.pak")).unwrap();
    assert!(pak.iter().enumerate().all(|(i, b)| *b == (i % 253) as u8));
    let launches = LaunchService::new(device.store.clone());
    let session = launches
        .launch(&copy.id)
        .await
        .expect("it starts")
        .wait()
        .await
        .expect("it ends");
    assert!(session.ended_at_ms.is_some(), "{session:?}");
    let played = std::fs::read_to_string(installed.join("played")).expect("the program ran");
    assert_eq!(played.trim(), "played");
    eprintln!("the game ran, from {}", installed.display());
}
