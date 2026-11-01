//! Native readings: sysinfo for CPU, memory, volumes and interfaces, and
//! [`gpu`](super::gpu) for GPUs.

use super::gpu::GpuReader;
use super::{GpuSamples, InterfaceKind, RawDisk, RawInterface, classify_interface};
use sysinfo::{DiskRefreshKind, Disks, Networks, System};

/// Everything one sample reads, before aggregation.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Readings {
    /// Overall CPU utilization in percent; `None` when there is no
    /// measurement yet.
    pub cpu: Option<f32>,
    pub cores: u32,
    /// `(total, available)` bytes; `None` when it couldn't be read.
    pub memory: Option<(u64, u64)>,
    pub gpus: GpuSamples,
    pub disks: Vec<RawDisk>,
    pub interfaces: Vec<RawInterface>,
}

/// A source of readings. Reads may block; the sampler calls them on a
/// blocking thread.
pub trait Probe: Send {
    fn read(&mut self) -> Readings;
}

/// Volumes change rarely and statting each costs more than the rest, so
/// the list is re-read every this many samples.
const DISKS_EVERY: u32 = 5;

/// This machine, through caches reused between reads.
pub struct NativeProbe {
    system: System,
    disks: Disks,
    networks: Networks,
    gpus: GpuReader,
    /// CPU usage is measured between two refreshes: the first gives none.
    cpu_primed: bool,
    reads: u32,
    /// Device, as sysinfo names it → filesystem label.
    #[cfg(target_os = "linux")]
    labels: std::collections::HashMap<String, String>,
}

impl Default for NativeProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeProbe {
    pub fn new() -> Self {
        Self {
            system: System::new(),
            disks: Disks::new(),
            networks: Networks::new(),
            gpus: GpuReader::new(),
            cpu_primed: false,
            reads: 0,
            #[cfg(target_os = "linux")]
            labels: Default::default(),
        }
    }

    fn disks(&mut self) -> Vec<RawDisk> {
        if self.reads.is_multiple_of(DISKS_EVERY) {
            // Removes volumes that went away and adds new ones.
            self.disks
                .refresh_specifics(true, DiskRefreshKind::nothing().with_storage());
            #[cfg(target_os = "linux")]
            {
                self.labels = linux::labels(self.disks.list().iter().map(|disk| disk.name()));
            }
        }
        self.disks
            .list()
            .iter()
            .map(|disk| {
                let name = disk.name().to_string_lossy().into_owned();
                let mount = disk.mount_point().to_string_lossy().into_owned();
                let file_system = disk.file_system().to_string_lossy().into_owned();
                // Linux names the device; Windows and macOS the label.
                #[cfg(target_os = "linux")]
                let (name, device) = (self.labels.get(&name).cloned().unwrap_or_default(), name);
                #[cfg(not(target_os = "linux"))]
                let device = String::new();
                RawDisk {
                    name,
                    device,
                    mount,
                    file_system,
                    removable: disk.is_removable(),
                    total: disk.total_space(),
                    available: disk.available_space(),
                }
            })
            .collect()
    }

    fn interfaces(&mut self) -> Vec<RawInterface> {
        self.networks.refresh(true);
        self.networks
            .list()
            .iter()
            .map(|(name, data)| RawInterface {
                kind: interface_kind(name, data.mac_address().is_unspecified()),
                name: name.clone(),
                received: data.total_received(),
                transmitted: data.total_transmitted(),
            })
            .collect()
    }
}

impl Probe for NativeProbe {
    fn read(&mut self) -> Readings {
        self.system.refresh_cpu_usage();
        let cpu = if self.cpu_primed {
            Some(self.system.global_cpu_usage())
        } else {
            self.cpu_primed = true;
            None
        };
        self.system.refresh_memory();
        let total = self.system.total_memory();
        let memory = (total > 0).then(|| (total, self.system.available_memory()));
        let readings = Readings {
            cpu,
            cores: self.system.cpus().len() as u32,
            memory,
            gpus: self.gpus.read(),
            disks: self.disks(),
            interfaces: self.interfaces(),
        };
        self.reads = self.reads.wrapping_add(1);
        readings
    }
}

/// Linux says which interfaces are hardware: those with a `device` link.
#[cfg(target_os = "linux")]
fn interface_kind(name: &str, no_hardware_address: bool) -> InterfaceKind {
    let class = std::path::Path::new("/sys/class/net").join(name);
    if name == "lo" {
        InterfaceKind::Loopback
    } else if class.join("device").exists() {
        InterfaceKind::Physical
    } else if class.exists() {
        InterfaceKind::Virtual
    } else {
        classify_interface(name, no_hardware_address)
    }
}

#[cfg(not(target_os = "linux"))]
fn interface_kind(name: &str, no_hardware_address: bool) -> InterfaceKind {
    classify_interface(name, no_hardware_address)
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::ffi::OsStr;

    /// Each device's filesystem label, from `/dev/disk/by-label`. Both
    /// sides are resolved: `/dev/mapper` names are links too.
    pub fn labels<'a>(devices: impl Iterator<Item = &'a OsStr>) -> HashMap<String, String> {
        let Ok(entries) = std::fs::read_dir("/dev/disk/by-label") else {
            return HashMap::new();
        };
        let by_device: HashMap<_, _> = entries
            .flatten()
            .filter_map(|entry| {
                let device = std::fs::canonicalize(entry.path()).ok()?;
                Some((device, unescape(&entry.file_name().to_string_lossy())))
            })
            .collect();
        devices
            .filter(|device| device.to_string_lossy().starts_with("/dev/"))
            .filter_map(|device| {
                let label = by_device.get(&std::fs::canonicalize(device).ok()?)?;
                Some((device.to_string_lossy().into_owned(), label.clone()))
            })
            .collect()
    }

    /// udev writes a label's spaces and slashes as `\x20`, `\x2f`.
    pub fn unescape(name: &str) -> String {
        let bytes = name.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] == b'\\'
                && bytes.get(at + 1) == Some(&b'x')
                && let Some(byte) = bytes
                    .get(at + 2..at + 4)
                    .and_then(|hex| std::str::from_utf8(hex).ok())
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                out.push(byte);
                at += 4;
            } else {
                out.push(bytes[at]);
                at += 1;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}

#[cfg(all(test, target_os = "linux"))]
pub(super) use linux::unescape as unescape_udev;
