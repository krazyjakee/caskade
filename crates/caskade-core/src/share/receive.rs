//! The recipient's side: look at a link, then receive it as a copy
//! (`InstallService::copy_with`), from the sharer and the share's other
//! recipients that serve it, and serve it in turn while receiving (and
//! after, when chosen).

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use caskade_installer::exec::CancellationToken as InstallCancel;
use caskade_sync::share::ShareLink;
use caskade_sync::{EndpointId, Purpose, Session};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::Shares;
use super::items::{Details, ShareContents};
use super::wire::{MAX_ABOUT, MAX_LINE, Message};
use crate::install::{
    FetchFailure, Fetched, JobSnapshot, PeerCopyTarget, PeerFetcher, PeerProgress,
};
use crate::{Error, Result, lock};

/// How long the sharer gets for each frame: reading a large game takes a
/// while, and it says how far it got meanwhile.
const FRAME_WAIT: Duration = Duration::from_secs(300);
/// How long the sharer gets to answer a look at its link.
const ABOUT_WAIT: Duration = Duration::from_secs(30);

/// How the link reached the sharer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ShareReach {
    /// Directly: the game can come.
    Direct,
    /// Only through a relay: the game waits for a direct connection, since
    /// game files never pass through one.
    RelayOnly,
}

/// What a link shares, as its sharer says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareInfo {
    pub sharer_id: String,
    pub sharer: String,
    pub kind: String,
    pub title: String,
    pub size: Option<u64>,
    /// A Windows build.
    pub windows: bool,
    /// What comes with the game.
    pub extras: Vec<String>,
    pub expires_at_ms: Option<i64>,
    pub reach: ShareReach,
    /// What it holds, and what of it this library has (#248).
    pub contents: ShareContents,
}

/// A share received here, or being received, as the app shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedShare {
    /// Its install job.
    pub job: String,
    pub title: String,
    pub sharer: String,
    /// Still arriving.
    pub receiving: bool,
    /// Serves the share to its other recipients after it's here.
    pub keep_helping: bool,
    /// Serves it now.
    pub serving: bool,
    /// Recipients it lets fetch the share.
    pub helping: usize,
}

/// A share received here, while it arrives or is served.
pub(super) struct Received {
    title: String,
    sharer: String,
    root: Option<String>,
    receiving: bool,
    keep_helping: bool,
    serving: bool,
    /// Installed: its files left the staging folder.
    installed: bool,
    allowed: HashSet<EndpointId>,
    /// Ends serving after it arrived.
    end: CancellationToken,
}

impl Received {
    /// A job whose files haven't started coming.
    fn queued() -> Self {
        Self {
            title: String::new(),
            sharer: String::new(),
            root: None,
            receiving: true,
            keep_helping: false,
            serving: false,
            installed: false,
            allowed: HashSet::new(),
            end: CancellationToken::new(),
        }
    }
}

fn tag(job: &str) -> String {
    format!("share/in/{job}")
}

/// `at_ms` as a date, `YYYY-MM-DD`.
fn date(at_ms: i64) -> String {
    let (year, month, day) = crate::civil_from_days(at_ms.div_euclid(86_400_000));
    format!("{year:04}-{month:02}-{day:02}")
}

fn expired(link: &ShareLink) -> Error {
    let at = link
        .expires_at()
        .and_then(|at| at.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX));
    Error::Invalid(crate::tr!(
        "This link expired on {date}. Ask whoever shared it for a new one.",
        date = date(at)
    ))
}

fn stopped() -> Error {
    Error::Invalid(crate::tr!(
        "This link doesn't work any more: the share was stopped or has expired. Ask whoever shared it for a new one."
    ))
}

fn unreachable() -> String {
    crate::tr!(
        "Can't reach whoever shared this: their Caskade may be closed or offline. Caskade keeps trying."
    )
}

fn relay_only(sharer: &str) -> String {
    crate::tr!(
        "Only a relay reaches {sharer}'s computer: the game waits for a direct connection",
        sharer = sharer
    )
}

/// The sharer refused the session: the link's share has stopped or expired.
fn is_refusal(error: &Error) -> bool {
    matches!(error, Error::Forbidden(_))
}

impl Shares {
    /// Opens a share session with `link`'s sharer.
    async fn open(&self, link: &ShareLink) -> std::result::Result<Session, FetchFailure> {
        if link.expired(SystemTime::now()) {
            return Err(FetchFailure::Failed(expired(link)));
        }
        self.inner
            .devices
            .connect_to(link.addr().clone(), Purpose::Share, Some(link.grant()))
            .await
            .map_err(|error| {
                if is_refusal(&error) {
                    FetchFailure::Failed(stopped())
                } else {
                    FetchFailure::Waiting(unreachable())
                }
            })
    }

    async fn hello(&self, session: &mut Session, fetch: bool) -> bool {
        let name = self.inner.devices.status().await.this_device.name;
        session
            .writer()
            .write(&Message::Hello { name, fetch })
            .await
            .is_ok()
    }

    /// What `text`, a share link, shares: asks its sharer. Fails with the
    /// reason when the link expired, was stopped, or its sharer can't be
    /// reached.
    pub async fn inspect(&self, text: &str) -> Result<ShareInfo> {
        self.look(text).await.map(|(info, _)| info)
    }

    /// [`Shares::inspect`], with what a share other than a game is.
    pub(super) async fn look(&self, text: &str) -> Result<(ShareInfo, Option<Details>)> {
        let link = ShareLink::parse(text)
            .map_err(|_| Error::Invalid(crate::tr!("that isn't a Caskade share link")))?;
        let mut session = self.open(&link).await.map_err(|failure| match failure {
            FetchFailure::Failed(error) => error,
            FetchFailure::Waiting(why) => Error::Invalid(why),
        })?;
        if !self.hello(&mut session, false).await {
            session.close();
            return Err(Error::Invalid(unreachable()));
        }
        let about = tokio::time::timeout(ABOUT_WAIT, session.reader().read(MAX_ABOUT)).await;
        let reach = if session.direct_addrs().is_empty() {
            ShareReach::RelayOnly
        } else {
            ShareReach::Direct
        };
        session.end().await;
        match about {
            Ok(Ok(Message::About {
                kind,
                title,
                size,
                windows,
                sharer,
                expires_at_ms,
                extras,
                details,
            })) => {
                let details = Details::read(details.as_ref());
                if kind != "game" && details.is_none() {
                    return Err(Error::Invalid(crate::tr!(
                        "This Caskade can't receive what that link shares. Update Caskade, then try again."
                    )));
                }
                let contents = self.contents(details.as_ref()).await?;
                let info = ShareInfo {
                    sharer_id: link.peer().to_string(),
                    sharer,
                    kind,
                    title,
                    size,
                    windows,
                    extras,
                    expires_at_ms,
                    reach,
                    contents,
                };
                Ok((info, details))
            }
            _ => Err(Error::Invalid(unreachable())),
        }
    }

    /// Receives the share `text` links to into a storage root here (the
    /// default when `root_id` is `None`), as a copy job: the game arrives as
    /// a game of its own, installed like any copy.
    pub async fn receive(&self, text: &str, root_id: Option<String>) -> Result<JobSnapshot> {
        let (info, details) = self.look(text).await?;
        if matches!(details, Some(Details::Collection { .. })) {
            return Err(Error::Invalid(crate::tr!(
                "That link shares a collection: add it to your collections instead."
            )));
        }
        let link = ShareLink::parse(text)
            .map_err(|_| Error::Invalid(crate::tr!("that isn't a Caskade share link")))?;
        let fetcher = Arc::new(ShareFetcher {
            shares: self.clone(),
            link,
            sharer: info.sharer.clone(),
            details,
        });
        self.inner
            .install
            .copy_with(
                fetcher,
                PeerCopyTarget {
                    device_id: info.sharer_id,
                    device_name: info.sharer,
                    entry_id: String::new(),
                    // None here: the copy becomes a game of its own.
                    game_id: String::new(),
                    title: info.title,
                },
                root_id,
            )
            .await
    }

    /// Shares received here that are arriving or served.
    pub fn received(&self) -> Vec<ReceivedShare> {
        let install = &self.inner.install;
        let mut received: Vec<ReceivedShare> = lock(&self.inner.received)
            .iter()
            // A job that ended without its files installed leaves nothing.
            .filter(|(job, received)| {
                received.serving
                    || install
                        .job(job)
                        .is_some_and(|job| !job.status.is_finished())
            })
            .map(|(job, received)| ReceivedShare {
                job: job.clone(),
                title: received.title.clone(),
                sharer: received.sharer.clone(),
                receiving: received.receiving,
                keep_helping: received.keep_helping,
                serving: received.serving,
                helping: received.allowed.len(),
            })
            .collect();
        received.sort_by(|a, b| a.job.cmp(&b.job));
        received
    }

    /// Whether the share received as `job` goes on serving its other
    /// recipients once it's here (it always does while it arrives).
    pub fn keep_helping(&self, job: &str, on: bool) -> Result<()> {
        let queued = self
            .inner
            .install
            .job(job)
            .is_some_and(|job| !job.status.is_finished());
        let mut received = lock(&self.inner.received);
        if !received.contains_key(job) && !queued {
            return Err(Error::Invalid(crate::tr!("that share isn't here any more")));
        }
        // A job whose files haven't started coming yet takes it when they do.
        let share = received.entry(job.to_owned()).or_insert_with(Received::queued);
        if on && !share.receiving && !share.serving {
            return Err(Error::Invalid(crate::tr!(
                "it's here already: it can help others only if that's chosen while it arrives"
            )));
        }
        share.keep_helping = on;
        if !on && !share.receiving {
            share.end.cancel();
        }
        drop(received);
        self.changed();
        Ok(())
    }

    /// Lets `peers` fetch what job `job` received.
    fn let_in(&self, job: &str, root: &str, peers: &[String]) {
        let mut received = lock(&self.inner.received);
        let Some(share) = received.get_mut(job) else {
            return;
        };
        for peer in peers {
            if let Ok(peer) = peer.parse::<EndpointId>()
                && self.inner.transfers.allow(peer, root).is_ok()
            {
                share.allowed.insert(peer);
            }
        }
    }

    /// Job `job` stops serving: everyone it let in loses the share, and its
    /// files go once installed.
    async fn stop_serving(&self, job: &str) {
        let (root, allowed, release) = {
            let mut received = lock(&self.inner.received);
            let Some(share) = received.get_mut(job) else {
                return;
            };
            share.serving = false;
            let allowed: Vec<EndpointId> = share.allowed.drain().collect();
            let root = share.root.clone();
            let release = share.installed;
            if release {
                received.remove(job);
            }
            (root, allowed, release)
        };
        if let Some(root) = root {
            for peer in allowed {
                self.inner.transfers.revoke(peer, &root);
            }
        }
        if release {
            let _ = self.inner.transfers.release(&tag(job)).await;
        }
        self.changed();
    }

    /// This device's own direct addresses, for the share's other recipients.
    async fn own_addrs(&self) -> Vec<SocketAddr> {
        self.inner
            .devices
            .link_addr(Duration::ZERO)
            .await
            .map(|addr| addr.ip_addrs().copied().collect())
            .unwrap_or_default()
    }

    /// After it arrived: serves the share on `session` until the sharer
    /// stops it, the session goes, or the user stops helping.
    async fn serve_after(&self, job: String, root: String, mut session: Session) {
        let end = match lock(&self.inner.received).get(&job) {
            Some(share) => share.end.clone(),
            None => return session.close(),
        };
        loop {
            tokio::select! {
                _ = end.cancelled() => {
                    let _ = session.writer().write(&Message::Serving { addrs: Vec::new() }).await;
                    break;
                }
                frame = session.reader().read::<Message>(MAX_LINE) => match frame {
                    Ok(Message::Allow { peers }) => {
                        self.let_in(&job, &root, &peers);
                        let _ = session.writer().write(&Message::Allowed).await;
                        self.changed();
                    }
                    Ok(Message::Stopped) | Err(_) => break,
                    Ok(_) => {}
                },
            }
        }
        // Released below, since it's installed by now or soon.
        if let Some(share) = lock(&self.inner.received).get_mut(&job) {
            share.keep_helping = false;
        }
        self.stop_serving(&job).await;
        session.end().await;
    }
}

/// Brings a received share's files, for its copy job.
struct ShareFetcher {
    shares: Shares,
    link: ShareLink,
    sharer: String,
    /// What a share other than a game is: put where it belongs, not
    /// installed (#248).
    details: Option<Details>,
}

/// What the sharer offered.
struct Offered {
    root: String,
    size: u64,
    launch: Option<String>,
    addrs: Vec<SocketAddr>,
}

impl ShareFetcher {
    /// Reads frames until the offer, reporting the sharer's reading.
    async fn offered(
        &self,
        session: &mut Session,
        progress: &(dyn Fn(PeerProgress) + Send + Sync),
    ) -> std::result::Result<Offered, FetchFailure> {
        loop {
            let frame = tokio::time::timeout(FRAME_WAIT, session.reader().read(MAX_LINE)).await;
            match frame {
                Ok(Ok(Message::Preparing { done, total })) => {
                    progress(PeerProgress::Preparing { done, total });
                }
                Ok(Ok(Message::Offer {
                    root,
                    size,
                    launch,
                    addrs,
                    ..
                })) => {
                    return Ok(Offered {
                        root,
                        size,
                        launch,
                        addrs,
                    });
                }
                Ok(Ok(Message::Refused { reason })) => {
                    return Err(FetchFailure::Failed(Error::Invalid(reason)));
                }
                Ok(Ok(Message::Stopped)) => return Err(FetchFailure::Failed(stopped())),
                Ok(Ok(_)) => continue,
                _ => return Err(FetchFailure::Waiting(unreachable())),
            }
        }
    }

    /// Whether there's room for what's still to come, and the install.
    fn room(&self, size: u64, have: u64, root: &Path) -> std::result::Result<(), FetchFailure> {
        let incoming = &self.shares.inner.incoming_dir;
        let _ = std::fs::create_dir_all(incoming);
        let cache_free = fs2::available_space(incoming).unwrap_or(u64::MAX);
        let root_free = fs2::available_space(root).unwrap_or(u64::MAX);
        let to_come = size.saturating_sub(have);
        let same_disk =
            fs2::total_space(incoming).ok() == fs2::total_space(root).ok() && cache_free == root_free;
        let short = if same_disk {
            to_come.saturating_add(size).saturating_sub(root_free)
        } else {
            to_come
                .saturating_sub(cache_free)
                .max(size.saturating_sub(root_free))
        };
        if short > 0 {
            return Err(FetchFailure::Failed(Error::Invalid(crate::tr!(
                "there isn't enough room for the game: {short} more bytes are needed",
                short = short
            ))));
        }
        Ok(())
    }
}

#[async_trait]
impl PeerFetcher for ShareFetcher {
    async fn fetch(
        &self,
        target: &PeerCopyTarget,
        root: &Path,
        job: &str,
        _cancel: InstallCancel,
        progress: Arc<dyn Fn(PeerProgress) + Send + Sync>,
    ) -> std::result::Result<Fetched, FetchFailure> {
        let shares = &self.shares;
        let mut session = shares.open(&self.link).await?;
        if !shares.hello(&mut session, true).await {
            session.close();
            return Err(FetchFailure::Waiting(unreachable()));
        }
        let offered = match self.offered(&mut session, progress.as_ref()).await {
            Ok(offered) => offered,
            Err(failure) => {
                session.close();
                return Err(failure);
            }
        };
        let others = match tokio::time::timeout(ABOUT_WAIT, session.reader().read(MAX_LINE)).await
        {
            Ok(Ok(Message::Providers { list })) => list,
            _ => Vec::new(),
        };
        let store = shares.inner.transfers.send_store();
        let have = store
            .local_bytes(&offered.root)
            .await
            .unwrap_or(0)
            .min(offered.size);
        self.room(offered.size, have, root)?;

        // The sharer at every direct address known for it; never a relay.
        let mut addrs = session.direct_addrs();
        addrs.extend(offered.addrs.iter().copied());
        addrs.extend(self.link.addr().ip_addrs().copied());
        addrs.sort();
        addrs.dedup();
        if session.direct_addrs().is_empty() && offered.addrs.is_empty() {
            session.close();
            return Err(FetchFailure::Waiting(relay_only(&self.sharer)));
        }
        let mut providers = vec![(self.link.peer(), addrs)];
        providers.extend(
            others
                .into_iter()
                .filter_map(|(id, addrs)| Some((id.parse::<EndpointId>().ok()?, addrs))),
        );
        let dialer = shares
            .inner
            .devices
            .swarm_dialer()
            .await
            .map_err(FetchFailure::Failed)?;

        // Serves what it has to the share's other recipients meanwhile.
        {
            let mut received = lock(&shares.inner.received);
            let share = received
                .entry(job.to_owned())
                .or_insert_with(Received::queued);
            share.title = target.title.clone();
            share.sharer = self.sharer.clone();
            share.root = Some(offered.root.clone());
            share.receiving = true;
            share.serving = true;
        }
        let own = shares.own_addrs().await;
        if !own.is_empty() {
            let _ = session
                .writer()
                .write(&Message::Serving { addrs: own })
                .await;
        }
        shares.changed();
        progress(PeerProgress::Receiving {
            done: have,
            total: offered.size,
        });

        let size = offered.size;
        let done = Arc::new(std::sync::atomic::AtomicU64::new(have));
        let in_tag = tag(job);
        let fetching = {
            let done = done.clone();
            let report = progress.clone();
            caskade_sync::swarm::fetch_from(
                &store,
                &dialer,
                &providers,
                &offered.root,
                &in_tag,
                move |bytes| {
                    let now = have.saturating_add(bytes).min(size);
                    done.store(now, std::sync::atomic::Ordering::Relaxed);
                    report(PeerProgress::Receiving { done: now, total: size });
                },
            )
        };
        tokio::pin!(fetching);
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let outcome = loop {
            tokio::select! {
                fetched = &mut fetching => break fetched.map_err(|error| error.to_string()),
                frame = session.reader().read::<Message>(MAX_LINE) => match frame {
                    Ok(Message::Allow { peers }) => {
                        shares.let_in(job, &offered.root, &peers);
                        let _ = session.writer().write(&Message::Allowed).await;
                        shares.changed();
                    }
                    Ok(Message::Stopped) => {
                        shares.stop_serving(job).await;
                        session.close();
                        return Err(FetchFailure::Failed(stopped()));
                    }
                    Ok(_) => {}
                    Err(_) => break Err("the sharer went".into()),
                },
                _ = tick.tick() => {
                    let done = done.load(std::sync::atomic::Ordering::Relaxed);
                    let _ = session.writer().write(&Message::Progress { done }).await;
                }
            }
        };
        if let Err(error) = outcome {
            tracing::debug!(%error, "a share stopped arriving; what arrived stays");
            // Stopped, or gone: the next frame says which.
            let next = tokio::time::timeout(
                Duration::from_secs(2),
                session.reader().read::<Message>(MAX_LINE),
            )
            .await;
            shares.stop_serving(job).await;
            session.close();
            return Err(match next {
                Ok(Ok(Message::Stopped)) => FetchFailure::Failed(stopped()),
                _ => FetchFailure::Waiting(unreachable()),
            });
        }
        let _ = session
            .writer()
            .write(&Message::Progress { done: size })
            .await;
        let _ = session.writer().write(&Message::Done).await;

        // Kept helping: the store keeps its own copy of the files to serve
        // them; otherwise they move out of it into the install.
        let keep = {
            let mut received = lock(&shares.inner.received);
            match received.get_mut(job) {
                Some(share) => {
                    share.receiving = false;
                    share.keep_helping
                }
                None => false,
            }
        };
        let staging = shares.inner.incoming_dir.join(job);
        let folder = staging.join(crate::transfer::folder_name(&target.title));
        let _ = std::fs::remove_dir_all(&staging);
        let transfers = &shares.inner.transfers;
        let exported = if keep {
            transfers.export_copy(&offered.root, &folder).await
        } else {
            transfers.export(&offered.root, &folder).await
        };
        if keep && exported.is_ok() {
            let this = shares.clone();
            let (job, root) = (job.to_owned(), offered.root.clone());
            tokio::spawn(async move { this.serve_after(job, root, session).await });
        } else {
            let _ = session
                .writer()
                .write(&Message::Serving { addrs: Vec::new() })
                .await;
            shares.stop_serving(job).await;
            session.end().await;
        }
        exported.map_err(FetchFailure::Failed)?;
        let placed = match &self.details {
            Some(details) => Some(
                shares
                    .place(details, &folder, root, &target.title)
                    .await
                    .map_err(FetchFailure::Failed)?,
            ),
            None => None,
        };
        Ok(Fetched {
            folder,
            title: target.title.clone(),
            launch: offered.launch,
            placed,
        })
    }

    async fn finished(&self, _root: &Path, job: &str) {
        let shares = &self.shares;
        let _ = tokio::fs::remove_dir_all(shares.inner.incoming_dir.join(job)).await;
        let release = {
            let mut received = lock(&shares.inner.received);
            match received.get_mut(job) {
                Some(share) if share.serving => {
                    share.installed = true;
                    false
                }
                _ => {
                    received.remove(job);
                    true
                }
            }
        };
        if release {
            let _ = shares.inner.transfers.release(&tag(job)).await;
        }
        shares.changed();
    }

    fn sending(&self, _entry_id: &str) -> bool {
        false
    }
}
