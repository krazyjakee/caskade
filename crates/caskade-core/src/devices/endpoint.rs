//! The sync endpoint (#226): this device's identity, binding the endpoint
//! with the settings' reach, and the relay-free dialers game files travel
//! by (#233).

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use caskade_sync::punch::OpenPort;
use caskade_sync::{
    Config, EndpointAddr, EndpointId, Identity, Loaded, Lost, Reach, SyncEndpoint, TransportAddr,
};

use super::{Devices, Handler, IdentityStatus, Inner};
use crate::{Error, Result};

impl Devices {
    /// This device's address for a link: every local address, plus the
    /// relay once there is one, waiting at most `wait` (#246).
    pub async fn link_addr(&self, wait: Duration) -> Result<EndpointAddr> {
        Ok(self.inner.endpoint()?.shareable_addr(wait).await)
    }

    /// An endpoint with this device's key and no relay transport, for
    /// pulling game files (#233): none can pass through a relay.
    pub async fn direct_dialer(&self) -> Result<caskade_sync::copy::DirectDialer> {
        let key = self.inner.lock().key.clone();
        let Some(key) = key else {
            return Err(Error::Invalid(crate::tr!(
                "this device has no identity to connect with"
            )));
        };
        let mut dialer = self.inner.dialer.lock().await;
        if let Some((id, dialer)) = dialer.as_ref()
            && *id == key.id()
        {
            return Ok(dialer.clone());
        }
        let bound = caskade_sync::copy::DirectDialer::bind(Some(&key), self.inner.on_loopback())
            .await
            .map_err(|error| Error::Invalid(error.to_string()))?;
        *dialer = Some((key.id(), bound.clone()));
        Ok(bound)
    }

    /// [`Devices::direct_dialer`] for fetching from several providers at
    /// once (shares, #249): this device's key, no relay transport, only the
    /// direct addresses it's given.
    pub async fn swarm_dialer(&self) -> Result<caskade_sync::swarm::SwarmDialer> {
        let key = self.inner.lock().key.clone();
        let Some(key) = key else {
            return Err(Error::Invalid(crate::tr!(
                "this device has no identity to connect with"
            )));
        };
        let mut swarm = self.inner.swarm.lock().await;
        if let Some((id, dialer)) = swarm.as_ref()
            && *id == key.id()
        {
            return Ok(dialer.clone());
        }
        let bound = caskade_sync::swarm::SwarmDialer::bind(Some(&key), self.inner.on_loopback())
            .await
            .map_err(|error| Error::Invalid(error.to_string()))?;
        *swarm = Some((key.id(), bound.clone()));
        Ok(bound)
    }

    /// A port of its own for one transfer through two routers
    /// (`caskade_sync::punch`), with a relay's word on its public address.
    /// `None` with no relay to ask (Connect only directly), or none that
    /// answers.
    async fn open_port(&self) -> Option<(Identity, OpenPort)> {
        let (key, relays) = {
            let state = self.inner.lock();
            (state.key.clone()?, state.relays.clone()?)
        };
        Some((key, caskade_sync::punch::open_port(&relays).await?))
    }

    /// [`Devices::direct_dialer`] on a port of its own, and the addresses to
    /// tell the other device: for a device no plain dial reaches. Close it
    /// after the transfer.
    pub async fn punched_dialer(
        &self,
    ) -> Option<(caskade_sync::copy::DirectDialer, Vec<SocketAddr>)> {
        let (key, open) = self.open_port().await?;
        caskade_sync::copy::DirectDialer::bind_on(&key, &open)
            .await
            .ok()
    }

    /// [`Devices::swarm_dialer`] on a port of its own: see
    /// [`Devices::punched_dialer`].
    pub async fn punched_swarm(
        &self,
    ) -> Option<(caskade_sync::swarm::SwarmDialer, Vec<SocketAddr>)> {
        let (key, open) = self.open_port().await?;
        caskade_sync::swarm::SwarmDialer::bind_on(&key, &open)
            .await
            .ok()
    }

    /// What this device serves, on a port of its own for `peer`, who dials
    /// from `theirs`. Close it after the transfer.
    pub async fn file_server(
        &self,
        peer: EndpointId,
        theirs: &[SocketAddr],
    ) -> Option<caskade_sync::punch::FileServer> {
        let blobs = self.inner.copy_provider.get()?.clone();
        let (key, open) = self.open_port().await?;
        caskade_sync::punch::FileServer::bind(&key, &open, blobs, peer, theirs)
            .await
            .ok()
    }
}

impl Inner {
    pub(super) async fn load_identity(self: &Arc<Self>) {
        let record = self.record().await;
        let known = record.id.as_deref().and_then(|id| id.parse().ok());
        let status = match Identity::load(self.keys.clone(), known).await {
            Loaded::Ready { identity, .. } => {
                self.identity_ready(identity).await;
                return;
            }
            Loaded::NeedsConfirmation {
                reason: Lost::Missing,
            } => IdentityStatus::Missing,
            Loaded::NeedsConfirmation {
                reason: Lost::Unreadable,
            } => IdentityStatus::Unreadable,
            Loaded::Unavailable => IdentityStatus::KeychainUnavailable,
        };
        tracing::warn!(?status, "sync waits for the device key");
        self.lock().identity = status;
        self.changed();
    }

    pub(super) async fn identity_ready(self: &Arc<Self>, identity: Identity) {
        let mut record = self.record().await;
        let id = identity.id().to_string();
        if record.id.as_deref() != Some(&id) {
            record.id = Some(id);
            if let Err(error) = self.save_record(&record).await {
                tracing::warn!(%error, "this device's id not saved");
            }
        }
        {
            let mut state = self.lock();
            state.identity = IdentityStatus::Ready;
            state.key = Some(identity);
        }
        self.bind().await;
    }

    /// (Re)binds the endpoint with the current settings.
    pub(super) async fn bind(self: &Arc<Self>) {
        let _binding = self.binding.lock().await;
        let (old, key) = {
            let mut state = self.lock();
            state.forwarders.clear();
            (state.endpoint.take(), state.key.clone())
        };
        if let Some(old) = old {
            let _ = old.shutdown().await;
        }
        let Some(identity) = key else {
            self.changed();
            return;
        };
        let settings = self.settings().await;
        self.lock().bound_with = Some((settings.never_relay, settings.port));
        let reach = (self.reach)(Reach::network(settings.never_relay, settings.port));
        self.loopback.store(
            matches!(reach.ip, caskade_sync::Ip::Loopback { .. }),
            Ordering::Relaxed,
        );
        self.lock().relays = Some(reach.relays.clone());
        let config = Config {
            identity: Some(identity),
            reach,
            device: self.me().await,
            blobs: self.copy_provider.get().cloned(),
            ..Config::default()
        };
        let handler = Arc::new(Handler(Arc::downgrade(self)));
        // The endpoint just closed may hold a fixed port a moment longer.
        let mut tries = 0;
        let bound = loop {
            let bound = SyncEndpoint::bind(config.clone(), self.gate.clone(), handler.clone());
            match bound.await {
                Err(_) if tries < BIND_TRIES => tries += 1,
                bound => break bound,
            }
            tokio::time::sleep(BIND_RETRY).await;
        };
        match bound {
            Ok(endpoint) => {
                let forwarders = self.forward(&endpoint);
                let mut state = self.lock();
                state.endpoint = Some(endpoint);
                state.forwarders = forwarders;
            }
            Err(error) => tracing::warn!(%error, "sync endpoint not started"),
        }
        self.changed();
        let this = self.clone();
        tokio::spawn(async move { this.tell_forgotten().await });
    }

    /// Link and nearby changes become revisions.
    fn forward(self: &Arc<Self>, endpoint: &SyncEndpoint) -> Vec<Forwarder> {
        let mut forwarders = Vec::new();
        let mut links = endpoint.links().subscribe();
        let weak = Arc::downgrade(self);
        forwarders.push(Forwarder(tokio::spawn(async move {
            while let Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
                links.recv().await
            {
                let Some(inner) = weak.upgrade() else { break };
                inner.changed();
            }
        })));
        // Relay connections coming and going: the internet came or went.
        let mut relays = endpoint.internet_changes();
        let weak = Arc::downgrade(self);
        forwarders.push(Forwarder(tokio::spawn(async move {
            while relays.next().await {
                let Some(inner) = weak.upgrade() else { break };
                inner.changed();
            }
        })));
        if let Some(lan) = endpoint.lan() {
            let mut nearby = lan.subscribe();
            let weak = Arc::downgrade(self);
            forwarders.push(Forwarder(tokio::spawn(async move {
                loop {
                    let found = match nearby.recv().await {
                        Ok(caskade_sync::NearbyChanged::Found(found)) => Some(found.id),
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => None,
                        Err(_) => break,
                    };
                    let Some(inner) = weak.upgrade() else { break };
                    if let Some(found) = found {
                        let _ = inner.reachable.send(found.to_string());
                    }
                    inner.changed();
                    inner.tell_forgotten().await;
                }
            })));
        }
        forwarders
    }
}

/// How often an endpoint that won't bind is tried again, and how long apart.
const BIND_TRIES: u32 = 20;

const BIND_RETRY: Duration = Duration::from_millis(250);

pub(super) struct Forwarder(tokio::task::JoinHandle<()>);

impl Drop for Forwarder {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) fn addr_text(addr: &TransportAddr) -> String {
    match addr {
        TransportAddr::Ip(addr) => addr.to_string(),
        TransportAddr::Relay(url) => url.to_string(),
        _ => String::new(),
    }
}

pub(super) fn parse_addr(text: &str) -> Option<TransportAddr> {
    if let Ok(addr) = text.parse::<SocketAddr>() {
        return Some(TransportAddr::Ip(addr));
    }
    text.parse().ok().map(TransportAddr::Relay)
}
