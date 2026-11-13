//! Settings → Statistics (#105), over a sampler reading a fake machine,
//! as the webview's `statistics` tests: it goes live with the first
//! readings, the gauges and facts say what was read, the graphs summarise
//! it, the network counts physical interfaces only, a full drive warns;
//! the watch ends when the panel goes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::data::use_data_source;
use crate::frame::tests::{ONE, Window, source};
use crate::input::InputRoot;
use blitz_dom::DocumentConfig;
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_system_stats::{
    GpuSample, GpuSamples, GpuVendor, InterfaceKind, Probe, RawDisk, RawInterface, Readings,
    SamplerConfig, SystemStats,
};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::*;

/// A machine at 42 % CPU, half its 16 GiB used, one GPU, one network card
/// and a loopback moving bytes, and a drive 95 % full.
struct Fake {
    reads: Arc<AtomicU64>,
}

impl Probe for Fake {
    fn read(&mut self) -> Readings {
        let n = self.reads.fetch_add(1, Ordering::Relaxed);
        Readings {
            cpu: Some(42.0),
            cores: 8,
            memory: Some((16 << 30, 8 << 30)),
            gpus: GpuSamples {
                devices: vec![GpuSample {
                    id: "card1".into(),
                    name: "Radeon Test".into(),
                    vendor: GpuVendor::Amd,
                    usage: Some(10.0),
                    memory_used: Some(1 << 30),
                    memory_total: Some(8 << 30),
                    unavailable: None,
                }],
                unavailable: None,
            },
            disks: vec![RawDisk {
                name: "Games".into(),
                device: "/dev/sdb1".into(),
                mount: "/mnt/games".into(),
                file_system: "ext4".into(),
                removable: false,
                total: 1000 << 30,
                available: 50 << 30,
            }],
            interfaces: vec![
                RawInterface {
                    name: "eth0".into(),
                    kind: InterfaceKind::Physical,
                    received: n * 2048,
                    transmitted: n * 1024,
                },
                RawInterface {
                    name: "lo".into(),
                    kind: InterfaceKind::Loopback,
                    received: n * 9999,
                    transmitted: n * 9999,
                },
            ],
        }
    }
}

static STATS: Mutex<Option<SystemStats>> = Mutex::new(None);

fn app() -> Element {
    use_data_source(|| source().clone());
    let stats = STATS.lock().unwrap().clone().expect("a sampler");
    use_context_provider(move || stats);
    let mut shown = use_signal(|| true);
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            button { id: "close", class: "size-4", onclick: move |_| shown.set(false) }
            if shown() {
                crate::settings::statistics::StatisticsSettings {}
            }
        }
    }
}

#[test]
fn statistics_go_live_and_say_what_was_read() {
    let _one = caskade_core::lock(&ONE);
    let reads = Arc::new(AtomicU64::new(0));
    let stats = {
        let reads = reads.clone();
        SystemStats::with_probe(
            SamplerConfig {
                interval: Duration::from_millis(200),
                history: 30,
                lease: Duration::from_secs(30),
            },
            move || {
                Box::new(Fake {
                    reads: reads.clone(),
                }) as Box<dyn Probe>
            },
        )
    };
    source();
    *STATS.lock().unwrap() = Some(stats);
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        DocumentConfig {
            viewport: Some(Viewport::new(1280, 2000, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until("live", |window| {
        window
            .find("data-cs", "system-stats")
            .and_then(|node| window.attr(node, "data-status"))
            .as_deref()
            == Some("live")
            && window.find_all("data-part", "interface").len() == 2
    });
    let card = |window: &Window, metric: &str| {
        let node = window.get("data-metric", metric);
        window.text(node)
    };
    let cpu = card(&window, "cpu");
    assert!(cpu.contains("8 threads"), "{cpu}");
    let gauge = window.get("data-part", "gauge");
    assert_eq!(window.attr(gauge, "aria-valuenow").as_deref(), Some("42"));
    let memory = card(&window, "memory");
    assert!(memory.contains("16.0 GiB RAM"), "{memory}");
    assert!(memory.contains("8.0 GiB"), "{memory}");
    let gpu = card(&window, "gpu");
    assert!(gpu.starts_with("Radeon Test"), "{gpu}");
    assert!(gpu.contains("AMD · card1"), "{gpu}");
    assert!(gpu.contains("1.0 GiB of 8.0 GiB"), "{gpu}");
    let kinds: Vec<String> = window
        .find_all("data-part", "interface")
        .into_iter()
        .map(|node| window.attr(node, "data-kind").unwrap_or_default())
        .collect();
    assert_eq!(kinds, ["physical", "loopback"]);
    let storage = card(&window, "storage");
    assert!(storage.contains("almost out of space"), "{storage}");
    let graph = window
        .find_all("role", "img")
        .into_iter()
        .find_map(|node| window.attr(node, "aria-label"))
        .expect("a graph");
    assert!(
        graph.starts_with("CPU usage, last 6 s. Usage: now 42%"),
        "{graph}"
    );
    // Closing the panel ends its watch: the sampler stops reading.
    let close = window.get("id", "close");
    window.click(close);
    window.settle();
    std::thread::sleep(Duration::from_millis(500));
    let before = reads.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(800));
    assert!(
        reads.load(Ordering::Relaxed) <= before + 1,
        "sampling stopped with the panel"
    );
}
