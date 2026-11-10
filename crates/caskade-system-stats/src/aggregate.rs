//! Raw readings to a [`SystemSample`]: volumes filtered and counted once,
//! counters turned into rates, physical interfaces added up. Pure, so tests
//! feed it readings.

use super::{
    CpuSample, DiskSample, GpuSamples, InterfaceKind, InterfaceSample, MemorySample, NetworkSample,
    Readings, SystemSample,
};
use std::collections::HashMap;
use std::time::Duration;

/// A mounted volume as the OS lists it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawDisk {
    /// Its label; may be empty.
    pub name: String,
    /// The block device (Linux); empty where the OS names none.
    pub device: String,
    pub mount: String,
    pub file_system: String,
    pub removable: bool,
    pub total: u64,
    pub available: u64,
}

/// An interface's counters: bytes since it came up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawInterface {
    pub name: String,
    pub kind: InterfaceKind,
    pub received: u64,
    pub transmitted: u64,
}

/// Filesystems that aren't storage: kernel views, RAM disks, container
/// layers and read-only images (snaps, AppImages).
const PSEUDO_FILE_SYSTEMS: &[&str] = &[
    "autofs",
    "binfmt_misc",
    "bpf",
    "cgroup",
    "cgroup2",
    "configfs",
    "debugfs",
    "devfs",
    "devpts",
    "devtmpfs",
    "efivarfs",
    "fusectl",
    "hugetlbfs",
    "mqueue",
    "nsfs",
    "nullfs",
    "overlay",
    "proc",
    "pstore",
    "ramfs",
    "rpc_pipefs",
    "securityfs",
    "squashfs",
    "sysfs",
    "tmpfs",
    "tracefs",
    "fuse.gvfsd-fuse",
    "fuse.lxcfs",
    "fuse.portal",
    "fuse.snapfuse",
];

/// Mount points of the same kind. A trailing `/` or `_` matches any name
/// under it; otherwise the path and everything below it.
const PSEUDO_MOUNTS: &[&str] = &[
    "/proc",
    "/sys",
    "/dev",
    "/run/user",
    "/snap",
    "/var/snap",
    "/var/lib/docker",
    "/var/lib/containers",
    "/tmp/.mount_",
    // macOS: the system volume's hidden siblings in its APFS container.
    "/System/Volumes/",
    "/private/var/vm",
];

fn under(mount: &str, prefix: &str) -> bool {
    if prefix.ends_with('/') || prefix.ends_with('_') {
        mount.starts_with(prefix)
    } else {
        mount == prefix
            || mount
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

fn is_storage(disk: &RawDisk) -> bool {
    disk.total > 0
        && !PSEUDO_FILE_SYSTEMS
            .iter()
            .any(|fs| disk.file_system.eq_ignore_ascii_case(fs))
        && !PSEUDO_MOUNTS
            .iter()
            .any(|prefix| under(&disk.mount, prefix))
}

/// What makes two mounts the same filesystem: the device where the OS
/// names one; else the label with the exact capacity and free space (an
/// APFS volume listed twice); else only the mount itself.
fn identity(disk: &RawDisk) -> String {
    if !disk.device.is_empty() {
        format!("device\0{}", disk.device)
    } else if !disk.name.is_empty() {
        format!("label\0{}\0{}\0{}", disk.name, disk.total, disk.available)
    } else {
        format!("mount\0{}", disk.mount)
    }
}

fn display_name(disk: &RawDisk) -> String {
    let name = disk.name.trim();
    if !name.is_empty() {
        return name.to_owned();
    }
    let mount = disk.mount.trim_end_matches(['/', '\\']);
    if mount.is_empty() {
        return caskade_core::trp!("disk name", "System");
    }
    // `C:` once the separator is gone.
    if mount.len() == 2 && mount.ends_with(':') {
        return if disk.removable {
            caskade_core::tr!("Removable disk")
        } else {
            caskade_core::tr!("Local disk")
        };
    }
    mount
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| disk.device.clone())
}

/// Storage volumes: pseudo filesystems dropped, each filesystem once (at
/// its shortest mount point, the others listed), fixed drives first, then
/// by mount point.
pub fn volumes(raw: &[RawDisk]) -> Vec<DiskSample> {
    let mut disks: Vec<&RawDisk> = raw.iter().filter(|disk| is_storage(disk)).collect();
    disks.sort_by(|a, b| {
        a.mount
            .len()
            .cmp(&b.mount.len())
            .then_with(|| a.mount.cmp(&b.mount))
    });
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<DiskSample> = Vec::new();
    for disk in disks {
        let key = identity(disk);
        if let Some(&at) = seen.get(&key) {
            out[at].other_mounts.push(disk.mount.clone());
            continue;
        }
        seen.insert(key, out.len());
        out.push(DiskSample {
            id: disk.mount.clone(),
            name: display_name(disk),
            mount: disk.mount.clone(),
            other_mounts: Vec::new(),
            device: disk.device.clone(),
            file_system: disk.file_system.clone(),
            removable: disk.removable,
            total: disk.total,
            available: disk.available.min(disk.total),
        });
    }
    out.sort_by(|a, b| {
        a.removable
            .cmp(&b.removable)
            .then_with(|| a.mount.cmp(&b.mount))
    });
    out
}

/// Classifies an interface by name, where the OS doesn't say (Windows,
/// macOS). `no_hardware_address`: its MAC is all zeros, as tunnels' are.
pub fn classify_interface(name: &str, no_hardware_address: bool) -> InterfaceKind {
    let lower = name.to_ascii_lowercase();
    if lower == "lo" || lower.starts_with("lo0") || lower.contains("loopback") {
        return InterfaceKind::Loopback;
    }
    const PREFIXES: &[&str] = &[
        "docker",
        "veth",
        "br-",
        "virbr",
        "vmnet",
        "vboxnet",
        "tun",
        "tap",
        "wg",
        "tailscale",
        "zt",
        "utun",
        "awdl",
        "llw",
        "bridge",
        "gif",
        "stf",
        "anpi",
        "ipsec",
        "ppp",
        "lxc",
        "lxd",
        "podman",
        "cni",
        "flannel",
        "vnet",
        "isatap",
        "teredo",
    ];
    const CONTAINS: &[&str] = &[
        "virtual",
        "vethernet",
        "vmware",
        "virtualbox",
        "hyper-v",
        "wireguard",
        "tailscale",
        "zerotier",
        "vpn",
        "tap-",
        "pseudo",
    ];
    // macOS's access point interfaces: `ap1`.
    let access_point = lower
        .strip_prefix("ap")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    if no_hardware_address
        || access_point
        || PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
        || CONTAINS.iter().any(|part| lower.contains(part))
    {
        InterfaceKind::Virtual
    } else {
        InterfaceKind::Physical
    }
}

#[derive(Debug, Clone, Copy)]
struct Counters {
    at: Duration,
    received: u64,
    transmitted: u64,
}

/// Turns readings into samples; remembers the last counters for rates.
#[derive(Debug, Default)]
pub struct Aggregator {
    counters: HashMap<String, Counters>,
}

fn rate(before: u64, after: u64, seconds: f64) -> Option<f64> {
    // A counter that went back was reset (the interface restarted).
    (after >= before && seconds > 0.0).then(|| (after - before) as f64 / seconds)
}

fn percent(value: Option<f32>) -> Option<f32> {
    value
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 100.0))
}

impl Aggregator {
    /// The sample for `readings` taken at `at` (Unix ms) and `now`
    /// (monotonic, for rates). Its `seq` is the caller's to set.
    pub fn sample(&mut self, at: i64, now: Duration, readings: Readings) -> SystemSample {
        let memory = readings
            .memory
            .filter(|(total, available)| *total > 0 && available <= total)
            .map(|(total, available)| MemorySample {
                total,
                used: total - available,
                available,
            });
        let gpus = GpuSamples {
            devices: readings
                .gpus
                .devices
                .into_iter()
                .map(|mut gpu| {
                    gpu.usage = percent(gpu.usage);
                    gpu
                })
                .collect(),
            unavailable: readings.gpus.unavailable,
        };
        SystemSample {
            seq: 0,
            at,
            error: None,
            cpu: CpuSample {
                usage: percent(readings.cpu),
                cores: readings.cores,
            },
            memory,
            gpus,
            disks: volumes(&readings.disks),
            network: self.network(now, readings.interfaces),
        }
    }

    /// A sample where nothing could be read.
    pub fn failed(at: i64, error: &str) -> SystemSample {
        SystemSample {
            seq: 0,
            at,
            error: Some(error.into()),
            cpu: CpuSample {
                usage: None,
                cores: 0,
            },
            memory: None,
            gpus: GpuSamples {
                devices: Vec::new(),
                unavailable: None,
            },
            disks: Vec::new(),
            network: NetworkSample {
                interfaces: Vec::new(),
                received: None,
                transmitted: None,
            },
        }
    }

    fn network(&mut self, now: Duration, raw: Vec<RawInterface>) -> NetworkSample {
        let mut next = HashMap::with_capacity(raw.len());
        let mut interfaces: Vec<InterfaceSample> = raw
            .into_iter()
            .map(|interface| {
                let before = self.counters.get(&interface.name);
                let seconds =
                    before.map_or(0.0, |before| now.saturating_sub(before.at).as_secs_f64());
                let received =
                    before.and_then(|before| rate(before.received, interface.received, seconds));
                let transmitted = before
                    .and_then(|before| rate(before.transmitted, interface.transmitted, seconds));
                next.insert(
                    interface.name.clone(),
                    Counters {
                        at: now,
                        received: interface.received,
                        transmitted: interface.transmitted,
                    },
                );
                InterfaceSample {
                    name: interface.name,
                    kind: interface.kind,
                    received,
                    transmitted,
                }
            })
            .collect();
        // Interfaces that went away are forgotten.
        self.counters = next;
        let order = |kind: InterfaceKind| match kind {
            InterfaceKind::Physical => 0,
            InterfaceKind::Virtual => 1,
            InterfaceKind::Loopback => 2,
        };
        interfaces.sort_by(|a, b| {
            order(a.kind)
                .cmp(&order(b.kind))
                .then_with(|| a.name.cmp(&b.name))
        });
        let total = |pick: fn(&InterfaceSample) -> Option<f64>| {
            interfaces
                .iter()
                .filter(|interface| interface.kind == InterfaceKind::Physical)
                .filter_map(pick)
                .fold(None, |sum: Option<f64>, value| {
                    Some(sum.unwrap_or(0.0) + value)
                })
        };
        let received = total(|interface| interface.received);
        let transmitted = total(|interface| interface.transmitted);
        NetworkSample {
            interfaces,
            received,
            transmitted,
        }
    }
}
