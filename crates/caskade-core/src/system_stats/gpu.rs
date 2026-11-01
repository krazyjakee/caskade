//! GPU utilization. Linux: the DRM cards in `/sys/class/drm`, whose amdgpu
//! (and some Intel Xe) drivers report `gpu_busy_percent`. NVIDIA, on Linux
//! and Windows: `nvidia-smi`, the driver's own tool, run once per watch
//! and kept reporting every second (NVML through a library would need
//! `unsafe` FFI, which the workspace forbids). macOS and other GPUs on
//! Windows have no source Caskade reads; they say so instead of showing 0.

// The sysfs reader is Linux's; tests run it on every host.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use super::{GpuSample, GpuSamples, GpuVendor};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A DRM card as sysfs describes it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DrmCard {
    /// `card0`.
    pub id: String,
    /// Its PCI address without the domain (`01:00.0`).
    pub slot: Option<String>,
    pub vendor: GpuVendor,
    pub name: Option<String>,
    pub usage: Option<f32>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
}

/// One GPU's line from `nvidia-smi`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SmiRow {
    pub index: u32,
    /// Its PCI address without the domain.
    pub slot: String,
    pub name: String,
    pub usage: Option<f32>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
}

/// `0000:01:00.0` and nvidia-smi's `00000000:01:00.0` both → `01:00.0`.
pub(crate) fn pci_slot(address: &str) -> String {
    let address = address.trim().to_ascii_lowercase();
    match address.split_once(':') {
        Some((_, rest)) if rest.contains(':') => rest.to_owned(),
        _ => address,
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn vendor(id: &str) -> GpuVendor {
    match id.to_ascii_lowercase().as_str() {
        "0x1002" => GpuVendor::Amd,
        "0x8086" => GpuVendor::Intel,
        "0x10de" => GpuVendor::Nvidia,
        _ => GpuVendor::Other,
    }
}

/// The GPUs under `drm` (`/sys/class/drm`): `card<N>` entries with a PCI
/// vendor, once per PCI device. Connectors (`card0-DP-1`) and render nodes
/// are skipped.
pub(crate) fn drm_cards(drm: &Path) -> Vec<DrmCard> {
    let Ok(entries) = std::fs::read_dir(drm) else {
        return Vec::new();
    };
    let mut cards: Vec<DrmCard> = entries
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            let number = id.strip_prefix("card")?;
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let device = entry.path().join("device");
            let vendor = vendor(&read_trimmed(&device.join("vendor"))?);
            let slot = read_trimmed(&device.join("uevent")).and_then(|uevent| {
                uevent
                    .lines()
                    .find_map(|line| line.strip_prefix("PCI_SLOT_NAME="))
                    .map(pci_slot)
            });
            let read_number = |file: &str| {
                read_trimmed(&device.join(file)).and_then(|text| text.parse::<u64>().ok())
            };
            Some(DrmCard {
                usage: read_number("gpu_busy_percent").map(|value| value as f32),
                memory_used: read_number("mem_info_vram_used"),
                memory_total: read_number("mem_info_vram_total").filter(|total| *total > 0),
                name: read_trimmed(&device.join("product_name")),
                slot,
                vendor,
                id,
            })
        })
        .collect();
    cards.sort_by_key(|card| {
        card.id
            .trim_start_matches("card")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });
    let mut slots = Vec::new();
    cards.retain(|card| match &card.slot {
        Some(slot) if slots.contains(slot) => false,
        Some(slot) => {
            slots.push(slot.clone());
            true
        }
        None => true,
    });
    cards
}

fn vendor_name(vendor: GpuVendor) -> String {
    match vendor {
        GpuVendor::Amd => crate::tr!("AMD GPU"),
        GpuVendor::Intel => crate::tr!("Intel GPU"),
        GpuVendor::Nvidia => crate::tr!("NVIDIA GPU"),
        GpuVendor::Other => crate::tr!("GPU"),
    }
}

/// Parses one line of `index, pci.bus_id, name, utilization.gpu,
/// memory.used, memory.total` (CSV, no header, no units; MiB). `None` for
/// anything else, such as nvidia-smi's error messages.
pub(crate) fn parse_smi_line(line: &str) -> Option<SmiRow> {
    let fields: Vec<&str> = line.split(',').map(str::trim).collect();
    if fields.len() < 6 {
        return None;
    }
    let index = fields[0].parse().ok()?;
    let end = fields.len();
    // `[N/A]`, `[Not Supported]`: not a number.
    let value = |text: &str| text.parse::<f64>().ok().filter(|value| value.is_finite());
    let mib = |text: &str| value(text).map(|mib| (mib.max(0.0) * 1024.0 * 1024.0) as u64);
    Some(SmiRow {
        index,
        slot: pci_slot(fields[1]),
        name: fields[2..end - 3].join(", "),
        usage: value(fields[end - 3]).map(|value| value as f32),
        memory_used: mib(fields[end - 2]),
        memory_total: mib(fields[end - 1]).filter(|total| *total > 0),
    })
}

/// What nvidia-smi says about NVIDIA GPUs this sample.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SmiView {
    /// Rows from the last few seconds.
    pub rows: Vec<SmiRow>,
    /// Why there are none: not installed, failed, stopped answering.
    pub problem: Option<String>,
}

/// The cards with nvidia-smi's rows joined in by PCI address, then any
/// rows no card matched (all of them on Windows).
pub(crate) fn merge(cards: Vec<DrmCard>, smi: &SmiView) -> Vec<GpuSample> {
    let mut used = vec![false; smi.rows.len()];
    let lone_nvidia = cards
        .iter()
        .filter(|card| card.vendor == GpuVendor::Nvidia)
        .count()
        == 1
        && smi.rows.len() == 1;
    let mut gpus: Vec<GpuSample> = cards
        .into_iter()
        .map(|card| {
            let row = if card.vendor == GpuVendor::Nvidia {
                smi.rows
                    .iter()
                    .position(|row| lone_nvidia || card.slot.as_deref() == Some(row.slot.as_str()))
            } else {
                None
            };
            if let Some(at) = row {
                used[at] = true;
                return from_row(card.id, &smi.rows[at]);
            }
            let unavailable = card.usage.is_none().then(|| match card.vendor {
                GpuVendor::Nvidia => smi
                    .problem
                    .clone()
                    .unwrap_or_else(|| crate::tr!("nvidia-smi didn't report this GPU.")),
                GpuVendor::Intel => crate::tr!("Intel's driver doesn't report GPU usage to apps."),
                _ => crate::tr!("This GPU's driver doesn't report its usage."),
            });
            GpuSample {
                name: card.name.unwrap_or_else(|| vendor_name(card.vendor)),
                id: card.id,
                vendor: card.vendor,
                usage: card.usage,
                memory_used: card.memory_used,
                memory_total: card.memory_total,
                unavailable,
            }
        })
        .collect();
    for (row, used) in smi.rows.iter().zip(used) {
        if !used {
            gpus.push(from_row(format!("nvidia{}", row.index), row));
        }
    }
    gpus
}

fn from_row(id: String, row: &SmiRow) -> GpuSample {
    GpuSample {
        id,
        name: if row.name.is_empty() {
            vendor_name(GpuVendor::Nvidia)
        } else {
            row.name.clone()
        },
        vendor: GpuVendor::Nvidia,
        usage: row.usage,
        memory_used: row.memory_used,
        memory_total: row.memory_total,
        unavailable: row
            .usage
            .is_none()
            .then(|| crate::tr!("The NVIDIA driver doesn't report this GPU's usage.")),
    }
}

/// Why no GPU is listed.
fn none_found() -> String {
    if cfg!(target_os = "macos") {
        crate::tr!("macOS doesn't report GPU usage to apps.")
    } else if cfg!(windows) {
        crate::tr!("GPU usage on Windows comes from NVIDIA's nvidia-smi; no NVIDIA GPU answered.")
    } else if cfg!(target_os = "linux") {
        crate::tr!("No GPU found.")
    } else {
        crate::tr!("GPU usage isn't available on this system.")
    }
}

/// A row and when it arrived.
type Rows = HashMap<u32, (Instant, SmiRow)>;

#[derive(Default)]
struct SmiState {
    rows: Rows,
    /// The last line that wasn't a row.
    message: Option<String>,
    ended: bool,
}

/// A running `nvidia-smi -lms 1000`, read by a thread of its own. Dropping
/// it stops the process.
struct NvidiaSmi {
    child: Child,
    state: Arc<Mutex<SmiState>>,
}

/// Rows older than this are a gap, not a reading.
const SMI_FRESH: Duration = Duration::from_secs(3);
/// After nvidia-smi fails or exits, wait this long before starting it again.
const SMI_RETRY: Duration = Duration::from_secs(30);

impl NvidiaSmi {
    fn start() -> std::io::Result<Self> {
        let mut command = Command::new("nvidia-smi");
        command
            .args([
                "--query-gpu=index,pci.bus_id,name,utilization.gpu,memory.used,memory.total",
                "--format=csv,noheader,nounits",
                "-lms",
                "1000",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console flashes up.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn()?;
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::other("nvidia-smi has no output"));
        };
        let state = Arc::new(Mutex::new(SmiState::default()));
        let shared = state.clone();
        let reader = std::thread::Builder::new()
            .name("nvidia-smi".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    let mut state = crate::lock(&shared);
                    match parse_smi_line(&line) {
                        Some(row) => {
                            state.rows.insert(row.index, (Instant::now(), row));
                        }
                        None if !line.trim().is_empty() => {
                            state.message = Some(line.trim().chars().take(200).collect());
                        }
                        None => {}
                    }
                }
                crate::lock(&shared).ended = true;
            });
        if let Err(error) = reader {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(Self { child, state })
    }
}

impl Drop for NvidiaSmi {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reads the GPUs each sample, keeping nvidia-smi running between them.
pub(crate) struct GpuReader {
    smi: Option<NvidiaSmi>,
    /// nvidia-smi isn't installed: don't try again this watch.
    smi_missing: bool,
    smi_retry_at: Option<Instant>,
    smi_problem: Option<String>,
}

impl GpuReader {
    pub fn new() -> Self {
        Self {
            smi: None,
            smi_missing: false,
            smi_retry_at: None,
            smi_problem: None,
        }
    }

    pub fn read(&mut self) -> GpuSamples {
        #[cfg(target_os = "linux")]
        let cards = drm_cards(Path::new("/sys/class/drm"));
        #[cfg(not(target_os = "linux"))]
        let cards: Vec<DrmCard> = Vec::new();
        let wants_smi = cfg!(windows)
            || (cfg!(target_os = "linux")
                && cards.iter().any(|card| card.vendor == GpuVendor::Nvidia));
        let smi = if wants_smi {
            self.smi()
        } else {
            SmiView::default()
        };
        let devices = merge(cards, &smi);
        // Windows without nvidia-smi most likely has no NVIDIA GPU at all.
        let unavailable = devices.is_empty().then(|| match smi.problem {
            Some(problem) if cfg!(windows) && !self.smi_missing => problem,
            _ => none_found(),
        });
        GpuSamples {
            devices,
            unavailable,
        }
    }

    fn smi(&mut self) -> SmiView {
        if self.smi_missing {
            return SmiView {
                rows: Vec::new(),
                problem: Some(crate::tr!(
                    "Install NVIDIA's nvidia-smi to see this GPU's usage."
                )),
            };
        }
        if self.smi.is_none() && self.smi_retry_at.is_none_or(|at| Instant::now() >= at) {
            match NvidiaSmi::start() {
                Ok(smi) => {
                    self.smi = Some(smi);
                    self.smi_retry_at = None;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.smi_missing = true;
                    return self.smi();
                }
                Err(error) => {
                    tracing::debug!(%error, "nvidia-smi didn't start");
                    self.smi_problem = Some(crate::tr!(
                        "nvidia-smi didn't start: {error}",
                        error = error
                    ));
                    self.smi_retry_at = Some(Instant::now() + SMI_RETRY);
                }
            }
        }
        let Some(smi) = &self.smi else {
            return SmiView {
                rows: Vec::new(),
                problem: self.smi_problem.clone(),
            };
        };
        let (rows, message, ended) = {
            let state = crate::lock(&smi.state);
            let now = Instant::now();
            let mut rows: Vec<SmiRow> = state
                .rows
                .values()
                .filter(|(at, _)| now.duration_since(*at) <= SMI_FRESH)
                .map(|(_, row)| row.clone())
                .collect();
            rows.sort_by_key(|row| row.index);
            (rows, state.message.clone(), state.ended)
        };
        if ended {
            self.smi = None;
            self.smi_retry_at = Some(Instant::now() + SMI_RETRY);
            self.smi_problem = Some(message.unwrap_or_else(|| crate::tr!("nvidia-smi stopped.")));
            return SmiView {
                rows,
                problem: self.smi_problem.clone(),
            };
        }
        let problem = rows.is_empty().then(|| {
            message.unwrap_or_else(|| crate::tr!("Waiting for nvidia-smi's first reading."))
        });
        SmiView { rows, problem }
    }
}
