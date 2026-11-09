//! Live system statistics for Settings → Statistics (#156): CPU, GPU,
//! memory, storage capacity and network rates of the machine Caskade runs
//! on.
//!
//! [`SystemStats`] samples only while someone watches: the dashboard takes a
//! lease with [`SystemStats::watch`] and renews it, and sampling stops once
//! the last lease is given back or runs out. Each sample is read off Tokio's
//! worker threads ([`INTERVAL`] apart), kept in a rolling in-memory history
//! of [`HISTORY`] samples and broadcast to [`SystemStats::subscribe`]rs.
//!
//! A metric that couldn't be read is `None` (or carries a reason), never a
//! zero: the dashboard shows a gap or an unavailable state for it.

mod aggregate;
mod gpu;
mod probe;
#[cfg(test)]
mod tests;

pub use aggregate::{Aggregator, RawDisk, RawInterface, classify_interface};
pub use probe::{NativeProbe, Probe, Readings};

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::time::MissedTickBehavior;

/// Time between samples.
pub const INTERVAL: Duration = Duration::from_secs(1);
/// Samples kept: a minute at [`INTERVAL`].
pub const HISTORY: usize = 60;
/// How long a watch lasts without being renewed. It covers a webview that
/// reloads or crashes without giving its watch back.
pub const LEASE: Duration = Duration::from_secs(30);

/// The history so far and how it is sampled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatsSnapshot {
    /// Milliseconds between samples.
    pub interval_ms: u32,
    /// Samples the history keeps.
    pub history: u32,
    /// Oldest first; empty when the watch didn't ask for them.
    pub samples: Vec<SystemSample>,
}

/// One reading of the whole machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SystemSample {
    /// Increases by one per sample, across watches.
    pub seq: u64,
    /// When it was taken, Unix milliseconds.
    pub at: i64,
    /// Why nothing could be read this time; every metric is then empty.
    pub error: Option<String>,
    pub cpu: CpuSample,
    /// `None` when memory couldn't be read.
    pub memory: Option<MemorySample>,
    pub gpus: GpuSamples,
    /// Mounted volumes, each filesystem once, pseudo filesystems left out.
    pub disks: Vec<DiskSample>,
    pub network: NetworkSample,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CpuSample {
    /// Overall utilization, 0–100, across every core. `None` for the first
    /// sample of a watch (usage is measured between two readings) and when
    /// it couldn't be read.
    pub usage: Option<f32>,
    /// Logical cores; 0 when unknown.
    pub cores: u32,
}

/// RAM in bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MemorySample {
    pub total: u64,
    pub used: u64,
    /// What programs can still get without swapping, page cache included.
    pub available: u64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GpuSamples {
    pub devices: Vec<GpuSample>,
    /// Why no GPU is listed: none found, or this OS has no source Caskade
    /// reads (macOS).
    pub unavailable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GpuSample {
    /// Stable while the GPU stays: the DRM card on Linux (`card1`), the
    /// driver's index elsewhere (`nvidia0`).
    pub id: String,
    pub name: String,
    pub vendor: GpuVendor,
    /// Utilization, 0–100; `None` with `unavailable` saying why.
    pub usage: Option<f32>,
    /// Video memory in bytes, where the driver reports it.
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    /// Why `usage` is missing.
    pub unavailable: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GpuVendor {
    Amd,
    Intel,
    Nvidia,
    Other,
}

/// A volume's capacity in bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiskSample {
    /// Its mount point; stable while it stays mounted.
    pub id: String,
    /// The volume's label, or a name made from its mount point.
    pub name: String,
    /// Where it is mounted: a path, or a drive (`C:\`) on Windows.
    pub mount: String,
    /// Other mount points of the same filesystem (bind mounts, subvolumes).
    pub other_mounts: Vec<String>,
    /// The block device on Linux (`/dev/nvme0n1p2`); empty elsewhere.
    pub device: String,
    pub file_system: String,
    pub removable: bool,
    pub total: u64,
    pub available: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSample {
    /// Physical first, then virtual, then loopback.
    pub interfaces: Vec<InterfaceSample>,
    /// Bytes per second over the physical interfaces only, so traffic
    /// through a VPN, bridge or container isn't counted twice. `None`
    /// until there is a rate to add.
    pub received: Option<f64>,
    pub transmitted: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceSample {
    pub name: String,
    pub kind: InterfaceKind,
    /// Bytes per second since the last sample; `None` on the interface's
    /// first sample and after its counters reset.
    pub received: Option<f64>,
    pub transmitted: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum InterfaceKind {
    /// A network card: counted in the totals.
    Physical,
    /// A VPN, bridge, container or VM interface: its traffic also crosses
    /// a physical one, so it isn't counted.
    Virtual,
    Loopback,
}

/// How often to sample, how much to keep and how long a watch lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SamplerConfig {
    pub interval: Duration,
    pub history: usize,
    pub lease: Duration,
}

impl Default for SamplerConfig {
    fn default() -> Self {
        Self {
            interval: INTERVAL,
            history: HISTORY,
            lease: LEASE,
        }
    }
}

type ProbeFactory = Arc<dyn Fn() -> Box<dyn Probe> + Send + Sync>;

/// The sampler. Cheap to clone; clones share the history and the watches.
#[derive(Clone)]
pub struct SystemStats {
    shared: Arc<Shared>,
}

struct Shared {
    config: SamplerConfig,
    factory: ProbeFactory,
    state: Mutex<State>,
    sender: broadcast::Sender<SystemSample>,
}

#[derive(Default)]
struct State {
    /// Watcher → when its lease runs out.
    watchers: HashMap<String, Instant>,
    history: VecDeque<SystemSample>,
    sampling: bool,
    seq: u64,
}

impl Default for SystemStats {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemStats {
    /// Samples this machine through [`NativeProbe`].
    pub fn new() -> Self {
        Self::with_probe(SamplerConfig::default(), || {
            Box::new(NativeProbe::new()) as Box<dyn Probe>
        })
    }

    /// Samples through probes from `factory`: one per run of the sampler,
    /// dropped when sampling stops, and a new one after a probe panics.
    pub fn with_probe(
        config: SamplerConfig,
        factory: impl Fn() -> Box<dyn Probe> + Send + Sync + 'static,
    ) -> Self {
        let (sender, _) = broadcast::channel(16);
        Self {
            shared: Arc::new(Shared {
                config,
                factory: Arc::new(factory),
                state: Mutex::new(State::default()),
                sender,
            }),
        }
    }

    /// Every sample from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<SystemSample> {
        self.shared.sender.subscribe()
    }

    /// Starts or renews `watcher`'s lease, starting the sampler if it isn't
    /// running, and answers the history (when `history`). Call it from
    /// inside a Tokio runtime.
    pub fn watch(&self, watcher: &str, history: bool) -> SystemStatsSnapshot {
        let config = self.shared.config;
        let mut state = self.shared.lock();
        state
            .watchers
            .insert(watcher.to_owned(), Instant::now() + config.lease);
        // A history from an earlier watch is kept while it is still in the
        // window; the time between shows as a gap.
        let window = config.interval.saturating_mul(config.history as u32);
        let oldest = caskade_core::now_ms().saturating_sub(window.as_millis() as i64);
        state.history.retain(|sample| sample.at >= oldest);
        let snapshot = SystemStatsSnapshot {
            interval_ms: config.interval.as_millis().min(u32::MAX as u128) as u32,
            history: config.history.min(u32::MAX as usize) as u32,
            samples: if history {
                state.history.iter().cloned().collect()
            } else {
                Vec::new()
            },
        };
        if !state.sampling {
            state.sampling = true;
            drop(state);
            tokio::spawn(run(self.shared.clone()));
        }
        snapshot
    }

    /// Gives `watcher`'s lease back; sampling stops before the next sample
    /// when it was the last.
    pub fn unwatch(&self, watcher: &str) {
        self.shared.lock().watchers.remove(watcher);
    }

    pub fn is_sampling(&self) -> bool {
        self.shared.lock().sampling
    }

    /// The history, oldest first.
    pub fn history(&self) -> Vec<SystemSample> {
        self.shared.lock().history.iter().cloned().collect()
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        caskade_core::lock(&self.state)
    }

    /// Drops the watches whose lease ran out; with none left, stops.
    fn keep_sampling(&self) -> bool {
        let mut state = self.lock();
        let now = Instant::now();
        state.watchers.retain(|_, until| *until > now);
        if state.watchers.is_empty() {
            state.sampling = false;
        }
        state.sampling
    }

    fn push(&self, mut sample: SystemSample) {
        let mut state = self.lock();
        state.seq += 1;
        sample.seq = state.seq;
        state.history.push_back(sample.clone());
        while state.history.len() > self.config.history {
            state.history.pop_front();
        }
        drop(state);
        // No receivers is fine: the dashboard may have just closed.
        let _ = self.sender.send(sample);
    }
}

/// The sampler, from the first watch until the last one ends.
async fn run(shared: Arc<Shared>) {
    let started = Instant::now();
    let mut aggregator = Aggregator::default();
    let mut probe: Option<Box<dyn Probe>> = None;
    let mut ticker = tokio::time::interval(shared.config.interval);
    // A slow read pushes the next sample back rather than bunching them.
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if !shared.keep_sampling() {
            break;
        }
        let factory = shared.factory.clone();
        let taken = probe.take();
        // sysinfo and the GPU files block; a slow disk mustn't hold a
        // worker. Not on the shared job queue: like process monitoring,
        // the dashboard mustn't wait behind a library scan.
        let read = tokio::task::spawn_blocking(move || {
            let mut probe = taken.unwrap_or_else(|| factory());
            let readings = probe.read();
            (probe, readings)
        })
        .await;
        let at = caskade_core::now_ms();
        let sample = match read {
            Ok((kept, readings)) => {
                probe = Some(kept);
                aggregator.sample(at, started.elapsed(), readings)
            }
            Err(error) => {
                tracing::warn!(%error, "system statistics: a sample failed");
                Aggregator::failed(at, &caskade_core::tr!("This sample couldn't be read."))
            }
        };
        shared.push(sample);
    }
    // Dropping the probe stops its helpers (nvidia-smi), which may block.
    if let Some(probe) = probe {
        tokio::task::spawn_blocking(move || drop(probe));
    }
}
