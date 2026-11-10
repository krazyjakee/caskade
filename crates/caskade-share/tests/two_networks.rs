//! The share scenarios of `scripts/two-networks.sh` (#17): devices on
//! networks of their own in network namespaces, routers doing NAT in front of
//! them, a real iroh relay on the internet between them. Each test here is
//! one device; the relay and the rest of the scenarios are caskade-core's
//! `two_networks`, whose helpers this crate includes.

use std::time::{Duration, Instant};

use caskade_core::install::JobStatus;
use caskade_share::{ShareExpiry, ShareReach, ShareState};

use crate::cores::within;
use crate::networks::{
    MIB, RELAY_ALLOWANCE, add_game, expected, follow, online, patience, plays, reach, relay_bytes,
};
use crate::peer::Peer;
use crate::shared_cores::{Core, core_with};

/// Shares `entry` for a week once its files are read: its id and link.
pub async fn shared(device: &Core, entry: &str) -> (String, String) {
    let share = device
        .shares()
        .share(entry, ShareExpiry::Week)
        .await
        .unwrap();
    within("the share's files are read", 60, || async {
        let shares = device.shares().list().await.unwrap();
        shares
            .iter()
            .any(|record| record.id == share.id && record.state == ShareState::Ready)
    })
    .await;
    let link = device.shares().link(&share.id).await.unwrap();
    (share.id, link)
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
    let info = device.shares().inspect(&link).await.unwrap();
    assert_eq!(info.reach, ShareReach::Direct);
    let job = device.shares().receive(&link, None).await.unwrap();
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
        let records = device.shares().list().await.unwrap();
        let record = records.iter().find(|record| record.id == share).unwrap();
        assert!(
            record.recipients.iter().all(|who| !who.finished),
            "{record:?}"
        );
        return;
    }
    let link = peer.heard("link").await;
    let before = relay_bytes();
    let info = device.shares().inspect(&link).await.unwrap();
    assert_eq!(info.reach, ShareReach::RelayOnly);
    let job = device.shares().receive(&link, None).await.unwrap();
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
            device.shares().stop(&share).await.unwrap();
            peer.say("stopped", "yes");
        }
        peer.heard("over").await;
        return;
    }
    let link = peer.heard("link").await;
    let job = device.shares().receive(&link, None).await.unwrap();
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
    let error = device
        .shares()
        .inspect(&link)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("doesn't work any more"), "{error}");
    peer.say("over", "yes");
}
