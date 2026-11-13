//! The sharer's side of a `share` session: say what the share is, offer
//! its collection once its files are read, name the recipients that serve
//! it, and follow how far the recipient got.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Weak;
use std::time::Duration;

use async_trait::async_trait;
use caskade_sync::{EndpointId, Session, Sessions};
use tokio::sync::mpsc;

use super::wire::{MAX_LINE, Message};
use super::{Connected, Inner, Prepared, Ready, Shares};
use caskade_core::lock;

/// How long a recipient gets to say hello.
const HELLO_WAIT: Duration = Duration::from_secs(30);
/// How long the sharer waits for serving recipients to let a new one in
/// before naming them to it.
const ALLOW_WAIT: Duration = Duration::from_secs(3);

/// What woke the session's loop.
enum Step {
    Stop,
    Gone,
    Send(Message),
    Frame(Message),
}

/// `share` sessions: a link's grant opened them.
pub(super) struct Serve(pub(super) Weak<Inner>);

#[async_trait]
impl Sessions for Serve {
    async fn opened(&self, session: Session) {
        let Some(inner) = self.0.upgrade() else {
            return session.end().await;
        };
        Shares { inner }.serve(session).await;
    }
}

/// Every address `addrs` and the session know for the peer, once each.
fn merged(mut addrs: Vec<SocketAddr>, session: &Session) -> Vec<SocketAddr> {
    addrs.extend(session.direct_addrs());
    addrs.sort();
    addrs.dedup();
    addrs
}

impl Shares {
    async fn serve(&self, mut session: Session) {
        let peer = session.peer();
        let Some(grant) = session.grant().copied() else {
            return session.end().await;
        };
        let found = lock(&self.inner.running)
            .iter()
            .find(|(_, share)| share.grant == grant)
            .map(|(id, share)| (id.clone(), share.prepared.subscribe(), share.stop.clone()));
        let Some((id, mut prepared, stop)) = found else {
            return session.end().await;
        };
        let Ok(Ok(Message::Hello { name, fetch })) =
            tokio::time::timeout(HELLO_WAIT, session.reader().read(MAX_LINE)).await
        else {
            return session.end().await;
        };
        let name = caskade_sync::device::clean_name(&name);
        let Ok(Some(row)) = self.inner.store.share(&id).await else {
            return session.end().await;
        };
        let size = match &*prepared.borrow() {
            Prepared::Ready(ready) => Some(ready.size),
            _ => u64::try_from(row.size).ok().filter(|size| *size > 0),
        };
        let windows = match row.entry_id.as_deref() {
            Some(entry) => self
                .inner
                .transfers
                .source(entry)
                .await
                .is_ok_and(|(_, _, _, windows)| windows),
            None => false,
        };
        let sharer = self.inner.devices.status().await.this_device.name;
        let about = Message::About {
            kind: row.kind.clone(),
            title: row.title.clone(),
            size,
            windows,
            sharer,
            expires_at_ms: row.expires_at_ms,
            extras: Vec::new(),
            details: serde_json::from_str::<serde_json::Value>(&row.details_json)
                .ok()
                .filter(|details| details.get("kind").is_some()),
        };
        // A collection is only its list: nothing more to send.
        if session.writer().write(&about).await.is_err() || !fetch || row.kind == "collection" {
            return session.end().await;
        }

        let ready = loop {
            let state = prepared.borrow_and_update().clone();
            match state {
                Prepared::Ready(ready) => break ready,
                Prepared::Failed(reason) => {
                    let _ = session.writer().write(&Message::Refused { reason }).await;
                    return session.end().await;
                }
                Prepared::Reading { done, total } => {
                    if session
                        .writer()
                        .write(&Message::Preparing { done, total })
                        .await
                        .is_err()
                    {
                        return session.close();
                    }
                }
            }
            tokio::select! {
                _ = stop.cancelled() => {
                    let _ = session.writer().write(&Message::Stopped).await;
                    return session.end().await;
                }
                changed = prepared.changed() => if changed.is_err() {
                    return session.close();
                },
                // A pause in the reports: say it's still going.
                () = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
        };
        let _ = self
            .inner
            .store
            .add_share_recipient(&id, &peer.to_string(), &name, caskade_core::now_ms())
            .await;
        if self.inner.transfers.allow(peer, &ready.root).is_err() {
            return session.end().await;
        }
        let (out, mut outgoing) = mpsc::unbounded_channel();
        let providers = self.join(&id, peer, out).await;
        self.changed();
        let own = self.own_addrs().await;
        let opening = [
            Message::Offer {
                root: ready.root.clone(),
                size: ready.size,
                files: ready.files,
                extras_root: None,
                launch: ready.launch.clone(),
                addrs: own,
            },
            Message::Providers { list: providers },
        ];
        for message in opening {
            if session.writer().write(&message).await.is_err() {
                self.leave(&id, peer, &ready).await;
                return session.close();
            }
        }

        let mut server = None;
        let stopped = loop {
            let step = tokio::select! {
                _ = stop.cancelled() => Step::Stop,
                Some(message) = outgoing.recv() => Step::Send(message),
                frame = session.reader().read::<Message>(MAX_LINE) => match frame {
                    Ok(message) => Step::Frame(message),
                    Err(_) => Step::Gone,
                },
            };
            let message = match step {
                Step::Stop => {
                    let _ = session.writer().write(&Message::Stopped).await;
                    break true;
                }
                Step::Gone => break false,
                Step::Send(message) => {
                    if session.writer().write(&message).await.is_err() {
                        break false;
                    }
                    continue;
                }
                Step::Frame(message) => message,
            };
            let recipient = peer.to_string();
            match message {
                Message::Progress { done } => {
                    let done = i64::try_from(done.min(ready.size)).unwrap_or(i64::MAX);
                    let _ = self
                        .inner
                        .store
                        .set_share_progress(&id, &recipient, done, None)
                        .await;
                    self.changed();
                }
                Message::Done => {
                    let size = i64::try_from(ready.size).unwrap_or(i64::MAX);
                    let _ = self
                        .inner
                        .store
                        .set_share_progress(&id, &recipient, size, Some(caskade_core::now_ms()))
                        .await;
                    if let Some(bus) = self.inner.bus.get() {
                        bus.emit(caskade_core::bus::Event::ShareReceived {
                            share_id: id.clone(),
                            title: row.title.clone(),
                            recipient: name.clone(),
                            endpoint: recipient.clone(),
                        });
                    }
                    self.changed();
                }
                Message::Serving { addrs } => {
                    let addrs = (!addrs.is_empty()).then(|| merged(addrs, &session));
                    if let Some(share) = lock(&self.inner.running).get_mut(&id)
                        && let Some(connected) = share.connected.get_mut(&peer)
                    {
                        connected.serving = addrs;
                    }
                    self.changed();
                }
                Message::Allowed => self.allowed(&id, peer),
                Message::Punch { addrs } => {
                    // One per session: a recipient may be a stranger with a link.
                    if server.is_none() {
                        server = self.inner.devices.file_server(peer, &addrs).await;
                    }
                    let addrs = server
                        .as_ref()
                        .map(caskade_sync::punch::FileServer::addrs)
                        .unwrap_or_default();
                    let _ = session.writer().write(&Message::Punch { addrs }).await;
                }
                _ => {}
            }
        };
        self.leave(&id, peer, &ready).await;
        if let Some(server) = server {
            server.close().await;
        }
        if stopped {
            session.end().await;
        } else {
            session.close();
        }
    }

    /// Adds `peer` to share `id`'s connected recipients, asks those that
    /// serve to let it in, and returns them once they did (or a little while
    /// passed).
    async fn join(
        &self,
        id: &str,
        peer: EndpointId,
        out: mpsc::UnboundedSender<Message>,
    ) -> Vec<(String, Vec<SocketAddr>)> {
        let (serving, waiter) = {
            let mut running = lock(&self.inner.running);
            let Some(share) = running.get_mut(id) else {
                return Vec::new();
            };
            let serving: Vec<(EndpointId, Vec<SocketAddr>)> = share
                .connected
                .iter()
                .filter(|(other, _)| **other != peer)
                .filter_map(|(other, connected)| Some((*other, connected.serving.clone()?)))
                .collect();
            for (other, _) in &serving {
                if let Some(connected) = share.connected.get(other) {
                    let _ = connected.out.send(Message::Allow {
                        peers: vec![peer.to_string()],
                    });
                }
            }
            share
                .connected
                .insert(peer, Connected { out, serving: None });
            let waiter = share.allowed.subscribe();
            (serving, waiter)
        };
        // Until each one answered, or a little while passed.
        let mut waiter = waiter;
        let mut waiting: HashSet<EndpointId> = serving.iter().map(|(other, _)| *other).collect();
        let deadline = tokio::time::Instant::now() + ALLOW_WAIT;
        while !waiting.is_empty() {
            match tokio::time::timeout_at(deadline, waiter.recv()).await {
                Ok(Ok(from)) => {
                    waiting.remove(&from);
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                _ => break,
            }
        }
        serving
            .into_iter()
            .map(|(other, addrs)| (other.to_string(), addrs))
            .collect()
    }

    /// `from` let in whom it was asked to.
    fn allowed(&self, id: &str, from: EndpointId) {
        if let Some(share) = lock(&self.inner.running).get(id) {
            let _ = share.allowed.send(from);
        }
    }

    /// `peer`'s session with share `id` ended: it can't fetch it any more.
    async fn leave(&self, id: &str, peer: EndpointId, ready: &Ready) {
        if let Some(share) = lock(&self.inner.running).get_mut(id) {
            share.connected.remove(&peer);
        }
        self.inner.transfers.revoke(peer, &ready.root);
        self.changed();
    }
}
