//! One-off shares (#246, #249): a game shared once with another person
//! through a link that can be stopped, and that may expire.
//!
//! A share is the game's folder as an iroh-blobs collection in the copies'
//! send store (`Transfers::offer_folder`), and a grant of its own. The link
//! (`caskade_sync::share::ShareLink`) carries the sharer's address and that
//! grant. A `share` session is admitted only while the share runs
//! ([`Shares`] is the devices' share admission), so Stop sharing, or the
//! expiry, refuses the link at once; sessions already open are told
//! `Stopped` and closed, and every gate revokes the collection, which stops
//! transfers in flight. Copies already received stay.
//!
//! The session carries what the share is and how it's going (`wire`); the
//! files never do. They travel as in copies, over a relay-free dialer to
//! direct addresses only: the sharer's as the session found them (iroh's
//! hole punching and port mapping) and as it states them. A recipient that
//! can't reach the sharer directly waits, saying so.
//!
//! Recipients serve each other (#249): while one receives, and after if it
//! chose to keep helping, it tells the sharer it serves, and the sharer
//! names it to the share's other recipients and tells it whom to let fetch
//! the collection. A recipient serves only while its session with the sharer
//! is open: a stopped share closes that session everywhere.
//!
//! Shares are this device's alone: `shares` and `share_recipients` never
//! sync and are never backed up (their grant is a credential).

mod items;
mod receive;
mod serve;
mod wire;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, SystemTime};

use caskade_sync::share::ShareLink;
use caskade_sync::{EndpointId, Grant};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::db::{LibraryStore, ShareRow};
use crate::devices::Devices;
use crate::install::InstallService;
use crate::transfer::Transfers;
use crate::{Error, Result, lock};

pub use items::{EmulatorInstaller, GameHere, ShareContents, ShareItem, SharedGame};
pub use receive::{ReceivedShare, ShareInfo, ShareReach};

/// Whether `text` reads as a share link (`caskade://receive?t=…`).
pub fn is_link(text: &str) -> bool {
    ShareLink::parse(text).is_ok()
}

/// How long a link may last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ShareExpiry {
    Day,
    Week,
    /// Until it's stopped.
    Never,
}

impl ShareExpiry {
    fn duration(self) -> Option<Duration> {
        match self {
            Self::Day => Some(Duration::from_secs(24 * 3600)),
            Self::Week => Some(Duration::from_secs(7 * 24 * 3600)),
            Self::Never => None,
        }
    }
}

/// How a share is going on this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ShareState {
    /// Reading the game's files.
    Preparing { done: u64, total: u64 },
    /// Its link works.
    Ready,
    /// Its files couldn't be read.
    Failed { reason: String },
    Stopped,
    Expired,
}

/// Someone who opened a share's link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareRecipient {
    pub name: String,
    pub done: u64,
    pub finished: bool,
    /// Its session is open now.
    pub connected: bool,
    /// It serves the share to its other recipients.
    pub serving: bool,
}

/// A share as the app shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareRecord {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub game_id: Option<String>,
    pub entry_id: Option<String>,
    pub size: Option<u64>,
    pub created_at_ms: i64,
    pub expires_at_ms: Option<i64>,
    pub stopped_at_ms: Option<i64>,
    pub state: ShareState,
    pub recipients: Vec<ShareRecipient>,
}

/// What a share's files are while it runs.
#[derive(Debug, Clone)]
enum Prepared {
    Reading { done: u64, total: u64 },
    Ready(Ready),
    Failed(String),
}

#[derive(Debug, Clone)]
struct Ready {
    root: String,
    size: u64,
    files: u64,
    launch: Option<String>,
}

/// A recipient connected to a running share.
struct Connected {
    out: mpsc::UnboundedSender<wire::Message>,
    /// Where it serves the share, when it does.
    serving: Option<Vec<SocketAddr>>,
}

/// A share that runs: its link works.
struct Running {
    grant: Grant,
    expires_at_ms: Option<i64>,
    prepared: watch::Sender<Prepared>,
    connected: HashMap<EndpointId, Connected>,
    /// Recipients that let in whom they were asked to.
    allowed: broadcast::Sender<EndpointId>,
    stop: CancellationToken,
}

/// One-off shares, both ways. Cheap to clone.
#[derive(Clone)]
pub struct Shares {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    devices: Devices,
    transfers: Transfers,
    install: InstallService,
    /// Where received shares are put together before they're installed.
    incoming_dir: PathBuf,
    running: Mutex<HashMap<String, Running>>,
    /// Shares being received, or served after, by install job.
    received: Mutex<HashMap<String, receive::Received>>,
    changed: broadcast::Sender<()>,
    /// Where someone receiving a share is announced (a notification).
    bus: std::sync::OnceLock<crate::bus::Bus>,
    /// Saves and emulator setup, for what else is shared (#248).
    items: items::Parts,
}

/// The tag a share's collection is kept under in the send store.
fn share_tag(id: &str) -> String {
    format!("share/{id}")
}

impl Shares {
    /// Shares over `devices`, with `transfers`' store and gate. Call before
    /// the devices' endpoint starts.
    pub fn new(
        store: LibraryStore,
        devices: Devices,
        transfers: Transfers,
        install: InstallService,
        cache: &Path,
    ) -> Self {
        let shares = Self {
            inner: Arc::new(Inner {
                store,
                devices: devices.clone(),
                transfers,
                install,
                incoming_dir: cache.join("shares").join("incoming"),
                running: Mutex::default(),
                received: Mutex::default(),
                changed: broadcast::channel(16).0,
                bus: std::sync::OnceLock::new(),
                items: items::Parts::default(),
            }),
        };
        let weak = Arc::downgrade(&shares.inner);
        devices.set_share_admission(Arc::new(move |_, grant| admits(&weak, grant)));
        devices.set_share_handler(Arc::new(serve::Serve(Arc::downgrade(&shares.inner))));
        shares
    }

    /// Runs the shares that were running when Caskade last quit: their
    /// files are read again in the background, and their links work again.
    pub async fn resume(&self) -> Result<()> {
        let now = crate::now_ms();
        for row in self.inner.store.shares().await? {
            if row.running(now) && !lock(&self.inner.running).contains_key(&row.id) {
                self.run(row);
            }
        }
        Ok(())
    }

    /// Announces on `bus` when someone received everything of a share
    /// (`Event::ShareReceived`, #247).
    pub fn set_bus(&self, bus: crate::bus::Bus) {
        let _ = self.inner.bus.set(bus);
    }

    /// A signal on every change to [`Shares::list`] or what's received.
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.inner.changed.subscribe()
    }

    fn changed(&self) {
        let _ = self.inner.changed.send(());
    }

    /// Shares release `entry_id` (installed here) until `expiry`, and
    /// returns the share; its link is [`Shares::link`]. Its files are read
    /// in the background meanwhile.
    pub async fn share(&self, entry_id: &str, expiry: ShareExpiry) -> Result<ShareRecord> {
        // An emulated release goes as its ROM, never its folder (#279).
        if let Some(release) = self.inner.store.get_entry(entry_id).await?
            && crate::transfer::rom::emulated(&release)
        {
            let item = ShareItem::Rom {
                entry_id: entry_id.to_owned(),
            };
            return self.share_item(item, expiry, "").await;
        }
        let (_, title, _, _) = self.inner.transfers.source(entry_id).await?;
        let entry = self
            .inner
            .store
            .get_entry(entry_id)
            .await?
            .ok_or_else(|| Error::Invalid(crate::tr!("that game isn't installed on this device")))?;
        let now = crate::now_ms();
        let row = ShareRow {
            id: crate::model::new_id(),
            kind: "game".into(),
            game_id: Some(entry.game_id.clone()),
            entry_id: Some(entry_id.to_owned()),
            title,
            size: entry.size.map_or(0, |size| i64::try_from(size).unwrap_or(i64::MAX)),
            root: None,
            extras_root: None,
            grant: Grant::generate().to_hex(),
            created_at_ms: now,
            expires_at_ms: expiry.duration().map(|until| {
                now.saturating_add(i64::try_from(until.as_millis()).unwrap_or(i64::MAX))
            }),
            stopped_at_ms: None,
            details_json: "{}".into(),
        };
        self.inner.store.insert_share(&row).await?;
        self.run(row.clone());
        self.changed();
        self.record(row).await
    }

    /// Share `id`'s link, with this device's addresses now (local ones, and
    /// the relay once there is one). Only for a share that runs.
    pub async fn link(&self, id: &str) -> Result<String> {
        let row = self.row(id).await?;
        if !row.running(crate::now_ms()) {
            return Err(Error::Invalid(crate::tr!(
                "this share has stopped; share the game again for a new link"
            )));
        }
        let grant: Grant = row
            .grant
            .parse()
            .map_err(|_| Error::Invalid(crate::tr!("this share's link is damaged")))?;
        let addr = self.inner.devices.link_addr(Duration::from_secs(3)).await?;
        let expires = row.expires_at_ms.map(|at| {
            SystemTime::UNIX_EPOCH + Duration::from_millis(u64::try_from(at).unwrap_or(0))
        });
        Ok(ShareLink::new(addr, grant, expires).to_url())
    }

    /// Stops share `id`: its link fails from now on, also part-way through
    /// a transfer. What was received stays.
    pub async fn stop(&self, id: &str) -> Result<()> {
        self.row(id).await?;
        self.inner.store.stop_share(id, crate::now_ms()).await?;
        self.end(id).await;
        self.changed();
        Ok(())
    }

    /// Forgets a share that has stopped or expired, and who received it.
    pub async fn forget(&self, id: &str) -> Result<()> {
        let row = self.row(id).await?;
        if row.running(crate::now_ms()) {
            return Err(Error::Invalid(crate::tr!("stop sharing it first")));
        }
        self.inner.store.delete_share(id).await?;
        self.changed();
        Ok(())
    }

    /// Every share, the newest first.
    pub async fn list(&self) -> Result<Vec<ShareRecord>> {
        let mut records = Vec::new();
        for row in self.inner.store.shares().await? {
            records.push(self.record(row).await?);
        }
        Ok(records)
    }

    async fn row(&self, id: &str) -> Result<ShareRow> {
        self.inner
            .store
            .share(id)
            .await?
            .ok_or_else(|| Error::Invalid(crate::tr!("there's no such share")))
    }

    async fn record(&self, row: ShareRow) -> Result<ShareRecord> {
        let now = crate::now_ms();
        let (state, live, size) = {
            let running = lock(&self.inner.running);
            match running.get(&row.id) {
                Some(share) if row.running(now) => {
                    let (state, size) = match &*share.prepared.borrow() {
                        Prepared::Reading { done, total } => (
                            ShareState::Preparing {
                                done: *done,
                                total: *total,
                            },
                            None,
                        ),
                        Prepared::Ready(ready) => (ShareState::Ready, Some(ready.size)),
                        Prepared::Failed(reason) => (
                            ShareState::Failed {
                                reason: reason.clone(),
                            },
                            None,
                        ),
                    };
                    let live: HashMap<String, bool> = share
                        .connected
                        .iter()
                        .map(|(peer, connected)| (peer.to_string(), connected.serving.is_some()))
                        .collect();
                    (state, live, size)
                }
                _ if row.stopped_at_ms.is_some() => (ShareState::Stopped, HashMap::new(), None),
                _ if !row.running(now) => (ShareState::Expired, HashMap::new(), None),
                _ => (
                    ShareState::Preparing { done: 0, total: 0 },
                    HashMap::new(),
                    None,
                ),
            }
        };
        let recipients = self
            .inner
            .store
            .share_recipients(&row.id)
            .await?
            .into_iter()
            .map(|recipient| ShareRecipient {
                connected: live.contains_key(&recipient.endpoint_id),
                serving: live.get(&recipient.endpoint_id).copied().unwrap_or(false),
                name: recipient.name,
                done: u64::try_from(recipient.done_bytes).unwrap_or(0),
                finished: recipient.finished_at_ms.is_some(),
            })
            .collect();
        Ok(ShareRecord {
            size: size.or_else(|| u64::try_from(row.size).ok().filter(|size| *size > 0)),
            id: row.id,
            kind: row.kind,
            title: row.title,
            game_id: row.game_id,
            entry_id: row.entry_id,
            created_at_ms: row.created_at_ms,
            expires_at_ms: row.expires_at_ms,
            stopped_at_ms: row.stopped_at_ms,
            state,
            recipients,
        })
    }

    /// Makes `row`'s link work: reads its files in the background, and ends
    /// it when it expires.
    fn run(&self, row: ShareRow) {
        let Ok(grant) = row.grant.parse::<Grant>() else {
            tracing::warn!(share = %row.id, "a share's grant is damaged; it stays stopped");
            return;
        };
        let (prepared, _) = watch::channel(Prepared::Reading { done: 0, total: 0 });
        let stop = CancellationToken::new();
        lock(&self.inner.running).insert(
            row.id.clone(),
            Running {
                grant,
                expires_at_ms: row.expires_at_ms,
                prepared: prepared.clone(),
                connected: HashMap::new(),
                allowed: broadcast::channel(16).0,
                stop: stop.clone(),
            },
        );
        let this = self.clone();
        tokio::spawn(async move {
            let reading = this.prepare(&row, &prepared);
            let expiry = async {
                match row.expires_at_ms {
                    Some(at) => {
                        let left = u64::try_from(at - crate::now_ms()).unwrap_or(0);
                        tokio::time::sleep(Duration::from_millis(left)).await;
                    }
                    None => std::future::pending().await,
                }
            };
            tokio::pin!(expiry);
            tokio::select! {
                _ = stop.cancelled() => return,
                _ = &mut expiry => {}
                () = reading => {
                    tokio::select! {
                        _ = stop.cancelled() => return,
                        _ = &mut expiry => {}
                    }
                }
            }
            // Expired: ended like a stop, without saying it was stopped.
            this.end(&row.id).await;
            this.changed();
        });
    }

    /// Reads `row`'s files into the send store, saying how far it got.
    async fn prepare(&self, row: &ShareRow, prepared: &watch::Sender<Prepared>) {
        let outcome = async {
            let (folder, launch) = if row.kind == "game" {
                let entry = row
                    .entry_id
                    .as_deref()
                    .ok_or_else(|| Error::Invalid(crate::tr!("this share has no game")))?;
                let (folder, _, launch, _) = self.inner.transfers.source(entry).await?;
                (folder, launch)
            } else {
                match self.staged(row).await? {
                    Some(folder) => (folder, None),
                    // A collection: only its list, no files.
                    None => {
                        return Ok(Ready {
                            root: String::new(),
                            size: 0,
                            files: 0,
                            launch: None,
                        });
                    }
                }
            };
            let this = self.clone();
            let reporter = prepared.clone();
            let offered = self
                .inner
                .transfers
                .offer_folder(&folder, &share_tag(&row.id), move |done, total| {
                    reporter.send_replace(Prepared::Reading { done, total });
                    let _ = this.inner.changed.send(());
                })
                .await?;
            self.inner
                .store
                .set_share_root(
                    &row.id,
                    &offered.root,
                    i64::try_from(offered.size).unwrap_or(i64::MAX),
                )
                .await?;
            Ok::<_, Error>(Ready {
                root: offered.root,
                size: offered.size,
                files: offered.files,
                launch,
            })
        }
        .await;
        prepared.send_replace(match outcome {
            Ok(ready) => Prepared::Ready(ready),
            Err(error) => {
                tracing::warn!(share = %row.id, %error, "a share's files couldn't be read");
                Prepared::Failed(error.to_string())
            }
        });
        self.changed();
    }

    /// Ends share `id` here: its sessions are told and closed, and every
    /// recipient loses the collection.
    async fn end(&self, id: &str) {
        let Some(share) = lock(&self.inner.running).remove(id) else {
            return;
        };
        share.stop.cancel();
        if let Prepared::Ready(ready) = &*share.prepared.borrow() {
            for peer in share.connected.keys() {
                self.inner.transfers.revoke(*peer, &ready.root);
            }
        }
        let _ = self.inner.transfers.release(&share_tag(id)).await;
        self.unstage(id).await;
    }
}

/// Whether `grant` opens a running share now.
fn admits(inner: &Weak<Inner>, grant: &Grant) -> bool {
    let Some(inner) = inner.upgrade() else {
        return false;
    };
    let now = crate::now_ms();
    lock(&inner.running)
        .values()
        .any(|share| share.grant == *grant && share.expires_at_ms.is_none_or(|at| now < at))
}
