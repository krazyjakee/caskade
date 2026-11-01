use super::*;
use std::sync::Mutex;

/// A point whose payload is the list of steps that ran.
struct Trace;

impl InterceptPoint for Trace {
    type Payload = Vec<String>;
    const NAME: &'static str = "test.trace";
}

/// Appends its name, or does something worse.
enum Step {
    Append(&'static str),
    Cancel(&'static str),
    Fail,
    Panic,
    Sleep(Duration),
}

#[async_trait]
impl Interceptor<Trace> for Step {
    async fn intercept(&self, mut payload: Vec<String>) -> Result<Intercept<Vec<String>>, String> {
        match self {
            Step::Append(name) => {
                payload.push((*name).to_owned());
                Ok(Intercept::Continue(payload))
            }
            Step::Cancel(reason) => Ok(Intercept::Cancel((*reason).to_owned())),
            Step::Fail => Err("refused".into()),
            Step::Panic => panic!("interceptor bug"),
            Step::Sleep(duration) => {
                tokio::time::sleep(*duration).await;
                payload.push("late".into());
                Ok(Intercept::Continue(payload))
            }
        }
    }
}

fn quick() -> Bus {
    Bus::new(BusOptions {
        subscriber_timeout: Duration::from_millis(100),
        interceptor_timeout: Duration::from_millis(100),
        stream_capacity: 16,
    })
}

#[tokio::test]
async fn interceptors_run_by_priority_then_registration_and_rewrite() {
    let bus = quick();
    bus.intercept::<Trace>("c", 10, Arc::new(Step::Append("c")));
    bus.intercept::<Trace>("a", -5, Arc::new(Step::Append("a")));
    bus.intercept::<Trace>("b1", 0, Arc::new(Step::Append("b1")));
    bus.intercept::<Trace>("b2", 0, Arc::new(Step::Append("b2")));
    let report = bus.run::<Trace>(vec!["start".into()]).await;
    assert_eq!(
        report.outcome,
        Outcome::Continue(vec![
            "start".into(),
            "a".into(),
            "b1".into(),
            "b2".into(),
            "c".into()
        ])
    );
    assert!(report.skipped.is_empty());
    // A point without interceptors passes the payload through.
    struct Empty;
    impl InterceptPoint for Empty {
        type Payload = u8;
        const NAME: &'static str = "test.empty";
    }
    assert_eq!(bus.run::<Empty>(7).await.outcome, Outcome::Continue(7));
}

#[tokio::test]
async fn cancel_stops_the_chain() {
    let bus = quick();
    bus.intercept::<Trace>("first", 0, Arc::new(Step::Append("first")));
    bus.intercept::<Trace>("guard", 1, Arc::new(Step::Cancel("not today")));
    bus.intercept::<Trace>("never", 2, Arc::new(Step::Append("never")));
    let report = bus.run::<Trace>(Vec::new()).await;
    assert_eq!(
        report.outcome,
        Outcome::Cancelled {
            by: "guard".into(),
            reason: "not today".into()
        }
    );
}

#[tokio::test]
async fn failing_panicking_and_slow_interceptors_are_skipped() {
    let bus = quick();
    bus.intercept::<Trace>("one", 0, Arc::new(Step::Append("one")));
    bus.intercept::<Trace>("fails", 1, Arc::new(Step::Fail));
    bus.intercept::<Trace>("panics", 2, Arc::new(Step::Panic));
    bus.intercept::<Trace>("slow", 3, Arc::new(Step::Sleep(Duration::from_secs(60))));
    bus.intercept::<Trace>("two", 4, Arc::new(Step::Append("two")));
    let started = std::time::Instant::now();
    let report = bus.run::<Trace>(Vec::new()).await;
    // The slow one was cut off at its 100 ms limit. The bound is far from
    // both, so a loaded machine can't fail it, and a missed timeout still
    // would (#194: it failed under a full parallel test run).
    assert!(started.elapsed() < Duration::from_secs(30));
    assert_eq!(
        report.outcome,
        Outcome::Continue(vec!["one".into(), "two".into()])
    );
    let skipped: Vec<_> = report
        .skipped
        .iter()
        .map(|(id, why)| (id.as_str(), why.as_str()))
        .collect();
    assert_eq!(
        skipped,
        [
            ("fails", "refused"),
            ("panics", "panicked"),
            ("slow", "timed out after 100 ms")
        ]
    );
}

/// A healthy interceptor can wait behind a longer background job.
#[tokio::test]
async fn resource_queue_waiting_does_not_consume_an_interceptors_time_box() {
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = tokio::sync::oneshot::channel();
    let occupied = tokio::spawn(async {
        caskade_jobs::global()
            .run("Other background work", async {
                entered.send(()).unwrap();
                wait.await.unwrap();
            })
            .await;
    });
    ready.await.unwrap();
    let bus = quick();
    bus.intercept::<Trace>("healthy", 0, Arc::new(Step::Append("done")));
    let step = tokio::spawn(async move { bus.run::<Trace>(Vec::new()).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!step.is_finished(), "the step is queued, not timed out");
    release.send(()).unwrap();
    occupied.await.unwrap();
    let result = step.await.unwrap();
    assert!(result.skipped.is_empty());
    assert_eq!(result.outcome, Outcome::Continue(vec!["done".into()]));
}

#[tokio::test]
async fn cold_backend_preparation_has_its_own_budget() {
    struct Cold;
    #[async_trait]
    impl Interceptor<Trace> for Cold {
        async fn prepare(&self) -> Result<(), String> {
            tokio::time::sleep(Duration::from_millis(150)).await;
            Ok(())
        }
        async fn intercept(
            &self,
            mut input: Vec<String>,
        ) -> Result<Intercept<Vec<String>>, String> {
            input.push("ready".into());
            Ok(Intercept::Continue(input))
        }
    }
    let bus = quick();
    bus.intercept::<Trace>("cold", 0, Arc::new(Cold));
    let result = bus.run::<Trace>(Vec::new()).await;
    assert!(result.skipped.is_empty());
    assert_eq!(result.outcome, Outcome::Continue(vec!["ready".into()]));
}

/// Records events; `fail`, `panic` or `hang` misbehave.
struct Recorder {
    seen: Arc<Mutex<Vec<&'static str>>>,
    mode: &'static str,
}

#[async_trait]
impl EventHandler for Recorder {
    async fn on_event(&self, event: &Event) {
        match self.mode {
            "panic" => panic!("subscriber bug"),
            "hang" => tokio::time::sleep(Duration::from_secs(5)).await,
            _ => self.seen.lock().unwrap().push(event.name()),
        }
    }
}

#[tokio::test]
async fn a_bad_subscriber_cannot_break_delivery() {
    let bus = quick();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = |mode| {
        Arc::new(Recorder {
            seen: seen.clone(),
            mode,
        })
    };
    bus.subscribe("panics", EventFilter::All, recorder("panic"));
    bus.subscribe("hangs", EventFilter::All, recorder("hang"));
    let good = bus.subscribe("good", EventFilter::All, recorder("ok"));
    bus.subscribe(
        "only-exits",
        EventFilter::Names(vec!["game.exited"]),
        recorder("ok"),
    );

    let started = std::time::Instant::now();
    let report = bus.emit_and_wait(Event::AppReady).await;
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(report.delivered, ["good"]);
    let mut failed = report.failed.clone();
    failed.sort();
    assert_eq!(
        failed,
        [
            ("hangs".to_owned(), "timed out after 100 ms".to_owned()),
            ("panics".to_owned(), "panicked".to_owned())
        ]
    );
    assert_eq!(*seen.lock().unwrap(), ["app.ready"]);

    // Fire-and-forget delivery survives the same subscribers.
    bus.unsubscribe(good);
    let session = GameSession {
        game_id: "g".into(),
        entry_id: "e".into(),
        session_id: "s".into(),
        started_at_ms: 1,
        ended_at_ms: Some(2),
        duration_ms: 1,
    };
    bus.emit(Event::GameExited(session));
    // Other tests/services can occupy the shared resource queue. Delivery is
    // asynchronous; its execution time box starts only after admission.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if seen.lock().unwrap().contains(&"game.exited") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("queued event was not delivered");
    assert_eq!(*seen.lock().unwrap(), ["app.ready", "game.exited"]);
}

#[tokio::test]
async fn the_stream_sees_every_event() {
    let bus = quick();
    let mut stream = bus.stream();
    bus.emit(Event::SettingsChanged {
        namespace: "library".into(),
        version: 2,
    });
    bus.emit(Event::LibraryGameAdded {
        game_id: "g1".into(),
    });
    assert_eq!(stream.recv().await.unwrap().name(), "settings.changed");
    let added = stream.recv().await.unwrap();
    assert_eq!(
        added,
        Event::LibraryGameAdded {
            game_id: "g1".into()
        }
    );
    assert_eq!(
        serde_json::to_value(&added).unwrap(),
        serde_json::json!({ "type": "libraryGameAdded", "value": { "game_id": "g1" } })
    );
}

/// A store that keeps whatever survives `library.import`.
async fn import(bus: &Bus, entries: Vec<DetectedGame>) -> Vec<DetectedGame> {
    let mut stored = Vec::new();
    for entry in entries {
        if let Outcome::Continue(entry) = bus.run::<LibraryImport>(entry).await.outcome {
            stored.push(entry);
        }
    }
    stored
}

struct Tidy;

#[async_trait]
impl Interceptor<LibraryImport> for Tidy {
    async fn intercept(&self, mut game: DetectedGame) -> Result<Intercept<DetectedGame>, String> {
        if game.title.contains("Demo") {
            return Ok(Intercept::Cancel("demos are not imported".into()));
        }
        game.title = game.title.trim_end_matches("™").to_owned();
        Ok(Intercept::Continue(game))
    }
}

#[tokio::test]
async fn library_import_can_rewrite_or_drop_an_entry_before_it_is_stored() {
    let bus = quick();
    bus.intercept::<LibraryImport>("tidy", 0, Arc::new(Tidy));
    let source = caskade_discovery::SourceId::new("steam");
    let stored = import(
        &bus,
        vec![
            DetectedGame::new(source.clone(), "1", "Portal™"),
            DetectedGame::new(source.clone(), "2", "Portal Demo"),
            DetectedGame::new(source, "3", "Celeste"),
        ],
    )
    .await;
    let titles: Vec<_> = stored.iter().map(|game| game.title.as_str()).collect();
    assert_eq!(titles, ["Portal", "Celeste"]);
}

struct Folder;

#[async_trait]
impl LibrarySource for Folder {
    fn id(&self) -> &str {
        "folder"
    }
    async fn scan(&self) -> Result<Vec<DetectedGame>, String> {
        Ok(vec![DetectedGame::new(
            caskade_discovery::SourceId::new("folder"),
            "x",
            "X",
        )])
    }
}

#[tokio::test]
async fn providers_are_registered_and_listed() {
    let bus = Bus::default();
    assert!(bus.library_sources().is_empty());
    bus.register_library_source(Arc::new(Folder));
    let sources = bus.library_sources();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id(), "folder");
    assert_eq!(sources[0].scan().await.unwrap().len(), 1);
    assert!(bus.install_handlers().is_empty() && bus.url_resolvers().is_empty());
}

/// Records whether it ever finished.
struct Late(Arc<std::sync::atomic::AtomicBool>);

#[async_trait]
impl Interceptor<Trace> for Late {
    async fn intercept(&self, payload: Vec<String>) -> Result<Intercept<Vec<String>>, String> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(Intercept::Continue(payload))
    }
}

#[tokio::test]
async fn a_timed_out_interceptor_is_stopped() {
    let bus = quick();
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    bus.intercept::<Trace>("late", 0, Arc::new(Late(finished.clone())));
    let report = bus.run::<Trace>(Vec::new()).await;
    assert_eq!(report.skipped.len(), 1);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !finished.load(std::sync::atomic::Ordering::SeqCst),
        "the late step kept running"
    );
}

#[tokio::test]
async fn removing_an_interceptor_id_takes_its_steps_off_every_point() {
    struct Other;
    impl InterceptPoint for Other {
        type Payload = Vec<String>;
        const NAME: &'static str = "test.other";
    }
    #[async_trait]
    impl Interceptor<Other> for Step {
        async fn intercept(&self, payload: Vec<String>) -> Result<Intercept<Vec<String>>, String> {
            Interceptor::<Trace>::intercept(self, payload).await
        }
    }
    let bus = quick();
    bus.intercept::<Trace>("plugin", 0, Arc::new(Step::Append("plugin")));
    bus.intercept::<Other>("plugin", 0, Arc::new(Step::Append("plugin")));
    bus.intercept::<Trace>("core", 1, Arc::new(Step::Append("core")));
    assert_eq!(bus.remove_interceptors("plugin"), 2);
    assert_eq!(bus.remove_interceptors("plugin"), 0);
    assert_eq!(
        bus.run::<Trace>(Vec::new()).await.outcome,
        Outcome::Continue(vec!["core".to_owned()])
    );
    assert_eq!(
        bus.run::<Other>(Vec::new()).await.outcome,
        Outcome::Continue(Vec::new())
    );
}
