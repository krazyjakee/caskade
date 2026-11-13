//! What this machine lets a host do, and what people are told.

use super::*;
use crate::host::checks::network_lets_others_in;

/// `f`'s host, looking at this machine with `checks`.
fn host_with(
    f: &Fixture,
    checks: impl Fn(OperatingSystem) -> HostChecks + Send + Sync + 'static,
) -> StreamingHost {
    StreamingHost::with(
        f.host.inner.tools.clone(),
        f.host.inner.elevation.clone(),
        f.settings.clone(),
        Parts {
            existing: Arc::new(|| None),
            os: OperatingSystem::Linux,
            runner: Arc::new(process::ChildRunner::new(
                f.temp.path().join("data/streaming/run.json"),
            )),
            secrets: f.secrets.clone(),
            timing: fast(),
            api_base: f.host.inner.api_base.clone(),
            http_port: f.host.inner.http_port,
            checks: Arc::new(checks),
        },
    )
}

/// Turns `host` on and waits for it to be ready.
async fn hosting(f: &Fixture, host: &StreamingHost) {
    let enabling = tokio::spawn({
        let host = host.clone();
        async move { host.enable().await }
    });
    until("credentials made", || at_once(f.secrets.get()).is_some()).await;
    answer(f).await;
    enabling.await.unwrap().unwrap();
    until("ready", || host.status().state == HostState::Ready).await;
}

/// A PC that moved from a public network to a private one: it stops saying
/// nobody can reach it by itself, with no Repair or restart, and says so
/// again when it moves back. The host stays ready all along.
#[tokio::test(flavor = "multi_thread")]
async fn the_warning_follows_the_network_while_hosting_runs() {
    let f = fixture(fast()).await;
    let public = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let network = public.clone();
    let host = host_with(&f, move |os| HostChecks {
        reachable: !network.load(std::sync::atomic::Ordering::SeqCst),
        ..HostChecks::fine(os)
    });
    hosting(&f, &host).await;
    assert!(host.status().unreachable);

    public.store(false, std::sync::atomic::Ordering::SeqCst);
    until("the warning gone", || {
        let status = host.status();
        !status.unreachable && status.warning.is_none()
    })
    .await;
    assert_eq!(host.status().state, HostState::Ready);

    public.store(true, std::sync::atomic::Ordering::SeqCst);
    until("the warning back", || host.status().unreachable).await;
    assert!(host.status().warning.unwrap().contains("public"));
    assert_eq!(host.status().state, HostState::Ready);
    host.shutdown();
}

/// A look can take a while, and on Windows it runs programs: one that never
/// ends leaves the status alone, isn't joined by more, and doesn't stop a
/// crash being noticed.
#[tokio::test(flavor = "multi_thread")]
async fn a_look_that_hangs_holds_nothing_up() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let f = fixture(fast()).await;
    let looks = Arc::new(AtomicUsize::new(0));
    let (release, stuck) = std::sync::mpsc::channel::<()>();
    let stuck = Mutex::new(stuck);
    let counted = looks.clone();
    let host = host_with(&f, move |os| {
        if counted.fetch_add(1, Ordering::SeqCst) == 1 {
            let _ = caskade_core::lock(&stuck).recv();
        }
        HostChecks::fine(os)
    });
    hosting(&f, &host).await;
    until("the second look", || looks.load(Ordering::SeqCst) == 2).await;
    tokio::time::sleep(fast().look_every * 6).await;
    assert_eq!(looks.load(Ordering::SeqCst), 2, "one look at a time");
    assert_eq!(host.status().state, HostState::Ready);

    let record: serde_json::Value = serde_json::from_slice(
        &std::fs::read(f.temp.path().join("data/streaming/run.json")).unwrap(),
    )
    .unwrap();
    std::process::Command::new("kill")
        .args(["-KILL", &record["pid"].as_u64().unwrap().to_string()])
        .status()
        .unwrap();
    until("the crash noticed", || looks.load(Ordering::SeqCst) > 2).await;
    release.send(()).unwrap();
    until("ready after the crash", || {
        host.status().state == HostState::Ready
    })
    .await;
    host.shutdown();
}

/// A look that ends after the host stopped has nothing to say.
#[tokio::test(flavor = "multi_thread")]
async fn a_late_look_leaves_a_stopped_host_alone() {
    let f = fixture(fast()).await;
    f.host.show_checks(HostChecks {
        reachable: false,
        ..HostChecks::fine(OperatingSystem::Linux)
    });
    assert_eq!(f.host.status().state, HostState::Off);
    assert!(!f.host.status().unreachable);
    assert_eq!(f.host.status().warning, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ready_host_says_when_pads_or_a_screen_wont_work() {
    let f = fixture(fast()).await;
    let host = |checks: fn(OperatingSystem) -> HostChecks| host_with(&f, checks);
    let no_pads = host(|_| HostChecks {
        screen: true,
        pads: false,
        reachable: true,
    });
    let enabling = tokio::spawn({
        let host = no_pads.clone();
        async move { host.enable().await }
    });
    until("credentials made", || at_once(f.secrets.get()).is_some()).await;
    answer(&f).await;
    enabling.await.unwrap().unwrap();
    until("ready", || no_pads.status().state == HostState::Ready).await;
    assert_eq!(
        no_pads.status().warning.as_deref(),
        Some("Controllers on other devices won't work here until Repair sets them up.")
    );
    no_pads.shutdown();

    let headless = host(|_| HostChecks {
        screen: false,
        pads: true,
        reachable: true,
    });
    headless.start().await;
    until("ready", || headless.status().state == HostState::Ready).await;
    assert!(headless.status().warning.unwrap().contains("no screen"));
    headless.shutdown();

    let fine = host(HostChecks::fine);
    fine.start().await;
    until("ready", || fine.status().state == HostState::Ready).await;
    assert_eq!(fine.status().warning, None);
    assert!(!fine.status().unreachable);
    fine.shutdown();

    // Ready above "nobody can reach this" would disagree with it: the status
    // says so, and the host goes on running for when somebody can. A warning
    // that leaves it reachable doesn't.
    let cut_off = host(|os| HostChecks {
        reachable: false,
        ..HostChecks::fine(os)
    });
    cut_off.start().await;
    until("ready", || cut_off.status().state == HostState::Ready).await;
    let status = cut_off.status();
    assert!(status.unreachable, "{status:?}");
    assert!(status.warning.unwrap().contains("public"));
    cut_off.shutdown();
    assert!(!cut_off.status().unreachable);

    let no_screen = host(|os| HostChecks {
        screen: false,
        ..HostChecks::fine(os)
    });
    no_screen.start().await;
    until("ready", || no_screen.status().state == HostState::Ready).await;
    assert!(!no_screen.status().unreachable);
    no_screen.shutdown();
    assert!(
        HostChecks {
            screen: true,
            pads: false,
            reachable: true,
        }
        .warning(OperatingSystem::Windows)
        .unwrap()
        .contains("controller driver")
    );
}

#[test]
fn a_windows_pc_only_on_public_networks_is_told_nobody_can_reach_it() {
    assert!(network_lets_others_in("Public\r\nPrivate\r\n"));
    assert!(network_lets_others_in("DomainAuthenticated\r\n"));
    assert!(!network_lets_others_in("Public\r\nPublic\r\n"));
    let checks = HostChecks {
        reachable: false,
        ..HostChecks::fine(OperatingSystem::Windows)
    };
    let warning = checks.warning(OperatingSystem::Windows).unwrap();
    assert!(warning.contains("public"), "{warning}");
}
