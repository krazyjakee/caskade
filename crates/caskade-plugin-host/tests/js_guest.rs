//! #132 spike: a TypeScript backend built with `jco componentize`
//! (docs/research/js-backend-spike/) against the same checks as the Rust
//! test guest, plus the measurements in docs/research/js-backend-spike.md.
//!
//! Opt-in, because building the JS guest needs Node and jco:
//! `CASKADE_JS_GUEST=/path/plugin.wasm cargo test -p caskade-plugin-host --test it js_guest:: -- --ignored --nocapture`

use caskade_core::{
    bus::{Bus, Event},
    db::LibraryStore,
};
use caskade_plugin_host::{
    CallError, HostError, HostOptions, InterceptResult, Limits, PluginHost, PluginServices,
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const ID: &str = "dev.caskade.spike";

fn js_guest() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CASKADE_JS_GUEST")
            .expect("set CASKADE_JS_GUEST to the componentized JS guest"),
    )
}

struct Setup {
    _temp: tempfile::TempDir,
    store: LibraryStore,
    bus: Bus,
    host: PluginHost,
}

async fn setup(backend: &Path, limits: Limits) -> Setup {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("plugins/spike");
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::copy(backend, dir.join("dist/backend.wasm")).unwrap();
    let manifest = serde_json::json!({
        "id": ID,
        "name": "Spike",
        "version": "0.1.0",
        "api": "^0.1",
        "backend": "dist/backend.wasm",
        "permissions": {"network": ["127.0.0.1"]},
        "activation": ["onRequest"],
        "contributes": {"hooks": ["game.exited", "launch.before"]}
    });
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let host = PluginHost::new(
        PluginServices {
            store: store.clone(),
            bus: Some(bus.clone()),
            settings: None,
        },
        HostOptions {
            folders: vec![temp.path().join("plugins")],
            data: temp.path().join("plugin-data"),
            limits,
        },
    )
    .unwrap();
    host.discover().await.unwrap();
    host.set_enabled(ID, true).await.unwrap();
    Setup {
        _temp: temp,
        store,
        bus,
        host,
    }
}

/// The checks tests/host.rs runs against the Rust guest.
async fn functional(backend: &Path) {
    let s = setup(backend, Limits::default()).await;
    let host = &s.host;
    assert_eq!(
        host.request(ID, "echo", r#"{"hi":1}"#).await.unwrap(),
        r#"{"hi":1}"#
    );
    assert_eq!(
        host.request(ID, "kv", "stored").await.unwrap(),
        "stored:true"
    );
    host.event(ID, "game.exited", r#"{"gameId":"g1"}"#)
        .await
        .unwrap();
    assert_eq!(
        s.store
            .plugin_kv_get(ID, "event:game.exited")
            .await
            .unwrap(),
        Some(br#"{"gameId":"g1"}"#.to_vec())
    );
    let mut stream = s.bus.stream();
    host.request(ID, "emit", r#"{"n":2}"#).await.unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), stream.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        event,
        Event::PluginEvent {
            plugin: ID.into(),
            name: "greeted".into(),
            payload: r#"{"n":2}"#.into(),
        }
    );
    assert_eq!(
        host.intercept(ID, "launch.before", r#"{"program":"game"}"#)
            .await
            .unwrap(),
        InterceptResult::Continue(r#"{"program":"gamemoderun game"}"#.into())
    );
    assert!(matches!(
        host.intercept(ID, "launch.before", r#"{"cancel-me":1}"#).await.unwrap(),
        InterceptResult::Cancel(reason) if reason.contains("not today")
    ));
    assert!(matches!(
        host.request(ID, "nope", "").await,
        Err(HostError::Plugin { code, .. }) if code == "unknown_method"
    ));
}

/// Loops, memory hogs and exceptions, each failing only its call.
async fn contained(backend: &Path) -> Vec<String> {
    let s = setup(
        backend,
        Limits {
            memory: 64 << 20,
            call_timeout: Duration::from_millis(300),
            ..Limits::default()
        },
    )
    .await;
    let host = &s.host;
    let mut seen = Vec::new();
    for method in ["spin", "grow", "trap"] {
        let started = Instant::now();
        let outcome = match host.request(ID, method, "").await {
            Err(HostError::Call { source, .. }) => match source {
                CallError::TimedOut(_) => "timed out".to_owned(),
                CallError::MemoryLimit => "memory limit".to_owned(),
                CallError::Trap(_) => "trap".to_owned(),
                other => format!("{other}"),
            },
            other => format!("{other:?}"),
        };
        seen.push(format!("{method}: {outcome} in {:?}", started.elapsed()));
        assert_eq!(host.request(ID, "echo", "after").await.unwrap(), "after");
    }
    seen
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// Size, compile + instantiate, instantiate alone, per-call latency and the
/// smallest memory limit the guest starts under.
async fn measure(label: &str, backend: &Path) {
    let size = std::fs::metadata(backend).unwrap().len();
    let s = setup(backend, Limits::default()).await;
    let started = Instant::now();
    s.host.activate(ID).await.unwrap();
    let cold = started.elapsed();
    let mut warm = Vec::new();
    for _ in 0..5 {
        s.host.deactivate(ID).await.unwrap();
        let started = Instant::now();
        s.host.activate(ID).await.unwrap();
        warm.push(started.elapsed());
    }
    warm.sort();
    let mut calls = Vec::new();
    for i in 0..1000 {
        let payload = format!(r#"{{"i":{i}}}"#);
        let started = Instant::now();
        s.host.request(ID, "echo", &payload).await.unwrap();
        calls.push(started.elapsed());
    }
    calls.sort();
    let mut intercepts = Vec::new();
    for _ in 0..200 {
        let started = Instant::now();
        s.host
            .intercept(ID, "launch.before", r#"{"program":"game"}"#)
            .await
            .unwrap();
        intercepts.push(started.elapsed());
    }
    intercepts.sort();
    // The smallest limit (in MiB) under which the guest instantiates and answers.
    let mut floor = None;
    for mib in [1u64, 2, 4, 8, 12, 16, 24, 32, 48, 64] {
        let s = setup(
            backend,
            Limits {
                memory: (mib << 20) as usize,
                call_timeout: Duration::from_secs(2),
                ..Limits::default()
            },
        )
        .await;
        if s.host.request(ID, "echo", "x").await.is_ok() {
            floor = Some(mib);
            break;
        }
    }
    println!(
        "{label}: size {:.2} MiB; compile+instantiate {cold:?}; instantiate median {:?}; \
         echo median {:?} p95 {:?}; intercept median {:?}; memory floor {floor:?} MiB",
        size as f64 / f64::from(1 << 20),
        warm[warm.len() / 2],
        percentile(&calls, 0.5),
        percentile(&calls, 0.95),
        percentile(&intercepts, 0.5),
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs CASKADE_JS_GUEST, a jco-built component"]
async fn js_guest_passes_the_rust_guest_checks_and_is_measured() {
    let js = js_guest();
    functional(&js).await;
    println!("js containment: {:?}", contained(&js).await);
    let s = setup(&js, Limits::default()).await;
    println!(
        "js fetch from a synchronous export: {:?}",
        s.host.request(ID, "fetch", "http://127.0.0.1:9/").await
    );
    let rust = crate::guest();
    println!("rust containment: {:?}", contained(rust).await);
    measure("rust", rust).await;
    measure("js", &js).await;
}
