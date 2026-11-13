//! The receiving side of a copy (#233): ask for a release, check there's
//! room, pull its files over a relay-free connection and export them for
//! the installer.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use caskade_sync::{EndpointId, Session};

use super::{
    DIAL_WAIT, FRAME_WAIT, Incoming, MAX_LINE, Message, PLAIN_WAIT, PUNCH_WAIT, Transfers,
    folder_name, readable_size, rom, shortfall, sync_error, web,
};
use crate::Error;
use crate::devices::ConnectionPurpose;
use crate::install::{FetchFailure, Fetched, PeerCopyTarget, PeerFetcher, PeerProgress};

impl Transfers {
    /// Makes a way through both routers for `session`'s copy: a port of its
    /// own at each end, and each dialling the other (`caskade_sync::punch`).
    async fn through_routers(
        &self,
        session: &mut Session,
        peer: EndpointId,
    ) -> Option<(
        caskade_sync::copy::DirectDialer,
        caskade_sync::copy::Connection,
    )> {
        let (dialer, addrs) = self.inner.devices.punched_dialer().await?;
        let answer = async {
            session
                .writer()
                .write(&Message::Punch { addrs })
                .await
                .ok()?;
            match tokio::time::timeout(PUNCH_WAIT * 2, session.reader().read(MAX_LINE)).await {
                Ok(Ok(Message::Punch { addrs })) if !addrs.is_empty() => Some(addrs),
                _ => None,
            }
        };
        let connection = match answer.await {
            Some(theirs) => tokio::time::timeout(DIAL_WAIT, dialer.connect(peer, &theirs))
                .await
                .ok()
                .and_then(std::result::Result::ok),
            None => None,
        };
        match connection {
            Some(connection) => Some((dialer, connection)),
            None => {
                dialer.close().await;
                None
            }
        }
    }
}

#[async_trait]
impl PeerFetcher for Transfers {
    async fn fetch(
        &self,
        target: &PeerCopyTarget,
        root: &Path,
        job: &str,
        cancel: caskade_installer::exec::CancellationToken,
        progress: Arc<dyn Fn(PeerProgress) + Send + Sync>,
    ) -> std::result::Result<Fetched, FetchFailure> {
        let device = target.device_name.as_str();
        let peer: EndpointId = target
            .device_id
            .parse()
            .map_err(|_| FetchFailure::Failed(Error::Invalid(crate::tr!("that isn't a device"))))?;
        let unreachable = || crate::tr!("Waiting for {device} to be in reach", device = device);
        let mut session = self
            .inner
            .devices
            .connect(&target.device_id, ConnectionPurpose::Copy)
            .await
            .map_err(|_| FetchFailure::Waiting(unreachable()))?;
        session
            .writer()
            .write(&Message::Request {
                entry: target.entry_id.clone(),
            })
            .await
            .map_err(|_| FetchFailure::Waiting(unreachable()))?;
        let (root_hash, size, title, launch, rom, web) = loop {
            let frame = tokio::time::timeout(FRAME_WAIT, session.reader().read(MAX_LINE)).await;
            match frame {
                Ok(Ok(Message::Preparing { done, total })) => {
                    progress(PeerProgress::Preparing { done, total });
                }
                Ok(Ok(Message::Offer {
                    root,
                    size,
                    title,
                    launch,
                    rom,
                    web,
                    ..
                })) => break (root, size, title, launch, rom, web),
                Ok(Ok(Message::Refused { reason })) => {
                    return Err(FetchFailure::Failed(Error::Invalid(reason)));
                }
                Ok(Ok(_)) => continue,
                _ => return Err(FetchFailure::Waiting(unreachable())),
            }
        };
        let store = self.inner.receiving.clone();
        let have = store.local_bytes(&root_hash).await.unwrap_or(0).min(size);
        let short = shortfall(&self.inner.incoming_dir, root, size, have);
        if short > 0 {
            return Err(FetchFailure::Failed(Error::Invalid(crate::tr!(
                "there isn't enough room for the copy: {short} more is needed",
                short = readable_size(short)
            ))));
        }
        if session.direct_path(PUNCH_WAIT).await.is_none() {
            return Err(FetchFailure::Waiting(crate::tr!(
                "Only a relay reaches {device}: the copy waits for a direct connection",
                device = device
            )));
        }
        let addrs = session.direct_addrs();
        let dialer = self
            .inner
            .devices
            .direct_dialer()
            .await
            .map_err(FetchFailure::Failed)?;
        let plain = tokio::time::timeout(PLAIN_WAIT, dialer.connect(peer, &addrs)).await;
        let (connection, punched) = match plain {
            Ok(Ok(connection)) => (connection, None),
            _ => match self.through_routers(&mut session, peer).await {
                Some((dialer, connection)) => (connection, Some(dialer)),
                None => {
                    return Err(FetchFailure::Waiting(crate::tr!(
                        "{device} can't be reached directly yet: the copy waits for a direct connection",
                        device = device
                    )));
                }
            },
        };
        crate::lock(&self.inner.incoming).insert(
            job.to_owned(),
            Incoming {
                device_id: target.device_id.clone(),
                title: title.clone(),
                done: have,
                total: size,
                started_at_ms: crate::now_ms(),
            },
        );
        self.changed();
        progress(PeerProgress::Receiving {
            done: have,
            total: size,
        });
        let tag = format!("copy/{job}");
        let this = self.clone();
        let key = job.to_owned();
        let report = progress.clone();
        let fetching = store.fetch(connection, &root_hash, &tag, move |bytes| {
            let done = have.saturating_add(bytes).min(size);
            if let Some(incoming) = crate::lock(&this.inner.incoming).get_mut(&key) {
                incoming.done = done;
            }
            report(PeerProgress::Receiving { done, total: size });
        });
        let fetched = tokio::select! {
            _ = cancel.cancelled() => Err(caskade_sync::Error::Copy { reason: "cancelled".into() }),
            fetched = fetching => fetched,
        };
        if let Some(dialer) = punched {
            dialer.close().await;
        }
        let finished = |this: &Self| {
            crate::lock(&this.inner.incoming).remove(job);
            this.changed();
        };
        if let Err(error) = fetched {
            finished(self);
            tracing::debug!(%error, "a copy stopped; what arrived stays");
            return Err(FetchFailure::Waiting(unreachable()));
        }
        let _ = session.writer().write(&Message::Done).await;
        session.end().await;
        let staging = self.inner.incoming_dir.join(job);
        let folder = staging.join(folder_name(&title));
        let _ = std::fs::remove_dir_all(&staging);
        let exported = store.export(&root_hash, &folder).await;
        finished(self);
        exported.map_err(|error| FetchFailure::Failed(sync_error(error)))?;
        let game_id = Some(&target.game_id)
            .filter(|game| !game.is_empty())
            .map(String::as_str);
        // An emulated release: placed as a game of its platform, not
        // installed (#279). A web game's offline copy: made that game's
        // offline copy, not installed (#76); a device on an older Caskade
        // doesn't say so, and its bundle does.
        let web = match (&rom, web) {
            (None, None) => web::in_folder(&folder),
            (_, web) => web,
        };
        let placed = match (&rom, &web) {
            (Some(rom), _) => Some(
                rom::place(
                    &self.inner.store,
                    self.emulator_installer(),
                    rom,
                    &folder,
                    root,
                    &title,
                    game_id,
                )
                .await
                .map_err(FetchFailure::Failed)?,
            ),
            (None, Some(web)) => Some(
                web::place(&self.inner.store, &web.url, &folder, root, &title, game_id)
                    .await
                    .map_err(FetchFailure::Failed)?,
            ),
            (None, None) => None,
        };
        Ok(Fetched {
            folder,
            title,
            launch,
            placed,
        })
    }

    async fn finished(&self, _root: &Path, job: &str) {
        let _ = tokio::fs::remove_dir_all(self.inner.incoming_dir.join(job)).await;
        let _ = self.inner.receiving.release(&format!("copy/{job}")).await;
    }

    fn sending(&self, entry_id: &str) -> bool {
        crate::lock(&self.inner.outgoing)
            .values()
            .any(|outgoing| outgoing.entry == entry_id)
    }
}
