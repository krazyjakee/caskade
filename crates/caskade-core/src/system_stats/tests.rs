use super::gpu::{DrmCard, SmiRow, SmiView, drm_cards, merge, parse_smi_line, pci_slot};
use super::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

const GIB: u64 = 1024 * 1024 * 1024;

fn disk(mount: &str, device: &str, fs: &str, total: u64, available: u64) -> RawDisk {
    RawDisk {
        name: String::new(),
        device: device.into(),
        mount: mount.into(),
        file_system: fs.into(),
        removable: false,
        total,
        available,
    }
}

fn interface(name: &str, kind: InterfaceKind, received: u64, transmitted: u64) -> RawInterface {
    RawInterface {
        name: name.into(),
        kind,
        received,
        transmitted,
    }
}

fn readings() -> Readings {
    Readings {
        cpu: Some(25.0),
        cores: 8,
        memory: Some((16 * GIB, 6 * GIB)),
        ..Readings::default()
    }
}

#[test]
fn cpu_and_memory_are_read_or_missing_never_zero() {
    let mut aggregator = Aggregator::default();
    let sample = aggregator.sample(1_000, Duration::ZERO, readings());
    assert_eq!(sample.cpu.usage, Some(25.0));
    assert_eq!(sample.cpu.cores, 8);
    assert_eq!(
        sample.memory,
        Some(MemorySample {
            total: 16 * GIB,
            used: 10 * GIB,
            available: 6 * GIB,
        })
    );
    assert_eq!(sample.at, 1_000);
    assert_eq!(sample.error, None);

    // No measurement yet, a broken reading, an impossible one.
    for (cpu, memory) in [
        (None, None),
        (Some(f32::NAN), Some((0, 0))),
        (Some(f32::INFINITY), Some((GIB, 2 * GIB))),
    ] {
        let sample = aggregator.sample(
            2_000,
            Duration::from_secs(1),
            Readings {
                cpu,
                memory,
                ..readings()
            },
        );
        assert_eq!(sample.cpu.usage, None);
        assert_eq!(sample.memory, None);
    }
    let over = aggregator.sample(
        3_000,
        Duration::from_secs(2),
        Readings {
            cpu: Some(100.4),
            ..readings()
        },
    );
    assert_eq!(over.cpu.usage, Some(100.0));
}

#[test]
fn a_failed_sample_is_a_gap_with_its_reason() {
    let sample = Aggregator::failed(5_000, "This sample couldn't be read.");
    assert_eq!(
        sample.error.as_deref(),
        Some("This sample couldn't be read.")
    );
    assert_eq!(sample.cpu.usage, None);
    assert_eq!(sample.memory, None);
    assert!(sample.disks.is_empty() && sample.gpus.devices.is_empty());
    assert_eq!(sample.network.received, None);
}

#[test]
fn volumes_count_each_filesystem_once_and_skip_pseudo_ones() {
    let mut removable = disk(
        "/run/media/player/SD Card",
        "/dev/mmcblk0p1",
        "exfat",
        64 * GIB,
        60 * GIB,
    );
    removable.name = "SD Card".into();
    removable.removable = true;
    let raw = vec![
        disk("/home", "/dev/nvme0n1p2", "btrfs", 500 * GIB, 200 * GIB),
        disk("/", "/dev/nvme0n1p2", "btrfs", 500 * GIB, 200 * GIB),
        disk("/boot/efi", "/dev/nvme0n1p1", "vfat", GIB / 2, GIB / 4),
        disk("/mnt/games", "/dev/sda1", "ext4", 2000 * GIB, 900 * GIB),
        removable,
        disk("/tmp", "tmpfs", "tmpfs", 8 * GIB, 8 * GIB),
        disk(
            "/var/lib/docker/overlay2/x/merged",
            "overlay",
            "overlay",
            500 * GIB,
            200 * GIB,
        ),
        disk("/snap/core22/1380", "/dev/loop3", "squashfs", GIB / 10, 0),
        disk(
            "/tmp/.mount_CaskadXYZ",
            "Caskade.AppImage",
            "fuse.Caskade.AppImage",
            GIB,
            0,
        ),
        disk("/mnt/empty", "/dev/sdz1", "ext4", 0, 0),
    ];
    let volumes = super::aggregate::volumes(&raw);
    let mounts: Vec<&str> = volumes.iter().map(|volume| volume.mount.as_str()).collect();
    assert_eq!(
        mounts,
        ["/", "/boot/efi", "/mnt/games", "/run/media/player/SD Card"]
    );
    let root = &volumes[0];
    assert_eq!(root.other_mounts, ["/home"]);
    assert_eq!(root.name, "System");
    assert_eq!((root.total, root.available), (500 * GIB, 200 * GIB));
    assert_eq!(volumes[2].name, "games");
    let card = &volumes[3];
    assert_eq!((card.name.as_str(), card.removable), ("SD Card", true));
}

#[test]
fn volumes_without_devices_are_told_apart_by_label_and_capacity() {
    let named = |name: &str, mount: &str, total: u64, available: u64| RawDisk {
        name: name.into(),
        ..disk(mount, "", "apfs", total, available)
    };
    // macOS lists the system volume twice; Windows drives have letters.
    let raw = vec![
        named("Macintosh HD", "/", 500 * GIB, 100 * GIB),
        named("Macintosh HD", "/System/Volumes/Data", 500 * GIB, 100 * GIB),
        named(
            "Macintosh HD",
            "/Volumes/Macintosh HD",
            500 * GIB,
            100 * GIB,
        ),
        named("Backup", "/Volumes/Backup", 1000 * GIB, 10 * GIB),
        named("", "C:\\", 250 * GIB, 50 * GIB),
        named("", "D:\\", 250 * GIB, 50 * GIB),
        named("Games", "E:\\", 250 * GIB, 50 * GIB),
    ];
    let volumes = super::aggregate::volumes(&raw);
    let shown: Vec<(&str, &str)> = volumes
        .iter()
        .map(|volume| (volume.name.as_str(), volume.mount.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("Macintosh HD", "/"),
            ("Backup", "/Volumes/Backup"),
            ("Local disk", "C:\\"),
            ("Local disk", "D:\\"),
            ("Games", "E:\\"),
        ]
    );
    assert_eq!(volumes[0].other_mounts, ["/Volumes/Macintosh HD"]);
}

#[test]
fn network_rates_come_from_counters_and_add_up_physical_interfaces_only() {
    let mut aggregator = Aggregator::default();
    let first = aggregator.sample(
        0,
        Duration::from_secs(10),
        Readings {
            interfaces: vec![
                interface("eth0", InterfaceKind::Physical, 1_000, 500),
                interface("wlan0", InterfaceKind::Physical, 0, 0),
                interface("lo", InterfaceKind::Loopback, 0, 0),
                interface("tailscale0", InterfaceKind::Virtual, 0, 0),
            ],
            ..readings()
        },
    );
    // One reading isn't a rate.
    assert!(
        first
            .network
            .interfaces
            .iter()
            .all(|i| i.received.is_none() && i.transmitted.is_none())
    );
    assert_eq!(
        (first.network.received, first.network.transmitted),
        (None, None)
    );

    let second = aggregator.sample(
        2_000,
        Duration::from_secs(12),
        Readings {
            interfaces: vec![
                interface("tailscale0", InterfaceKind::Virtual, 4_000, 4_000),
                interface("lo", InterfaceKind::Loopback, 9_000_000, 9_000_000),
                interface("wlan0", InterfaceKind::Physical, 3_000, 1_000),
                interface("eth0", InterfaceKind::Physical, 5_000, 2_500),
                // Plugged in since: no rate yet.
                interface("usb0", InterfaceKind::Physical, 7_000, 7_000),
            ],
            ..readings()
        },
    );
    let rates: Vec<(&str, Option<f64>, Option<f64>)> = second
        .network
        .interfaces
        .iter()
        .map(|i| (i.name.as_str(), i.received, i.transmitted))
        .collect();
    assert_eq!(
        rates,
        [
            ("eth0", Some(2_000.0), Some(1_000.0)),
            ("usb0", None, None),
            ("wlan0", Some(1_500.0), Some(500.0)),
            ("tailscale0", Some(2_000.0), Some(2_000.0)),
            ("lo", Some(4_500_000.0), Some(4_500_000.0)),
        ]
    );
    // The tunnel's and loopback's traffic isn't counted twice.
    assert_eq!(second.network.received, Some(3_500.0));
    assert_eq!(second.network.transmitted, Some(1_500.0));

    // eth0's counters reset and wlan0 went away.
    let third = aggregator.sample(
        3_000,
        Duration::from_secs(13),
        Readings {
            interfaces: vec![
                interface("eth0", InterfaceKind::Physical, 100, 100),
                interface("usb0", InterfaceKind::Physical, 8_000, 7_500),
            ],
            ..readings()
        },
    );
    let eth0 = &third.network.interfaces[0];
    assert_eq!((eth0.received, eth0.transmitted), (None, None));
    assert_eq!(third.network.received, Some(1_000.0));
    assert_eq!(third.network.transmitted, Some(500.0));
    assert_eq!(third.network.interfaces.len(), 2);
}

#[test]
fn interfaces_are_classified_by_name_where_the_os_does_not_say() {
    use InterfaceKind::*;
    for (name, no_mac, kind) in [
        ("Ethernet", false, Physical),
        ("Wi-Fi", false, Physical),
        ("en0", false, Physical),
        ("lo0", true, Loopback),
        ("Loopback Pseudo-Interface 1", true, Loopback),
        ("vEthernet (WSL)", false, Virtual),
        ("VirtualBox Host-Only Network", false, Virtual),
        ("utun3", true, Virtual),
        ("awdl0", false, Virtual),
        ("ap1", false, Virtual),
        ("bridge100", false, Virtual),
        ("docker0", false, Virtual),
        ("wg0", true, Virtual),
        ("Tailscale", false, Virtual),
        ("SomeTunnel", true, Virtual),
    ] {
        assert_eq!(classify_interface(name, no_mac), kind, "{name}");
    }
}

#[test]
fn nvidia_smi_lines_parse_and_messages_do_not() {
    assert_eq!(
        parse_smi_line("0, 00000000:01:00.0, NVIDIA GeForce RTX 3080, 37, 2048, 10240"),
        Some(SmiRow {
            index: 0,
            slot: "01:00.0".into(),
            name: "NVIDIA GeForce RTX 3080".into(),
            usage: Some(37.0),
            memory_used: Some(2048 * 1024 * 1024),
            memory_total: Some(10240 * 1024 * 1024),
        })
    );
    let unsupported =
        parse_smi_line("1, 00000000:02:00.0, Tesla, Inc., [N/A], [Not Supported], 0").unwrap();
    assert_eq!(unsupported.name, "Tesla, Inc.");
    assert_eq!(
        (
            unsupported.usage,
            unsupported.memory_used,
            unsupported.memory_total
        ),
        (None, None, None)
    );
    assert_eq!(
        parse_smi_line(
            "NVIDIA-SMI has failed because it couldn't communicate with the NVIDIA driver."
        ),
        None
    );
    assert_eq!(parse_smi_line(""), None);
    assert_eq!(pci_slot("0000:0A:00.0"), "0a:00.0");
}

fn write(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn drm_cards_are_read_from_sysfs() {
    let drm = tempfile::tempdir().unwrap();
    let card = |name: &str, file: &str, text: &str| {
        write(&drm.path().join(name).join("device").join(file), text)
    };
    card("card1", "vendor", "0x1002\n");
    card(
        "card1",
        "uevent",
        "DRIVER=amdgpu\nPCI_SLOT_NAME=0000:03:00.0\n",
    );
    card("card1", "gpu_busy_percent", "37\n");
    card("card1", "mem_info_vram_used", "1073741824\n");
    card("card1", "mem_info_vram_total", "8589934592\n");
    card("card1", "product_name", "Radeon RX 6700 XT\n");
    card("card0", "vendor", "0x8086\n");
    card("card0", "uevent", "PCI_SLOT_NAME=0000:00:02.0\n");
    card("card2", "vendor", "0x10de\n");
    card("card2", "uevent", "PCI_SLOT_NAME=0000:01:00.0\n");
    // A connector, a render node, a card without a PCI vendor (simpledrm).
    card("card1-DP-1", "vendor", "0x1002\n");
    card("renderD128", "vendor", "0x1002\n");
    std::fs::create_dir_all(drm.path().join("card3/device")).unwrap();

    let cards = drm_cards(drm.path());
    let ids: Vec<&str> = cards.iter().map(|card| card.id.as_str()).collect();
    assert_eq!(ids, ["card0", "card1", "card2"]);
    assert_eq!(
        cards[1],
        DrmCard {
            id: "card1".into(),
            slot: Some("03:00.0".into()),
            vendor: GpuVendor::Amd,
            name: Some("Radeon RX 6700 XT".into()),
            usage: Some(37.0),
            memory_used: Some(GIB),
            memory_total: Some(8 * GIB),
        }
    );
    assert_eq!(cards[0].usage, None);
    assert!(drm_cards(&drm.path().join("missing")).is_empty());
}

fn drm(id: &str, vendor: GpuVendor, slot: &str, usage: Option<f32>) -> DrmCard {
    DrmCard {
        id: id.into(),
        slot: Some(slot.into()),
        vendor,
        name: None,
        usage,
        memory_used: None,
        memory_total: None,
    }
}

fn row(index: u32, slot: &str, usage: Option<f32>) -> SmiRow {
    SmiRow {
        index,
        slot: slot.into(),
        name: format!("NVIDIA GPU {index}"),
        usage,
        memory_used: Some(GIB),
        memory_total: Some(4 * GIB),
    }
}

#[test]
fn gpus_merge_sysfs_and_nvidia_smi_and_say_why_usage_is_missing() {
    let cards = vec![
        drm("card0", GpuVendor::Intel, "00:02.0", None),
        drm("card1", GpuVendor::Amd, "03:00.0", Some(12.0)),
        drm("card2", GpuVendor::Nvidia, "01:00.0", None),
        drm("card3", GpuVendor::Nvidia, "02:00.0", None),
    ];
    let smi = SmiView {
        rows: vec![row(1, "02:00.0", Some(80.0)), row(0, "01:00.0", None)],
        problem: None,
    };
    let gpus = merge(cards.clone(), &smi);
    let shown: Vec<(&str, &str, Option<f32>, bool)> = gpus
        .iter()
        .map(|gpu| {
            (
                gpu.id.as_str(),
                gpu.name.as_str(),
                gpu.usage,
                gpu.unavailable.is_some(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("card0", "Intel GPU", None, true),
            ("card1", "AMD GPU", Some(12.0), false),
            ("card2", "NVIDIA GPU 0", None, true),
            ("card3", "NVIDIA GPU 1", Some(80.0), false),
        ]
    );
    assert_eq!(gpus[3].memory_total, Some(4 * GIB));

    // nvidia-smi missing: the NVIDIA cards say so, the others still read.
    let missing = SmiView {
        rows: Vec::new(),
        problem: Some("Install NVIDIA's nvidia-smi to see this GPU's usage.".into()),
    };
    let gpus = merge(cards, &missing);
    assert_eq!(gpus[1].usage, Some(12.0));
    assert_eq!(gpus[2].unavailable, missing.problem);

    // Windows: no sysfs, every row is a GPU of its own.
    let gpus = merge(Vec::new(), &smi);
    let ids: Vec<&str> = gpus.iter().map(|gpu| gpu.id.as_str()).collect();
    assert_eq!(ids, ["nvidia1", "nvidia0"]);
}

#[cfg(target_os = "linux")]
#[test]
fn udev_label_escapes_are_decoded() {
    assert_eq!(super::probe::unescape_udev("SD\\x20Card"), "SD Card");
    assert_eq!(super::probe::unescape_udev("plain\\x"), "plain\\x");
}

/// Readings numbered by call; the `panic_on`th read panics.
struct FakeProbe {
    reads: Arc<AtomicU32>,
    panic_on: u32,
}

impl Probe for FakeProbe {
    fn read(&mut self) -> Readings {
        let n = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        assert_ne!(n, self.panic_on, "fake probe failure");
        Readings {
            cpu: Some(n as f32),
            ..readings()
        }
    }
}

fn fake(config: SamplerConfig, panic_on: u32) -> (SystemStats, Arc<AtomicU32>, Arc<AtomicU32>) {
    let reads = Arc::new(AtomicU32::new(0));
    let probes = Arc::new(AtomicU32::new(0));
    let (shared, made) = (reads.clone(), probes.clone());
    let stats = SystemStats::with_probe(config, move || {
        made.fetch_add(1, Ordering::SeqCst);
        Box::new(FakeProbe {
            reads: shared.clone(),
            panic_on,
        }) as Box<dyn Probe>
    });
    (stats, reads, probes)
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn quick(history: usize, lease: Duration) -> SamplerConfig {
    SamplerConfig {
        interval: Duration::from_millis(10),
        history,
        lease,
    }
}

#[tokio::test]
async fn samples_only_while_watched_and_keeps_a_bounded_history() {
    let (stats, reads, _) = fake(quick(5, Duration::from_secs(60)), 0);
    let mut samples = stats.subscribe();
    assert!(!stats.is_sampling());
    let snapshot = stats.watch("dashboard", true);
    assert_eq!((snapshot.interval_ms, snapshot.history), (10, 5));
    assert!(snapshot.samples.is_empty());
    let first = samples.recv().await.unwrap();
    let second = samples.recv().await.unwrap();
    assert_eq!(second.seq, first.seq + 1);
    assert_eq!(first.cpu.usage, Some(1.0));
    until("a full history", || reads.load(Ordering::SeqCst) >= 8).await;
    let history = stats.history();
    assert_eq!(history.len(), 5);
    assert!(
        history
            .windows(2)
            .all(|pair| pair[1].seq == pair[0].seq + 1)
    );

    // A second watcher; the sampler keeps going until both are gone.
    let renewed = stats.watch("other", false);
    assert!(renewed.samples.is_empty());
    stats.unwatch("dashboard");
    let before = reads.load(Ordering::SeqCst);
    until("more samples", || reads.load(Ordering::SeqCst) > before + 2).await;
    stats.unwatch("other");
    until("the sampler to stop", || !stats.is_sampling()).await;
    let stopped = reads.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(reads.load(Ordering::SeqCst), stopped);

    // A history older than its window (5 × 10 ms) isn't answered again.
    let again = stats.watch("dashboard", true);
    assert!(again.samples.is_empty());
    stats.unwatch("dashboard");
}

#[tokio::test]
async fn watching_again_answers_the_recent_history() {
    let (stats, reads, _) = fake(
        SamplerConfig {
            interval: Duration::from_millis(10),
            history: 1_000,
            lease: Duration::from_secs(60),
        },
        0,
    );
    stats.watch("dashboard", true);
    until("a few samples", || reads.load(Ordering::SeqCst) >= 3).await;
    stats.unwatch("dashboard");
    until("the sampler to stop", || !stats.is_sampling()).await;
    let kept = stats.history();
    let again = stats.watch("dashboard", true);
    assert!(kept.len() >= 2);
    assert_eq!(again.samples, kept);
    stats.unwatch("dashboard");
}

#[tokio::test]
async fn a_watch_that_is_not_renewed_runs_out() {
    let (stats, _, _) = fake(quick(5, Duration::from_millis(40)), 0);
    stats.watch("reloaded webview", false);
    assert!(stats.is_sampling());
    until("the lease to run out", || !stats.is_sampling()).await;
}

#[tokio::test]
async fn a_failed_read_is_a_gap_and_sampling_goes_on() {
    let (stats, _, probes) = fake(quick(10, Duration::from_secs(60)), 2);
    let mut samples = stats.subscribe();
    stats.watch("dashboard", false);
    let first = samples.recv().await.unwrap();
    let failed = samples.recv().await.unwrap();
    let third = samples.recv().await.unwrap();
    stats.unwatch("dashboard");
    assert_eq!(first.error, None);
    assert!(failed.error.is_some());
    assert_eq!(failed.cpu.usage, None);
    assert_eq!(failed.memory, None);
    assert_eq!(third.error, None);
    assert_eq!(third.cpu.usage, Some(3.0));
    // The panicking probe was replaced.
    assert_eq!(probes.load(Ordering::SeqCst), 2);
}
