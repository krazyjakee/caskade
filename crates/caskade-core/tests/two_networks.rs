//! Devices on networks of their own, as `scripts/two-networks.sh` builds them
//! in network namespaces: routers doing NAT in front of them, a real iroh
//! relay on the internet between them (#10, #11, #17, #20). Each test here is
//! one process: a device, the relay, or one half of the NAT probe. What each
//! expects comes from the script, which runs the same tests on networks they
//! have to fail on, so that passing means something.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use caskade_core::devices::{ConnectionPurpose, DeviceLink, Devices, Reach};
use caskade_core::install::{JobSnapshot, JobStatus, PeerCopyTarget};
use caskade_core::launch::LaunchService;
use caskade_core::model::{
    Game, GameEdit, GameEntry, GameRuntime, InstallLocation, InstallState, LaunchSpec, TextEdit,
};
use caskade_core::share::{ShareExpiry, ShareReach, ShareState};
use caskade_sync::testing::{relay_on, relays_at};
use caskade_sync::{Admission, Config, EndpointId, Grant, Relays, Session, Sessions, SyncEndpoint};
use serde_json::json;

use crate::cores::{Core, core_on, core_with, within};
use crate::lan_unplugged::{Peer, pair_by_code};

const INTERNET: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 1);
const SECOND_HOST: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 9);
const MIB: usize = 1024 * 1024;
/// More than a session's frames and a relay's keepalives come to in a run,
/// and a sixtieth of the game: a game that passed through the relay shows.
const RELAY_ALLOWANCE: u64 = MIB as u64;

fn relays() -> Relays {
    relays_at(&std::env::var("CASKADE_NET_RELAY").expect("run by scripts/two-networks.sh"))
}

/// The app's reach, with a relay on this internet for n0's and no lookup on
/// n0's DNS: links carry the relay and the addresses.
fn reach() -> Reach {
    Reach {
        relays: relays(),
        ip: caskade_sync::Ip::Any { port: None },
        internet_lookup: false,
        lan: Some(caskade_sync::lan::SERVICE.to_owned()),
    }
}

/// A device that binds what `core.sync` asks for, as the app does, with this
/// internet's relay for n0's.
async fn core_following(name: &str) -> Core {
    let relays = relays();
    core_on(name, move |store, accounts, keys| {
        Devices::with_reach_from(store, accounts, keys, move |asked: Reach| Reach {
            relays: match asked.relays {
                Relays::N0 => relays.clone(),
                other => other,
            },
            internet_lookup: false,
            ..asked
        })
    })
    .await
}

/// `core.sync`: Connect only directly, and the port the endpoint binds.
async fn connection(device: &Core, never_relay: bool, port: Option<u16>) {
    device
        .settings
        .set(
            "core.sync",
            json!({ "categories": {}, "neverRelay": never_relay, "port": port }),
            None,
        )
        .await
        .unwrap();
    let bound = async || {
        let this_device = device.devices.status().await.this_device;
        this_device.listening && this_device.never_relay == never_relay && this_device.port == port
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while !bound().await {
        let this_device = device.devices.status().await.this_device;
        assert!(
            Instant::now() < deadline,
            "the endpoint didn't bind again for {never_relay} {port:?}: {this_device:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn expected() -> String {
    std::env::var("CASKADE_NET_EXPECT").unwrap_or_default()
}

/// How long a scenario gives something that has to happen. Short in the runs
/// meant to fail, so they don't take a timeout each.
fn patience() -> Duration {
    let seconds = std::env::var("CASKADE_NET_PATIENCE").unwrap_or_default();
    Duration::from_secs(seconds.parse().unwrap_or(60))
}

/// Bytes through the relay so far, both ways, as iptables in the internet's
/// namespace counts them, so iroh's word isn't taken for it.
fn relay_bytes() -> u64 {
    let meter = Path::new(&std::env::var("CASKADE_LAN_NOTES").unwrap()).join("relay-bytes");
    std::fs::read_to_string(meter)
        .expect("the script keeps the meter")
        .trim()
        .parse()
        .expect("a number")
}

async fn online(device: &Core) {
    within("a relay answers", 30, || async {
        device.devices.status().await.this_device.internet == Some(true)
    })
    .await;
}

/// A Linux game: a program that leaves a mark beside itself when it runs,
/// and `size` bytes of data.
async fn add_game(device: &Core, size: usize) -> (String, PathBuf) {
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

/// Shares `entry` for a week once its files are read: its id and link.
async fn shared(device: &Core, entry: &str) -> (String, String) {
    let share = device.shares.share(entry, ShareExpiry::Week).await.unwrap();
    within("the share's files are read", 60, || async {
        let shares = device.shares.list().await.unwrap();
        shares
            .iter()
            .any(|record| record.id == share.id && record.state == ShareState::Ready)
    })
    .await;
    let link = device.shares.link(&share.id).await.unwrap();
    (share.id, link)
}

/// Follows `job` until it ends or `wait` passes, logging what it says.
async fn follow(device: &Core, job: &str, wait: Duration) -> JobSnapshot {
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
async fn plays(device: &Core, done: &JobSnapshot) {
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

/// The relay, on the internet, until the script ends it.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "the relay of scripts/two-networks.sh"]
async fn relay() {
    let peer = Peer::from_env();
    let (at, _server) = relay_on(INTERNET).await;
    peer.say("at", &at);
    std::future::pending::<()>().await;
}

/// Answers each datagram with the address it came from. `knock` also has
/// other sockets write to that address: a port and a host the sender never
/// wrote to.
#[test]
#[ignore = "the outside half of the NAT probe in scripts/two-networks.sh"]
fn nat_echo() {
    let notes = PathBuf::from(std::env::var("CASKADE_LAN_NOTES").unwrap());
    let first = UdpSocket::bind((INTERNET, 7001)).unwrap();
    let second = UdpSocket::bind((INTERNET, 7002)).unwrap();
    let other_port = UdpSocket::bind((INTERNET, 7003)).unwrap();
    let other_host = UdpSocket::bind((SECOND_HOST, 7001)).unwrap();
    first.set_nonblocking(true).unwrap();
    second.set_nonblocking(true).unwrap();
    std::fs::write(notes.join("echo-ready"), "").unwrap();
    let mut buffer = [0; 64];
    loop {
        for socket in [&first, &second] {
            if let Ok((length, from)) = socket.recv_from(&mut buffer) {
                if &buffer[..length] == b"knock" {
                    other_port.send_to(b"another port", from).unwrap();
                    other_host.send_to(b"another host", from).unwrap();
                } else {
                    socket.send_to(from.to_string().as_bytes(), from).unwrap();
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// What the NAT in front of this namespace does, from what the outside saw
/// and what got back in: the scenarios run on the NAT they say they do.
#[test]
#[ignore = "the inside half of the NAT probe in scripts/two-networks.sh"]
fn nat_probe() {
    let nat = std::env::var("CASKADE_NAT").unwrap();
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let ask = |port: u16| -> SocketAddr {
        socket.send_to(b"who", (INTERNET, port)).unwrap();
        let mut buffer = [0; 64];
        let (length, _) = socket.recv_from(&mut buffer).unwrap();
        std::str::from_utf8(&buffer[..length])
            .unwrap()
            .parse()
            .unwrap()
    };
    let (a, b) = (ask(7001), ask(7002));
    assert_eq!(a, ask(7001), "a mapping stays while it's used");
    socket.send_to(b"knock", (INTERNET, 7001)).unwrap();
    let mut heard = Vec::new();
    let mut buffer = [0; 64];
    while let Ok((length, _)) = socket.recv_from(&mut buffer) {
        heard.push(String::from_utf8_lossy(&buffer[..length]).into_owned());
    }
    let inside = socket.local_addr().unwrap().port();
    println!(
        "nat: {nat}: inside port {inside}, seen by two ports of one host as {a} and {b}; \
         got back in unasked: {heard:?}"
    );
    match nat.as_str() {
        "cone" => {
            assert_eq!(a, b, "one mapping for both");
            assert_eq!(a.port(), inside, "the port kept");
            assert!(heard.is_empty(), "{heard:?}");
        }
        "symmetric" => {
            assert_ne!(a, b, "a mapping per destination");
            assert!(heard.is_empty(), "{heard:?}");
        }
        "fullcone" => {
            assert_eq!(a, b, "one mapping for both");
            assert_eq!(heard.len(), 2, "{heard:?}");
        }
        other => panic!("no NAT called {other}"),
    }
}

/// T1, #17: the Desk's share link opened behind another router. The game
/// arrives directly, nothing of it through the relay, and it plays.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "one device of two: scripts/two-networks.sh"]
async fn share_arrives_and_plays() {
    let peer = Peer::from_env();
    let device = core_with(&peer.me, reach()).await;
    online(&device).await;
    let size = 32 * MIB;
    if peer.me == "Desk" {
        let (entry, _) = add_game(&device, size).await;
        let (_, link) = shared(&device, &entry).await;
        peer.say("link", &link);
        peer.heard("played").await;
        return;
    }
    let link = peer.heard("link").await;
    let before = relay_bytes();
    let info = device.shares.inspect(&link).await.unwrap();
    assert_eq!(info.reach, ShareReach::Direct);
    let job = device.shares.receive(&link, None).await.unwrap();
    let done = follow(&device, &job.id, patience()).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    let through_relay = relay_bytes() - before;
    eprintln!("{size} bytes of game, {through_relay} bytes through the relay");
    assert!(through_relay < RELAY_ALLOWANCE, "{through_relay}");
    plays(&device, &done).await;
    peer.say("played", "yes");
}

/// T1's other outcome, #17: with no direct path the job says only a relay
/// reaches the sharer, and no game passes through it.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "one device of two: scripts/two-networks.sh"]
async fn share_waits_when_only_a_relay_reaches() {
    let peer = Peer::from_env();
    let device = core_with(&peer.me, reach()).await;
    online(&device).await;
    let size = 32 * MIB;
    if peer.me == "Desk" {
        let (entry, _) = add_game(&device, size).await;
        let (share, link) = shared(&device, &entry).await;
        peer.say("link", &link);
        peer.heard("waited").await;
        let records = device.shares.list().await.unwrap();
        let record = records.iter().find(|record| record.id == share).unwrap();
        assert!(
            record.recipients.iter().all(|who| !who.finished),
            "{record:?}"
        );
        return;
    }
    let link = peer.heard("link").await;
    let before = relay_bytes();
    let info = device.shares.inspect(&link).await.unwrap();
    assert_eq!(info.reach, ShareReach::RelayOnly);
    let job = device.shares.receive(&link, None).await.unwrap();
    let waiting = follow(&device, &job.id, Duration::from_secs(40)).await;
    assert!(!waiting.status.is_finished(), "{waiting:?}");
    let progress = waiting.progress.expect("it says why");
    assert!(
        progress.message.contains("Only a relay reaches"),
        "{progress:?}"
    );
    assert_eq!(progress.bytes, 0);
    let through_relay = relay_bytes() - before;
    eprintln!("waited 40 s, {through_relay} bytes through the relay of a {size}-byte game");
    assert!(through_relay < RELAY_ALLOWANCE, "{through_relay}");
    peer.say("waited", "yes");
}

/// T2, #17: Stop sharing part-way, across the two routers. `no-stop` is its
/// control: the same copy left alone finishes, so it's the stop that ends it.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "one device of two: scripts/two-networks.sh"]
async fn stopping_a_share_fails_the_copy_part_way() {
    let peer = Peer::from_env();
    let device = core_with(&peer.me, reach()).await;
    online(&device).await;
    let leave_it = expected() == "no-stop";
    let size = 48 * MIB;
    if peer.me == "Desk" {
        let (entry, _) = add_game(&device, size).await;
        let (share, link) = shared(&device, &entry).await;
        peer.say("link", &link);
        peer.heard("part-way").await;
        if !leave_it {
            device.shares.stop(&share).await.unwrap();
            peer.say("stopped", "yes");
        }
        peer.heard("over").await;
        return;
    }
    let link = peer.heard("link").await;
    let job = device.shares.receive(&link, None).await.unwrap();
    within("some of it arrives", 60, || async {
        device
            .install
            .job(&job.id)
            .and_then(|job| job.progress)
            .is_some_and(|progress| {
                progress.bytes > MIB as u64 && progress.bytes < (size / 2) as u64
            })
    })
    .await;
    peer.say("part-way", "yes");
    if leave_it {
        let done = follow(&device, &job.id, patience()).await;
        assert_eq!(done.status, JobStatus::Completed, "{done:?}");
        peer.say("over", "yes");
        return;
    }
    peer.heard("stopped").await;
    let stopped = Instant::now();
    let done = follow(&device, &job.id, Duration::from_secs(15)).await;
    assert_eq!(done.status, JobStatus::Failed, "{done:?}");
    eprintln!("failed {:?} after the stop", stopped.elapsed());
    assert!(
        stopped.elapsed() < Duration::from_secs(10),
        "{:?}",
        stopped.elapsed()
    );
    let reason = done.error.clone().unwrap_or_default();
    assert!(reason.contains("stopped"), "{reason}");
    let arrived = done.progress.map_or(0, |progress| progress.bytes);
    assert!(arrived < size as u64, "it all arrived anyway: {arrived}");
    let error = device.shares.inspect(&link).await.unwrap_err().to_string();
    assert!(error.contains("doesn't work any more"), "{error}");
    peer.say("over", "yes");
}

/// Pairs the two with a pairing link the Desk makes: it carries the relay.
async fn pair_by_link(device: &Core, peer: &Peer) -> String {
    let other = if peer.me == "Desk" {
        let link = device.devices.pairing_link().await.unwrap();
        peer.say("pairing", &link.url);
        peer.heard("id").await
    } else {
        let url = peer.heard("pairing").await;
        device.devices.accept_pairing_link(&url).await.unwrap();
        let id = device.devices.status().await.this_device.id.unwrap();
        peer.say("id", &id);
        device.devices.status().await.devices[0].id.clone()
    };
    within("both are paired", 30, || async {
        let status = device.devices.status().await;
        status.devices.iter().any(|paired| paired.id == other)
    })
    .await;
    other
}

/// The paired device's link now.
async fn link(device: &Core) -> DeviceLink {
    device.devices.status().await.devices[0].link
}

/// The game copy between paired devices, #11. `copies`: it crosses the
/// routers directly. Otherwise a relay is all there is (T3, #11, #20): the
/// link says so, and the copy waits and moves nothing.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "one device of two: scripts/two-networks.sh"]
async fn copy_between_paired_devices() {
    let peer = Peer::from_env();
    let device = core_with(&peer.me, reach()).await;
    online(&device).await;
    let other = pair_by_link(&device, &peer).await;
    let _sync = device.sync();
    let copies = expected() == "copies";
    let wanted = if copies {
        DeviceLink::Direct
    } else {
        DeviceLink::Relay
    };
    within(&format!("the link is {wanted:?}"), 30, || async {
        link(&device).await == wanted
    })
    .await;
    let size = 32 * MIB;
    if peer.me == "Desk" {
        let (entry, _) = add_game(&device, size).await;
        peer.say("celeste", &entry);
        peer.heard("over").await;
        return;
    }
    let entry_id = peer.heard("celeste").await;
    let game = device
        .store
        .upsert_game(&Game::new("Celeste"))
        .await
        .unwrap();
    let before = relay_bytes();
    let job = device
        .install
        .copy_from_device(
            PeerCopyTarget {
                device_id: other,
                device_name: peer.other.clone(),
                entry_id,
                game_id: game.id.clone(),
                title: "Celeste".into(),
            },
            None,
        )
        .await
        .unwrap();
    if copies {
        let done = follow(&device, &job.id, patience()).await;
        assert_eq!(done.status, JobStatus::Completed, "{done:?}");
        let through_relay = relay_bytes() - before;
        eprintln!("{size} bytes of game, {through_relay} bytes through the relay");
        assert!(through_relay < RELAY_ALLOWANCE, "{through_relay}");
        plays(&device, &done).await;
    } else {
        let waiting = follow(&device, &job.id, Duration::from_secs(40)).await;
        assert!(!waiting.status.is_finished(), "{waiting:?}");
        let progress = waiting.progress.expect("it says why");
        assert!(
            progress.message.contains("Only a relay reaches"),
            "{progress:?}"
        );
        assert_eq!(progress.bytes, 0);
        assert_eq!(link(&device).await, DeviceLink::Relay);
        let through_relay = relay_bytes() - before;
        eprintln!("waited 40 s, {through_relay} bytes through the relay of a {size}-byte game");
        assert!(through_relay < RELAY_ALLOWANCE, "{through_relay}");
    }
    peer.say("over", "yes");
}

/// T3, #11, #20: paired through a relay, then Connect only directly on, both
/// sides or the Desk's alone. From then on the link is never `relay`, and
/// it's what the script expects: `direct` to a Desk that can be dialled (a
/// public address and a fixed port), otherwise `offline`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "one device of two: scripts/two-networks.sh"]
async fn connect_only_directly_never_shows_a_relay() {
    let peer = Peer::from_env();
    let device = core_following(&peer.me).await;
    let is_desk = peer.me == "Desk";
    let port = std::env::var("CASKADE_NET_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .filter(|_| is_desk);
    let turns_it_on = is_desk || std::env::var("CASKADE_NET_ONLY_DESK").is_err();
    connection(&device, false, port).await;
    online(&device).await;
    pair_by_link(&device, &peer).await;
    let sync = device.sync();
    within("a first sync", 60, || async {
        device.devices.status().await.devices[0]
            .last_sync_ms
            .is_some()
    })
    .await;
    peer.say("synced", "yes");
    peer.heard("synced").await;
    // Long enough for whichever link it has to settle, and to be remembered.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let before = link(&device).await;
    eprintln!("with relays allowed the link is {before:?}");
    if turns_it_on {
        connection(&device, true, port).await;
        assert_eq!(device.devices.status().await.this_device.internet, None);
    }
    peer.say("on", "yes");
    peer.heard("on").await;

    // Something to sync, so the devices keep trying to reach each other.
    let snapshot = device.store.sync_snapshot().await.unwrap();
    let game = snapshot.games.into_iter().next().unwrap();
    let edit = GameEdit {
        description: Some(TextEdit::Set {
            value: format!("Edited on {}", peer.me),
        }),
        ..GameEdit::default()
    };
    device.store.edit_game(&game, &edit, None).await.unwrap();
    let other = device.devices.status().await.devices[0].id.clone();
    let mut seen = Vec::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        sync.sync_now(&other);
        let now = link(&device).await;
        // It's the setting's promise: the device without it may show its
        // last relay until an exchange fails.
        if turns_it_on {
            assert_ne!(now, DeviceLink::Relay, "after {:?}", started.elapsed());
        }
        if seen.last() != Some(&now) {
            eprintln!("{:.1?}: {now:?}", started.elapsed());
            seen.push(now);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let wanted = if expected() == "direct" {
        DeviceLink::Direct
    } else {
        DeviceLink::Offline
    };
    if turns_it_on {
        assert_eq!(link(&device).await, wanted, "seen: {seen:?}");
    }
    if wanted == DeviceLink::Offline {
        // Offline in fact: no session opens, whatever the screen says.
        let session = device
            .devices
            .connect(&other, ConnectionPurpose::Sync)
            .await;
        assert!(session.is_err(), "a session opened");
    }
    peer.say("over", "yes");
    peer.heard("over").await;
}

/// O5, #10: Connect only directly on the Desk, two devices on one network
/// whose internet comes back. The link stays `lan`, on both.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "one device of two: scripts/two-networks.sh"]
async fn the_link_stays_lan_when_the_internet_returns() {
    let peer = Peer::from_env();
    let device = core_following(&peer.me).await;
    let is_desk = peer.me == "Desk";
    let other = pair_by_code(&device.devices, &peer, is_desk).await;
    let sync = device.sync();
    within("a first sync", 60, || async {
        device.devices.status().await.devices[0]
            .last_sync_ms
            .is_some()
    })
    .await;
    assert_eq!(
        device.devices.status().await.this_device.internet,
        Some(false)
    );
    if is_desk {
        connection(&device, true, None).await;
    }
    peer.say("set", "yes");
    peer.heard("set").await;
    within("the devices find each other again", 60, || async {
        sync.sync_now(&other);
        link(&device).await == DeviceLink::Lan
    })
    .await;

    // The script plugs the internet in now.
    peer.say("plug", "yes");
    peer.heard("plug").await;
    let this_device = || async { device.devices.status().await.this_device };
    let on_this_network = async || {
        let session = device
            .devices
            .connect(&other, ConnectionPurpose::Copy)
            .await;
        let session = session.expect("a session opens");
        let path = session.direct_path(Duration::from_secs(2)).await;
        session.close();
        path.is_some_and(|path| path.ip().to_string().starts_with("192.168.10."))
    };
    assert!(on_this_network().await, "before the internet returns");
    if is_desk {
        assert_eq!(this_device().await.internet, None);
        assert!(reaches_the_internet().await, "the router is plugged in");
    } else {
        within("the relay answers now", 30, || async {
            this_device().await.internet == Some(true)
        })
        .await;
    }
    peer.say("online", "yes");
    peer.heard("online").await;

    // An edit each way, and the link watched while they cross.
    let snapshot = device.store.sync_snapshot().await.unwrap();
    let game = snapshot.games.into_iter().next().unwrap();
    let said = format!("Edited on {}", peer.me);
    let edit = GameEdit {
        tags: None,
        description: is_desk.then(|| TextEdit::Set {
            value: said.clone(),
        }),
        ..GameEdit::default()
    };
    device.store.edit_game(&game, &edit, None).await.unwrap();
    if !is_desk {
        device.store.set_game_favorite(&game, true).await.unwrap();
    }
    // The status says `lan` while mDNS remembers the device, so a session's
    // real path is asked as well.
    let started = Instant::now();
    let mut asked = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        sync.sync_now(&other);
        let now = link(&device).await;
        assert_eq!(now, DeviceLink::Lan, "after {:?}", started.elapsed());
        if asked.elapsed() > Duration::from_secs(2) {
            assert!(on_this_network().await, "after {:?}", started.elapsed());
            asked = Instant::now();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    within("the other side's edit arrives", 10, || async {
        let row = device.store.game(&game).await.unwrap().unwrap();
        if is_desk {
            row.favorite
        } else {
            row.description == "Edited on Desk"
        }
    })
    .await;
    peer.say("over", "yes");
    peer.heard("over").await;
}

/// Whether a TCP connection reaches the internet's relay from here.
async fn reaches_the_internet() -> bool {
    let relay = std::env::var("CASKADE_NET_RELAY").unwrap();
    let address = relay
        .trim_start_matches("https://")
        .split_once(' ')
        .unwrap()
        .0
        .to_owned();
    tokio::net::lookup_host(&address).await.is_ok()
        && tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::TcpStream::connect(&address),
        )
        .await
        .is_ok_and(|stream| stream.is_ok())
}

/// O6, #10: multicast blocked between the two, as guest Wi-Fi does. Neither
/// sees the other nearby, and a pairing link still pairs and syncs, `lan`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "one device of two: scripts/two-networks.sh"]
async fn a_pairing_link_pairs_with_multicast_blocked() {
    let peer = Peer::from_env();
    let device = core_with(&peer.me, Reach::network(false, None)).await;
    let is_desk = peer.me == "Desk";
    // A device on this network shows within a second or two when multicast
    // passes (lan-unplugged.sh).
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        assert_eq!(
            device.devices.status().await.nearby,
            Vec::new(),
            "seen nearby"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let other = pair_by_link(&device, &peer).await;
    assert!(device.devices.status().await.nearby.is_empty());
    let sync = device.sync();
    within("a first sync", 60, || async {
        device.devices.status().await.devices[0]
            .last_sync_ms
            .is_some()
    })
    .await;
    assert_eq!(link(&device).await, DeviceLink::Lan);

    let snapshot = device.store.sync_snapshot().await.unwrap();
    let game = snapshot.games.into_iter().next().unwrap();
    if is_desk {
        let edit = GameEdit {
            description: Some(TextEdit::Set {
                value: "Edited on Desk".to_owned(),
            }),
            ..GameEdit::default()
        };
        device.store.edit_game(&game, &edit, None).await.unwrap();
    } else {
        device.store.set_game_favorite(&game, true).await.unwrap();
    }
    within("the other side's edit arrives", 60, || async {
        sync.sync_now(&other);
        let row = device.store.game(&game).await.unwrap().unwrap();
        if is_desk {
            row.favorite
        } else {
            row.description == "Edited on Desk"
        }
    })
    .await;
    assert_eq!(link(&device).await, DeviceLink::Lan);
    peer.say("over", "yes");
    peer.heard("over").await;
}

/// Admits everyone: the meter's control has no devices to speak of.
struct Open;

impl Admission for Open {
    fn admits(&self, _: EndpointId, _: ConnectionPurpose, _: Option<&Grant>) -> bool {
        true
    }
}

/// Takes a stream of bytes and says how many came.
struct Sink;

#[async_trait]
impl Sessions for Sink {
    async fn opened(&self, session: Session) {
        let connection = session.connection().clone();
        let mut stream = connection.accept_uni().await.unwrap();
        let mut count = 0_u64;
        let mut chunk = vec![0; 64 * 1024];
        while let Some(read) = stream.read(&mut chunk).await.unwrap() {
            count += read as u64;
        }
        eprintln!("{count} bytes came through");
        connection.closed().await;
        drop(session);
    }
}

/// The meter's control: bytes known to pass through the relay show on it. A
/// low reading in the scenarios above is worth nothing without this.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "one device of two: scripts/two-networks.sh"]
async fn the_relay_meter_sees_what_passes_through_the_relay() {
    let peer = Peer::from_env();
    let config = Config {
        reach: reach(),
        ..Config::default()
    };
    let endpoint = SyncEndpoint::bind(config, Arc::new(Open), Arc::new(Sink))
        .await
        .unwrap();
    if peer.me == "Desk" {
        let addr = endpoint.shareable_addr(Duration::from_secs(10)).await;
        let ticket = caskade_sync::share::ShareLink::new(addr, Grant::generate(), None);
        peer.say("addr", &ticket.to_url());
        peer.heard("sent").await;
        return;
    }
    let ticket = caskade_sync::share::ShareLink::parse(&peer.heard("addr").await).unwrap();
    let before = relay_bytes();
    let session = endpoint
        .connect(ticket.addr().clone(), ConnectionPurpose::Share, None)
        .await
        .unwrap();
    assert_eq!(session.direct_path(Duration::from_secs(3)).await, None);
    let mut stream = session.connection().open_uni().await.unwrap();
    let size = 8 * MIB;
    stream.write_all(&vec![7; size]).await.unwrap();
    stream.finish().unwrap();
    stream.stopped().await.ok();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let through_relay = relay_bytes() - before;
    eprintln!("{size} bytes sent, {through_relay} counted at the relay");
    assert!(through_relay >= size as u64, "{through_relay}");
    peer.say("sent", "yes");
}
