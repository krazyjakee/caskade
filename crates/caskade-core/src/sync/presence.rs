//! Which games are running on which device (#240): presence, not stored data.
//!
//! Each `sync` exchange ends with both sides sending the sessions running on
//! them (`Presence` frame: game in the sender's ids, account, start time),
//! after the changes and their files. A device's presence is kept in memory
//! only, replaced by every exchange with it, and counts while it's fresh:
//! received within [`FRESH`], with the device's link up and its last sync
//! not failed, so a device that crashed or left never shows a game running
//! for long. A peer one build behind sends no frame: the exchange still ends
//! cleanly, and that device simply shows nothing running.
//!
//! A game starting or ending here asks every paired device to sync at once
//! ([`Presence::watch`]), so the others know within seconds. Reading
//! presence never touches the network, so it never holds up a launch. With
//! Play history set to This device only, presence is neither sent nor taken
//! in.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::engine::SyncEngine;
use super::policy::Category;
use crate::Result;
use crate::bus::{Bus, Event};
use crate::db::LibraryStore;
use crate::devices::{DeviceLink, Devices};

/// How long a device's presence counts without another exchange. Sync polls
/// each device a minute after the last exchange, so a device in reach
/// refreshes it well within this.
pub const FRESH: Duration = Duration::from_secs(180);
/// The frame's version; a newer one is read as far as this build knows it.
const VERSION: u32 = 1;
/// The longest presence frame this build reads.
const MAX_LINE: usize = 256 * 1024;
/// Sessions per frame.
const MAX_SESSIONS: usize = 64;

/// A game running on another device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Playing {
    /// The device it's running on.
    pub device_id: String,
    /// The game, in this device's ids.
    pub game_id: String,
    /// Its account there; empty when none was signed in.
    pub account_id: String,
    pub started_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Session {
    game: String,
    #[serde(default)]
    account: String,
    started: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message {
    Presence {
        #[serde(default)]
        version: u32,
        sessions: Vec<Session>,
    },
}

struct Received {
    playing: Vec<Playing>,
    at_ms: i64,
}

type Lookup = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Presence for this device and what paired devices said. Cheap to clone.
#[derive(Clone)]
pub struct Presence {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    account: RwLock<Lookup>,
    remote: Mutex<HashMap<String, Received>>,
    changed: tokio::sync::broadcast::Sender<()>,
}

impl Presence {
    pub fn new(store: LibraryStore) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                account: RwLock::new(Arc::new(|| None)),
                remote: Mutex::default(),
                changed: tokio::sync::broadcast::channel(16).0,
            }),
        }
    }

    /// The account playing here now.
    pub fn set_account(&self, account: impl Fn() -> Option<String> + Send + Sync + 'static) {
        *crate::write(&self.inner.account) = Arc::new(account);
    }

    /// Fires whenever another device's presence changes here.
    pub fn changes(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.inner.changed.subscribe()
    }

    /// Asks every paired device to sync when a game starts or ends here, so
    /// they learn of it within seconds. Runs until the bus closes.
    pub fn watch(
        &self,
        bus: &Bus,
        sync: super::SyncService,
        devices: Devices,
    ) -> tokio::task::JoinHandle<()> {
        let mut events = bus.stream();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(Event::GameStarted(_) | Event::GameExited(_)) => {
                        for device in devices.status().await.devices {
                            sync.sync_now(&device.id);
                        }
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    /// The sessions running here, in this device's ids.
    async fn local(&self) -> Result<Vec<Session>> {
        let account = (crate::read(&self.inner.account))().unwrap_or_default();
        Ok(self
            .inner
            .store
            .in_flight_sessions()
            .await?
            .into_iter()
            .take(MAX_SESSIONS)
            .map(|session| Session {
                game: session.game_id,
                account: account.clone(),
                started: session.started_at_ms,
            })
            .collect())
    }

    fn receive(&self, device: &str, playing: Vec<Playing>) {
        let changed = {
            let mut remote = crate::lock(&self.inner.remote);
            let before = remote.get(device).map(|received| received.playing.clone());
            remote.insert(
                device.to_owned(),
                Received {
                    playing: playing.clone(),
                    at_ms: crate::now_ms(),
                },
            );
            before.as_ref() != Some(&playing)
        };
        if changed {
            let _ = self.inner.changed.send(());
        }
    }

    /// What's running on other devices now, by what they last said: only
    /// fresh presence, from devices whose last sync didn't fail. Never
    /// waits on the network.
    pub async fn playing(&self, devices: &Devices) -> Vec<Playing> {
        let status = devices.status().await;
        let now = crate::now_ms();
        let fresh = i64::try_from(FRESH.as_millis()).unwrap_or(i64::MAX);
        let remote = crate::lock(&self.inner.remote);
        let mut playing: Vec<Playing> = status
            .devices
            .iter()
            .filter(|device| device.sync_error.is_none() && device.link != DeviceLink::Offline)
            .filter_map(|device| remote.get(&device.id))
            .filter(|received| now.saturating_sub(received.at_ms) < fresh)
            .flat_map(|received| received.playing.iter().cloned())
            .collect();
        playing
            .sort_by(|a, b| (&a.device_id, a.started_at_ms).cmp(&(&b.device_id, b.started_at_ms)));
        playing
    }

    /// Forgets what a device said (it was forgotten, say).
    pub fn forget(&self, device: &str) {
        let removed = crate::lock(&self.inner.remote).remove(device).is_some();
        if removed {
            let _ = self.inner.changed.send(());
        }
    }
}

/// Tells the device on the other end what's running here, and hears what's
/// running there. Both sides send, then read. Presence is best effort: a
/// peer that sends nothing (one build behind) or a dropped connection ends
/// it without an error, and that device's presence stays as it was.
pub async fn exchange(
    engine: &SyncEngine,
    peer: &str,
    writer: &mut caskade_sync::FrameWriter,
    reader: &mut caskade_sync::FrameReader,
) -> Result<()> {
    let presence = engine.presence();
    let allowed = engine.categories().await.allows(Category::PlaySessions);
    let mine = match (&presence, allowed) {
        (Some(presence), true) => presence.local().await.unwrap_or_else(|error| {
            tracing::warn!(%error, "running sessions unknown");
            Vec::new()
        }),
        _ => Vec::new(),
    };
    let sent = writer
        .write(&Message::Presence {
            version: VERSION,
            sessions: mine,
        })
        .await;
    if let Err(error) = sent {
        tracing::debug!(%error, "presence not sent");
        return Ok(());
    }
    let sessions = match reader.read::<Message>(MAX_LINE).await {
        Ok(Message::Presence { sessions, .. }) => sessions,
        Err(error) => {
            tracing::debug!(%error, "no presence from the other device");
            return Ok(());
        }
    };
    let (Some(presence), true) = (presence, allowed) else {
        return Ok(());
    };
    let mut playing = Vec::new();
    for session in sessions.into_iter().take(MAX_SESSIONS) {
        // A game this device doesn't know yet isn't one it could start.
        if let Some(game_id) = engine.game(peer, &session.game, false).await? {
            playing.push(Playing {
                device_id: peer.to_owned(),
                game_id,
                account_id: session.account,
                started_at_ms: session.started,
            });
        }
    }
    presence.receive(peer, playing);
    Ok(())
}
