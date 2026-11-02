//! Library sync in the background (#230): paired devices stay in step on
//! their own, with no buttons to press.
//!
//! The service captures this device's changes every couple of seconds and,
//! when there are any, exchanges with every paired device at once. It also
//! exchanges when a paired device comes into reach on this network, when one
//! connects, and otherwise polls quietly: a minute after a sync, backing off
//! from 30 seconds to 5 minutes while a device can't be reached. An
//! unreachable device is a normal state, never a toast: the status says so
//! per device (`DeviceSync`), and the UI shows it.
//!
//! Everything runs on background tasks, and every write is one short
//! transaction per change, so sync never blocks the UI or a game launch.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{Notify, mpsc};
use tokio::time::Instant;

use super::clock::now_ms;
use super::engine::SyncEngine;
use super::exchange::{self, Options};
use crate::Result;
use crate::db::LibraryStore;
use crate::devices::{ConnectionPurpose, DeviceSession, DeviceSync, Devices};
use crate::settings::SettingsService;

/// How often this device's changes are captured.
const CAPTURE: Duration = Duration::from_secs(2);
/// How long after a sync a device is polled again.
const POLL: Duration = Duration::from_secs(60);
/// The first wait after a failed attempt; it doubles up to [`MAX_BACKOFF`].
const BACKOFF: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(5 * 60);
/// How often the log is compacted.
const COMPACT: Duration = Duration::from_secs(10 * 60);

/// Library sync with every paired device. Cheap to clone.
#[derive(Clone)]
pub struct SyncService {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    devices: Devices,
    settings: OnceLock<SettingsService>,
    /// The engine for this device's current identity.
    engine: RwLock<Option<Arc<SyncEngine>>>,
    /// Devices an exchange is running with.
    busy: Mutex<HashSet<String>>,
    /// Something wants an exchange now: devices to sync.
    due: mpsc::UnboundedSender<Option<String>>,
    due_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Option<String>>>,
    done: Notify,
}

impl SyncService {
    /// Sync for `store`, over `devices`' endpoint. Takes the `sync` sessions
    /// paired devices open.
    pub fn new(store: LibraryStore, devices: Devices) -> Self {
        let (due, due_rx) = mpsc::unbounded_channel();
        let service = Self {
            inner: Arc::new(Inner {
                store,
                devices: devices.clone(),
                settings: OnceLock::new(),
                engine: RwLock::default(),
                busy: Mutex::default(),
                due,
                due_rx: tokio::sync::Mutex::new(due_rx),
                done: Notify::new(),
            }),
        };
        devices.set_sync_handler(Arc::new(Inbound(Arc::downgrade(&service.inner))));
        service
    }

    /// Reads the category choices (`core.sync.categories`) from `settings`.
    pub fn set_settings(&self, settings: SettingsService) {
        let _ = self.inner.settings.set(settings);
    }

    /// The engine, once this device's identity is ready.
    pub fn engine(&self) -> Option<Arc<SyncEngine>> {
        self.inner.engine()
    }

    /// Syncs with one device now.
    pub fn sync_now(&self, device_id: &str) {
        let _ = self.inner.due.send(Some(device_id.to_owned()));
    }

    /// Runs sync until the runtime stops.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let inner = self.inner.clone();
        tokio::spawn(async move { inner.run().await })
    }
}

impl Inner {
    fn engine(&self) -> Option<Arc<SyncEngine>> {
        let device = self.devices.listening_id()?;
        let current = self
            .engine
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(engine) = current.filter(|engine| engine.device() == device) {
            return Some(engine);
        }
        // A new identity is a new device: a new engine with its id.
        let engine = Arc::new(SyncEngine::new(self.store.clone(), device));
        if let Some(settings) = self.settings.get() {
            engine.set_settings(settings.clone());
        }
        *self.engine.write().unwrap_or_else(|e| e.into_inner()) = Some(engine.clone());
        Some(engine)
    }

    async fn run(self: Arc<Self>) {
        let mut due_rx = self.due_rx.lock().await;
        let mut reachable = self.devices.reachable();
        let mut settings = self.settings.get().map(SettingsService::subscribe);
        let mut capture = tokio::time::interval(CAPTURE);
        let mut compact = tokio::time::interval(COMPACT);
        let mut next: HashMap<String, Instant> = HashMap::new();
        let mut backoff: HashMap<String, Duration> = HashMap::new();
        let (results, mut finished) = mpsc::unbounded_channel::<(String, bool)>();
        loop {
            let wake = next.values().min().copied();
            let mut now_due: Vec<String> = Vec::new();
            tokio::select! {
                _ = capture.tick() => {
                    let Some(engine) = self.engine() else { continue };
                    match engine.capture().await {
                        // This device changed something: tell everyone.
                        Ok(count) if count > 0 => now_due.extend(self.paired().await),
                        Ok(_) => {}
                        Err(error) => tracing::warn!(%error, "sync capture failed"),
                    }
                    for device in self.paired().await {
                        next.entry(device).or_insert_with(Instant::now);
                    }
                }
                _ = compact.tick() => {
                    if let Some(engine) = self.engine()
                        && let Err(error) = engine.compact().await
                    {
                        tracing::warn!(%error, "sync log not compacted");
                    }
                }
                device = due_rx.recv() => match device {
                    Some(Some(device)) => now_due.push(device),
                    Some(None) => now_due.extend(self.paired().await),
                    None => break,
                },
                device = reachable.recv() => {
                    if let Ok(device) = device {
                        now_due.push(device);
                    }
                }
                changed = async {
                    match settings.as_mut() {
                        Some(changes) => changes.recv().await.ok(),
                        None => std::future::pending().await,
                    }
                } => {
                    if changed.is_some_and(|changed| changed.namespace == "core.sync")
                        && let Some(engine) = self.engine()
                    {
                        self.categories_changed(&engine).await;
                        now_due.extend(self.paired().await);
                    }
                }
                Some((device, ok)) = finished.recv() => {
                    let wait = if ok {
                        backoff.remove(&device);
                        POLL
                    } else {
                        let wait = backoff.get(&device).map_or(BACKOFF, |wait| (*wait * 2).min(MAX_BACKOFF));
                        backoff.insert(device.clone(), wait);
                        wait
                    };
                    next.insert(device, Instant::now() + wait);
                }
                _ = async {
                    match wake {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => {
                    let now = Instant::now();
                    now_due.extend(next.iter().filter(|(_, at)| **at <= now).map(|(device, _)| device.clone()));
                }
            }
            if now_due.is_empty() {
                continue;
            }
            let paired: HashSet<String> = self.paired().await.into_iter().collect();
            for device in now_due {
                if !paired.contains(&device) {
                    next.remove(&device);
                    continue;
                }
                // Not again until this attempt reports back.
                next.insert(device.clone(), Instant::now() + MAX_BACKOFF * 10);
                let inner = self.clone();
                let results = results.clone();
                tokio::spawn(async move {
                    let ok = inner.dial(&device).await;
                    let _ = results.send((device, ok));
                });
            }
        }
    }

    async fn paired(&self) -> Vec<String> {
        self.devices.paired_ids().await.unwrap_or_default()
    }

    /// Categories turned back on: their waiting changes apply now, and every
    /// device gets them again in full.
    async fn categories_changed(&self, engine: &SyncEngine) {
        let turned_on = engine.turned_on().await;
        if let Err(error) = exchange::replay(engine, &turned_on).await {
            tracing::warn!(%error, "categories not marked to send again");
        }
        if let Err(error) = engine.apply_pending().await {
            tracing::warn!(%error, "waiting changes not applied");
        }
    }

    fn claim(&self, device: &str) -> bool {
        self.busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(device.to_owned())
    }

    fn release(&self, device: &str) {
        self.busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(device);
        self.done.notify_waiters();
    }

    /// Connects to a device and exchanges. `true` when it went through.
    async fn dial(&self, device: &str) -> bool {
        let Some(engine) = self.engine() else {
            return false;
        };
        if !self.claim(device) {
            return true;
        }
        self.devices.set_sync_state(
            device,
            DeviceSync {
                syncing: true,
                error: None,
            },
        );
        let result = async {
            let mut session = self
                .devices
                .connect(device, ConnectionPurpose::Sync)
                .await?;
            let exchanged =
                exchange::exchange(&engine, device, &mut session, &Options::default()).await;
            session.end().await;
            exchanged
        }
        .await;
        self.finish(device, result.map(|_| ())).await
    }

    /// Records how an exchange with `device` ended.
    async fn finish(&self, device: &str, result: Result<()>) -> bool {
        let ok = result.is_ok();
        let error = match result {
            Ok(()) => {
                let _ = self.devices.record_sync(device, now_ms()).await;
                None
            }
            Err(error) => {
                tracing::debug!(%error, "sync with a device didn't go through");
                Some(error.to_string())
            }
        };
        self.devices.set_sync_state(
            device,
            DeviceSync {
                syncing: false,
                error,
            },
        );
        self.release(device);
        ok
    }
}

/// `sync` sessions a paired device opened.
struct Inbound(std::sync::Weak<Inner>);

#[async_trait]
impl caskade_sync::Sessions for Inbound {
    async fn opened(&self, mut session: DeviceSession) {
        let Some(inner) = self.0.upgrade() else {
            return session.end().await;
        };
        let device = session.peer().to_string();
        let Some(engine) = inner.engine() else {
            return session.end().await;
        };
        // Taken even while this device is dialling it: two exchanges at once
        // are safe (each change applies in its own transaction), and turning
        // one away when both dial at once would leave both backing off.
        inner.claim(&device);
        inner.devices.set_sync_state(
            &device,
            DeviceSync {
                syncing: true,
                error: None,
            },
        );
        let result = exchange::exchange(&engine, &device, &mut session, &Options::default()).await;
        session.end().await;
        inner.finish(&device, result.map(|_| ())).await;
    }
}
