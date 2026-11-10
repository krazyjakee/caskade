//! A share between two networks that really are two (#17, T1 and T2 in
//! `docs/manual-checks.md`), where `two_networks` has one kernel and a local
//! relay. `scripts/across-networks.sh` starts [`sharer`] on a GitHub runner and
//! the two others on this PC: real routers, real NAT, n0's relays and address
//! lookup, as the app sets them. Nothing here runs in CI's suites.
//!
//! Sharing from the runner keeps this PC's address out of a public log: only
//! the runner's own addresses are in the links.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use caskade_core::devices::Reach;
use caskade_core::install::JobStatus;
use caskade_share::ShareState;

use crate::networks::{MIB, add_game, follow, online, plays};
use crate::shared_cores::{Core, core_with};
use crate::two_networks::shared;

const SIZE: usize = 256 * MIB;
/// How far the second copy gets before Stop sharing.
const STOP_AT: u64 = 16 * MIB as u64;
const PATIENCE: Duration = Duration::from_secs(15 * 60);

fn links_file() -> PathBuf {
    std::env::var_os("CASKADE_SHARE_LINKS")
        .expect("run by scripts/across-networks.sh")
        .into()
}

/// The app's reach: local discovery, n0's lookup and n0's relays.
async fn device(name: &str) -> Core {
    let device = core_with(name, Reach::network(false, None)).await;
    online(&device).await;
    device
}

/// Whether what `done` says of a recipient of `share` has come true, within
/// [`PATIENCE`].
async fn recipient(device: &Core, share: &str, done: impl Fn(u64, bool) -> bool) -> bool {
    let deadline = Instant::now() + PATIENCE;
    let mut said = String::new();
    while Instant::now() < deadline {
        let shares = device.shares().list().await.unwrap();
        let record = shares.iter().find(|record| record.id == share).unwrap();
        for one in &record.recipients {
            let now = format!("{:?}: {} finished={}", record.state, one.done, one.finished);
            if now != said {
                eprintln!("{now}");
                said = now;
            }
            if done(one.done, one.finished) {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// Shares a game twice. The first is left to finish, the second is stopped
/// part-way.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "the sharer of scripts/across-networks.sh, on a GitHub runner"]
async fn sharer() {
    let device = device("Runner").await;
    let (entry, folder) = add_game(&device, SIZE).await;
    let (whole, whole_link) = shared(&device, &entry).await;
    // Another collection, so stopping the second leaves the first alone.
    std::fs::write(folder.join("Content/second.bin"), "the second share").unwrap();
    let (part, part_link) = shared(&device, &entry).await;
    std::fs::write(links_file(), format!("{whole_link}\n{part_link}\n")).unwrap();

    let arrived = recipient(&device, &whole, |_, finished| finished).await;
    eprintln!("the whole game arrived: {arrived}");
    device.shares().stop(&whole).await.unwrap();

    let part_way = recipient(&device, &part, |done, _| done > STOP_AT).await;
    eprintln!("the second copy got part-way: {part_way}");
    device.shares().stop(&part).await.unwrap();
    let stopped = device.shares().list().await.unwrap();
    let stopped = stopped.iter().find(|record| record.id == part).unwrap();
    assert_eq!(stopped.state, ShareState::Stopped);
    // The recipient asks once more, and is refused.
    tokio::time::sleep(Duration::from_secs(90)).await;
}

fn link(line: usize) -> String {
    let links = std::fs::read_to_string(links_file()).unwrap();
    links.lines().nth(line).expect("two links").to_owned()
}

/// T1: the game comes from a link opened on another network, and plays.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "this PC's half of scripts/across-networks.sh"]
async fn receives_and_plays() {
    let device = device("Home").await;
    let link = link(0);
    let info = device.shares().inspect(&link).await.unwrap();
    eprintln!("reach {:?}, size {:?}", info.reach, info.size);
    let started = Instant::now();
    let job = device.shares().receive(&link, None).await.unwrap();
    let done = follow(&device, &job.id, PATIENCE).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    let seconds = started.elapsed().as_secs_f64();
    let rate = SIZE as f64 / 1e6 / seconds;
    eprintln!("{SIZE} bytes in {seconds:.1} s, {rate:.1} MB/s");
    plays(&device, &done).await;
}

/// T2: Stop sharing on the other network fails a copy under way at once.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "this PC's half of scripts/across-networks.sh"]
async fn a_stop_fails_the_copy() {
    let device = device("Home").await;
    let link = link(1);
    let job = device.shares().receive(&link, None).await.unwrap();
    let done = follow(&device, &job.id, PATIENCE).await;
    assert_eq!(done.status, JobStatus::Failed, "{done:?}");
    let reason = done.error.clone().unwrap_or_default();
    assert!(reason.contains("stopped"), "{reason}");
    let arrived = done.progress.map_or(0, |progress| progress.bytes);
    assert!(arrived < SIZE as u64, "it all arrived anyway: {arrived}");
    eprintln!("failed at {arrived} bytes: {reason}");
    let error = device
        .shares()
        .inspect(&link)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("doesn't work any more"), "{error}");
}
