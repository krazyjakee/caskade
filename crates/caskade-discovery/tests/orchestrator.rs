use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use caskade_discovery::{
    Cancel, Config, DetectedGame, HostEnv, LauncherKind, LauncherStatus, OsSet, ScanCtx, Source,
    SourceError, SourceId, SourceSelection, WatchPath, detect_launchers_with_sources,
    scan_with_sources, testing::FakeClock,
};

#[derive(Clone, Copy)]
enum Behaviour {
    Games(usize),
    Panic,
    Fail,
    /// Sleeps without checking its context.
    Hang,
    /// Loops until cancelled.
    Cooperative,
}

struct Fake {
    id: &'static str,
    hosts: OsSet,
    behaviour: Behaviour,
}

fn fake(id: &'static str, behaviour: Behaviour) -> Arc<dyn Source> {
    Arc::new(Fake {
        id,
        hosts: OsSet::ALL,
        behaviour,
    })
}

impl Source for Fake {
    fn id(&self) -> SourceId {
        SourceId::new(self.id)
    }
    fn hosts(&self) -> OsSet {
        self.hosts
    }
    fn detect(&self, _env: &HostEnv) -> Vec<LauncherStatus> {
        vec![LauncherStatus {
            kind: Some(LauncherKind::Native),
            found: true,
            path: Some(PathBuf::from(format!("/opt/{}", self.id))),
            ..LauncherStatus::not_found(self.id())
        }]
    }
    fn scan(&self, _env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        match self.behaviour {
            Behaviour::Games(count) => {
                ctx.warn(format!("{} warning", self.id));
                // Reverse order: the orchestrator sorts by key.
                Ok((0..count)
                    .rev()
                    .map(|n| DetectedGame::new(self.id(), format!("{n:03}"), format!("Game {n}")))
                    .collect())
            }
            Behaviour::Panic => panic!("{} exploded", self.id),
            Behaviour::Fail => Err(SourceError::parse("/x/library.vdf", "bad header")),
            Behaviour::Hang => {
                std::thread::sleep(Duration::from_secs(3));
                Ok(vec![DetectedGame::new(self.id(), "late", "Late")])
            }
            Behaviour::Cooperative => loop {
                ctx.check()?;
                std::thread::sleep(Duration::from_millis(5));
            },
        }
    }
    fn watch_paths(&self, _env: &HostEnv) -> Vec<WatchPath> {
        Vec::new()
    }
}

fn env() -> HostEnv {
    HostEnv::live().with_clock(Arc::new(FakeClock::default()))
}

fn config(source_budget_ms: u64) -> Config {
    Config {
        source_budget_ms,
        ..Config::default()
    }
}

#[test]
fn failing_panicking_and_hanging_sources_do_not_affect_others() {
    let sources = [
        fake("alpha", Behaviour::Games(2)),
        fake("boom", Behaviour::Panic),
        fake("broken", Behaviour::Fail),
        fake("sleepy", Behaviour::Hang),
        fake("polite", Behaviour::Cooperative),
        fake("omega", Behaviour::Games(1)),
    ];
    let started = Instant::now();
    // A second per source: a panic on a busy Windows runner took over the
    // 300 ms this once allowed. The hang still outlasts the limit below.
    let report = scan_with_sources(&env(), &config(1_000), &Cancel::new(), &sources).unwrap();
    // On a single-core host the sole worker waits for the uncooperative
    // source to stop before later sources can run; it must not oversubscribe.
    let limit = if std::thread::available_parallelism().map_or(1, usize::from) == 1 {
        Duration::from_secs(5)
    } else {
        Duration::from_millis(2_500)
    };
    assert!(started.elapsed() < limit, "{:?}", started.elapsed());

    let ids: Vec<_> = report.sources.iter().map(|r| r.source.as_str()).collect();
    assert_eq!(
        ids,
        ["alpha", "boom", "broken", "sleepy", "polite", "omega"]
    );
    let errors: Vec<_> = report.sources.iter().map(|r| r.error.clone()).collect();
    assert_eq!(errors[0], None);
    assert_eq!(
        errors[1],
        Some(SourceError::Panicked {
            message: "boom exploded".into()
        })
    );
    assert!(matches!(errors[2], Some(SourceError::Parse { .. })));
    assert_eq!(errors[3], Some(SourceError::TimedOut));
    assert_eq!(errors[4], Some(SourceError::TimedOut));
    assert_eq!(errors[5], None);
    assert_eq!(report.sources[0].count, 2);
    assert_eq!(report.sources[0].warnings, ["alpha warning"]);

    let keys: Vec<_> = report
        .groups
        .iter()
        .map(|group| {
            let entry = group.primary_entry().unwrap();
            format!("{}:{}", entry.source, entry.key)
        })
        .collect();
    assert_eq!(keys, ["alpha:000", "alpha:001", "omega:000"]);
    // A source that fails after detection keeps its launchers; one that
    // panics or is stopped loses them.
    let launchers: Vec<_> = report.launchers.iter().map(|l| l.source.as_str()).collect();
    assert_eq!(launchers, ["alpha", "broken", "omega"]);
}

#[test]
fn cancellation_stops_the_scan() {
    let cancel = Cancel::new();
    cancel.cancel();
    let sources = [fake("polite", Behaviour::Cooperative)];
    let report = scan_with_sources(&env(), &config(10_000), &cancel, &sources).unwrap();
    assert_eq!(report.sources[0].error, Some(SourceError::Cancelled));
    assert!(report.groups.is_empty());
}

#[test]
fn selection_and_hosts_filter_sources() {
    let other_host = if cfg!(windows) {
        OsSet::LINUX
    } else {
        OsSet::WINDOWS
    };
    let sources: [Arc<dyn Source>; 3] = [
        fake("alpha", Behaviour::Games(1)),
        fake("omega", Behaviour::Games(1)),
        Arc::new(Fake {
            id: "foreign",
            hosts: other_host,
            behaviour: Behaviour::Games(1),
        }),
    ];
    let config = Config {
        sources: SourceSelection::only([SourceId::new("omega"), SourceId::new("foreign")]),
        ..Config::default()
    };
    let report = scan_with_sources(&env(), &config, &Cancel::new(), &sources).unwrap();
    let ids: Vec<_> = report.sources.iter().map(|r| r.source.as_str()).collect();
    assert_eq!(ids, ["omega"]);

    let launchers = detect_launchers_with_sources(&env(), &sources);
    let ids: Vec<_> = launchers.iter().map(|l| l.source.as_str()).collect();
    assert_eq!(ids, ["alpha", "omega"]);
}

#[test]
fn repeated_fixture_scans_are_byte_identical() {
    let machine = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/machines/linux-mixed"
    );
    let sources = [
        fake("omega", Behaviour::Games(3)),
        fake("boom", Behaviour::Panic),
        fake("alpha", Behaviour::Games(2)),
    ];
    let run = || {
        // A fresh fixture env per scan, each with its own frozen clock.
        let env = HostEnv::fixture(machine).unwrap();
        let report = scan_with_sources(&env, &config(5_000), &Cancel::new(), &sources).unwrap();
        serde_json::to_string(&report).unwrap()
    };
    let first = run();
    for _ in 0..5 {
        assert_eq!(run(), first);
    }
}

#[test]
fn config_defaults_roundtrip() {
    let config = Config::default();
    let json = serde_json::to_string(&config).unwrap();
    assert_eq!(serde_json::from_str::<Config>(&json).unwrap(), config);
    assert_eq!(serde_json::from_str::<Config>("{}").unwrap(), config);
}

#[test]
fn source_work_is_bounded_by_the_host_and_all_queued_sources_run() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Counted {
        id: SourceId,
        active: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
    }
    impl Source for Counted {
        fn id(&self) -> SourceId {
            self.id.clone()
        }
        fn hosts(&self) -> OsSet {
            OsSet::ALL
        }
        fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
            vec![]
        }
        fn watch_paths(&self, _: &HostEnv) -> Vec<WatchPath> {
            vec![]
        }
        fn scan(&self, _: &HostEnv, _: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(10));
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(vec![DetectedGame::new(self.id(), "one", self.id.as_str())])
        }
    }
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let sources: Vec<Arc<dyn Source>> = (0..12)
        .map(|n| {
            Arc::new(Counted {
                id: SourceId::from(format!("source-{n}")),
                active: active.clone(),
                peak: peak.clone(),
            }) as Arc<dyn Source>
        })
        .collect();
    let report = scan_with_sources(&env(), &config(1000), &Cancel::new(), &sources).unwrap();
    assert_eq!(report.sources.len(), 12);
    assert!(
        report
            .sources
            .iter()
            .all(|source| source.error.is_none() && source.count == 1)
    );
    let limit = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(2);
    assert!(peak.load(Ordering::SeqCst) <= limit);
    assert_eq!(active.load(Ordering::SeqCst), 0);
}
