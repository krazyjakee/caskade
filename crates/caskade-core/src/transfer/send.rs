//! The sending side of a copy (#233): a paired device asks for a release,
//! and this one offers its files, or turns it down.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Weak;
use std::time::Duration;

use async_trait::async_trait;
use caskade_sync::{EndpointId, Session, Sessions};

use tokio::sync::mpsc;

use super::{
    Described, Inner, MAX_LINE, Message, OFFER_LIFETIME, Offer, Outgoing, SendRequest, Transfers,
    rom, sync_error, web,
};
use crate::model::{GameRuntime, InstallLocation, InstallState, LaunchSpec};
use crate::{Error, Result};

/// `copy` sessions from paired devices: they ask for a release.
pub(super) struct Serve(pub(super) Weak<Inner>);

#[async_trait]
impl Sessions for Serve {
    async fn opened(&self, session: Session) {
        let Some(inner) = self.0.upgrade() else {
            return session.end().await;
        };
        Transfers { inner }.serve(session).await;
    }
}

impl Transfers {
    async fn serve(&self, mut session: Session) {
        let peer = session.peer();
        let entry = match session.reader().read(MAX_LINE).await {
            Ok(Message::Request { entry }) => entry,
            Ok(Message::Send {
                entry,
                game,
                title,
                size,
            }) => return self.requested(session, entry, game, title, size).await,
            _ => return session.end().await,
        };
        match self.offer(&entry, peer, &mut session).await {
            Ok(()) => {
                let mut server = None;
                // Until the device says it has everything, or goes.
                loop {
                    match tokio::time::timeout(OFFER_LIFETIME, session.reader().read(MAX_LINE))
                        .await
                    {
                        Ok(Ok(Message::Done)) => {
                            self.withdraw(&entry, peer).await;
                            break;
                        }
                        Ok(Ok(Message::Punch { addrs })) => {
                            if server.is_none() {
                                server = self.inner.devices.file_server(peer, &addrs).await;
                            }
                            let addrs = server
                                .as_ref()
                                .map(caskade_sync::punch::FileServer::addrs)
                                .unwrap_or_default();
                            let _ = session.writer().write(&Message::Punch { addrs }).await;
                        }
                        Ok(Ok(_)) => continue,
                        // Gone: the offer stays a while, to resume.
                        _ => break,
                    }
                }
                session.end().await;
                if let Some(server) = server {
                    server.close().await;
                }
            }
            Err(reason) => {
                let _ = session
                    .writer()
                    .write(&Message::Refused {
                        reason: reason.to_string(),
                    })
                    .await;
                session.end().await;
            }
        }
    }

    /// Another device asks this one to copy a game from it: queued for
    /// confirmation here, or started (the app decides, `on_send`).
    async fn requested(
        &self,
        mut session: Session,
        entry: String,
        game: String,
        title: String,
        size: Option<u64>,
    ) {
        let peer = session.peer().to_string();
        let answer = match self.inner.on_send.get() {
            Some(handler) => {
                let game_id = self
                    .inner
                    .store
                    .sync_mapped_game(&peer, &game)
                    .await
                    .ok()
                    .flatten();
                handler(SendRequest {
                    device_id: peer,
                    entry_id: entry,
                    game_id,
                    title,
                    size,
                });
                Message::Queued
            }
            None => Message::Refused {
                reason: crate::tr!("this device doesn't take copies"),
            },
        };
        let _ = session.writer().write(&answer).await;
        session.end().await;
    }

    /// The folder release `entry` is in, its program relative to it, and
    /// whether it's a Windows build; refused when it can't be copied.
    pub async fn source(&self, entry: &str) -> Result<(PathBuf, String, Option<String>, bool)> {
        let not_here = || Error::Invalid(crate::tr!("that game isn't installed on this device"));
        let entry = self
            .inner
            .store
            .get_entry(entry)
            .await?
            .ok_or_else(not_here)?;
        let source = entry.source_id.as_str();
        if source.starts_with("peer:")
            || source.starts_with("store:")
            || source.starts_with("stream:")
        {
            return Err(not_here());
        }
        if !matches!(entry.state, InstallState::Installed) {
            return Err(Error::Invalid(crate::tr!(
                "the game isn't fully installed on this device"
            )));
        }
        // Its ROM files go instead (`rom`): its folder may hold every game
        // of the platform.
        if rom::emulated(&entry) {
            return Err(Error::Invalid(crate::tr!(
                "an emulated game goes as its ROM files, not its folder"
            )));
        }
        // A web game goes as its offline copy, when it has one (`web`).
        let web = web::is_web(&entry)
            .then(|| web::describe(&entry))
            .transpose()?;
        let folder = match &entry.location {
            Some(InstallLocation::Managed { root_id, rel_dir }) => {
                let root = self.inner.store.get_root(root_id).await?.ok_or_else(|| {
                    Error::Invalid(crate::tr!("the game's storage isn't available"))
                })?;
                root.path.join(rel_dir)
            }
            Some(InstallLocation::External { path })
                if matches!(entry.launch, Some(LaunchSpec::Executable { .. })) =>
            {
                if path.is_dir() {
                    path.clone()
                } else {
                    path.parent().map(Path::to_path_buf).ok_or_else(not_here)?
                }
            }
            Some(InstallLocation::External { .. }) => {
                return Err(Error::Invalid(crate::tr!(
                    "it starts through its store's launcher here; install it from the store there"
                )));
            }
            None => return Err(not_here()),
        };
        if !folder.is_dir() {
            return Err(Error::Invalid(crate::tr!("the game's folder isn't there")));
        }
        if web.is_some() && !folder.join(crate::web::bundle::MANIFEST).is_file() {
            return Err(Error::Invalid(crate::tr!(
                "the offline copy's files aren't there"
            )));
        }
        let launch = match &entry.launch {
            Some(LaunchSpec::Executable { path, .. }) => {
                let relative = if path.is_absolute() {
                    path.strip_prefix(&folder).ok().map(Path::to_path_buf)
                } else {
                    Some(path.clone())
                };
                relative.and_then(|path| {
                    let parts: Vec<String> = path
                        .components()
                        .filter_map(|part| match part {
                            std::path::Component::Normal(part) => part.to_str().map(str::to_owned),
                            _ => None,
                        })
                        .collect();
                    (!parts.is_empty()).then(|| parts.join("/"))
                })
            }
            _ => None,
        };
        let title = self
            .inner
            .store
            .game(&entry.game_id)
            .await?
            .map(|game| game.title)
            .unwrap_or_default();
        let windows = matches!(entry.runtime, GameRuntime::Windows { .. });
        Ok((folder, title, launch, windows))
    }

    /// Offers release `entry` to `peer` on `session`: reads its files (or
    /// takes an earlier offer of it), lets `peer` fetch them, and says so.
    async fn offer(&self, entry: &str, peer: EndpointId, session: &mut Session) -> Result<()> {
        let earlier = {
            let offers = crate::lock(&self.inner.offers);
            offers
                .get(entry)
                .map(|offer| (offer.offered.clone(), offer.what.clone()))
        };
        let (offered, what) = match earlier {
            Some(earlier) => earlier,
            None => {
                let (folder, what) = self.what_to_send(entry).await?;
                let tag = format!("copy/{entry}");
                let (tx, mut rx) = mpsc::unbounded_channel();
                let reading = self.inner.sending.offer(&folder, &tag, move |done, total| {
                    let _ = tx.send((done, total));
                });
                tokio::pin!(reading);
                let mut last = std::time::Instant::now();
                let offered = loop {
                    tokio::select! {
                        offered = &mut reading => break offered.map_err(sync_error)?,
                        Some((done, total)) = rx.recv() => {
                            if last.elapsed() >= Duration::from_millis(500) {
                                last = std::time::Instant::now();
                                let _ = session
                                    .writer()
                                    .write(&Message::Preparing { done, total })
                                    .await;
                            }
                        }
                    }
                };
                crate::lock(&self.inner.offers).insert(
                    entry.to_owned(),
                    Offer {
                        offered: offered.clone(),
                        tag: tag.clone(),
                        what: what.clone(),
                        peers: HashSet::new(),
                    },
                );
                (offered, what)
            }
        };
        self.inner
            .gate
            .allow(peer, &offered.root)
            .map_err(sync_error)?;
        if let Some(offer) = crate::lock(&self.inner.offers).get_mut(entry) {
            offer.peers.insert(peer);
        }
        crate::lock(&self.inner.outgoing)
            .entry((peer, entry.to_owned()))
            .or_insert_with(|| Outgoing {
                entry: entry.to_owned(),
                title: what.title.clone(),
                root: offered.root.clone(),
                size: offered.size,
                started_at_ms: crate::now_ms(),
            });
        self.changed();
        // Offered for a while, then withdrawn whatever happened.
        let this = self.clone();
        let (key, until) = (entry.to_owned(), OFFER_LIFETIME);
        tokio::spawn(async move {
            tokio::time::sleep(until).await;
            this.withdraw(&key, peer).await;
        });
        session
            .writer()
            .write(&Message::Offer {
                root: offered.root,
                size: offered.size,
                files: offered.files,
                title: what.title,
                launch: what.launch,
                windows: what.windows,
                rom: what.rom,
                web: what.web,
            })
            .await
            .map_err(sync_error)
    }

    /// What release `entry` sends: its folder, title, program and whether
    /// it's a Windows build, and for a web game its page; or, for an
    /// emulated release, its ROM files put together on their own and what
    /// they are.
    async fn what_to_send(&self, entry: &str) -> Result<(PathBuf, Described)> {
        let release = self.inner.store.get_entry(entry).await?;
        if !release.as_ref().is_some_and(rom::emulated) {
            let (folder, title, launch, windows) = self.source(entry).await?;
            let web = release
                .as_ref()
                .filter(|release| web::is_web(release))
                .map(web::describe)
                .transpose()?;
            return Ok((
                folder,
                Described {
                    title,
                    launch,
                    windows,
                    rom: None,
                    web,
                },
            ));
        }
        let (release, copy, files) = rom::describe(&self.inner.store, entry).await?;
        if !matches!(release.state, InstallState::Installed) {
            return Err(Error::Invalid(crate::tr!(
                "the game isn't fully installed on this device"
            )));
        }
        let folder = rom::put_together(&self.inner.outgoing_dir.join(entry), files).await?;
        let title = self
            .inner
            .store
            .game(&release.game_id)
            .await?
            .map(|game| game.title)
            .unwrap_or_default();
        Ok((
            folder,
            Described {
                title,
                launch: None,
                windows: false,
                rom: Some(copy),
                web: None,
            },
        ))
    }

    /// `peer` has release `entry`, or its offer ran out: withdrawn, and let
    /// go once nobody else is fetching it.
    async fn withdraw(&self, entry: &str, peer: EndpointId) {
        let released = {
            let mut offers = crate::lock(&self.inner.offers);
            let Some(offer) = offers.get_mut(entry) else {
                return;
            };
            self.inner.gate.revoke(peer, &offer.offered.root);
            offer.peers.remove(&peer);
            if offer.peers.is_empty() {
                offers.remove(entry).map(|offer| offer.tag)
            } else {
                None
            }
        };
        crate::lock(&self.inner.outgoing).remove(&(peer, entry.to_owned()));
        if let Some(tag) = released {
            let _ = self.inner.sending.release(&tag).await;
            let _ = tokio::fs::remove_dir_all(self.inner.outgoing_dir.join(entry)).await;
        }
        self.changed();
    }
}
