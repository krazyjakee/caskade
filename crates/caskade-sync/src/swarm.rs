//! One shared game from several providers at once (#249): the sharer and
//! the share's other recipients that already hold some of it.
//!
//! The fetch splits the collection into one request per file and asks the
//! providers in a shuffled order for each, so files come from several at
//! once. Every chunk is checked against the BLAKE3 tree as it arrives,
//! whoever sent it. What a provider may serve is its own [`crate::copy::CopyGate`]'s:
//! a recipient serves only the share's collection, only to the share's other
//! recipients the sharer named, and only while the share runs.
//!
//! Like copies, nothing here passes through a relay: [`SwarmDialer`] is an
//! endpoint with no relay transport and no address lookup but the direct
//! addresses it is given.

use std::net::SocketAddr;
use std::str::FromStr;

use futures_util::StreamExt as _;
use iroh::address_lookup::MemoryLookup;
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, TransportAddr, endpoint::presets};
use iroh_blobs::api::downloader::{DownloadProgressItem, DownloadRequest, Shuffled, SplitStrategy};
use iroh_blobs::{Hash, HashAndFormat};

use crate::copy::CopyStore;
use crate::error::{Error, Result};
use crate::identity::Identity;

fn swarm_error(error: impl std::fmt::Display) -> Error {
    Error::Copy {
        reason: error.to_string(),
    }
}

/// A provider of a collection: its id and its direct addresses.
pub type Provider = (EndpointId, Vec<SocketAddr>);

/// An endpoint with this device's key, no relay transport, and only the
/// direct addresses it was told: fetches from several providers over it.
///
/// Bound like [`crate::copy::DirectDialer`] (no relay, no lookup but its
/// own), with a [`MemoryLookup`] for several providers' addresses: keep the
/// two in step on "never a relay".
#[derive(Debug, Clone)]
pub struct SwarmDialer {
    endpoint: Endpoint,
    lookup: MemoryLookup,
}

impl SwarmDialer {
    /// Binds one for `identity` (a throwaway key when `None`, for tests), on
    /// loopback only when `loopback`.
    pub async fn bind(identity: Option<&Identity>, loopback: bool) -> Result<Self> {
        let lookup = MemoryLookup::new();
        let mut builder = Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .clear_address_lookup()
            .address_lookup(lookup.clone());
        if loopback {
            builder = builder
                .clear_ip_transports()
                .bind_addr(std::net::SocketAddrV4::new(
                    std::net::Ipv4Addr::LOCALHOST,
                    0,
                ))
                .expect("127.0.0.1 is a valid bind address");
        }
        if let Some(identity) = identity {
            builder = builder.secret_key(identity.secret_key().clone());
        }
        let endpoint = builder
            .bind()
            .await
            .map_err(|source| Error::Bind { source })?;
        Ok(Self { endpoint, lookup })
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Remembers where `providers` are: their direct addresses only.
    fn know(&self, providers: &[Provider]) {
        for (id, addrs) in providers {
            if addrs.is_empty() {
                continue;
            }
            self.lookup.add_endpoint_info(EndpointAddr::from_parts(
                *id,
                addrs.iter().map(|addr| TransportAddr::Ip(*addr)),
            ));
        }
    }

    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

/// Fetches what's missing of the collection `root` into `store` from
/// `providers`, several at once, keeping it under `tag`. `progress` gets
/// the bytes received in this fetch. Returns once everything is here and
/// verified; a provider that refuses or goes is passed over for the others.
pub async fn fetch_from(
    store: &CopyStore,
    dialer: &SwarmDialer,
    providers: &[Provider],
    root: &str,
    tag: &str,
    mut progress: impl FnMut(u64) + Send,
) -> Result<()> {
    let root = Hash::from_str(root).map_err(|_| swarm_error("that isn't a collection"))?;
    let reachable: Vec<EndpointId> = providers
        .iter()
        .filter(|(_, addrs)| !addrs.is_empty())
        .map(|(id, _)| *id)
        .collect();
    if reachable.is_empty() {
        return Err(swarm_error("no direct address for anyone who has it"));
    }
    dialer.know(providers);
    let content = HashAndFormat::hash_seq(root);
    store.settle(content).await.map_err(swarm_error)?;
    let blobs = store.blobs();
    // Kept from the start, so a stopped fetch keeps what arrived.
    blobs
        .tags()
        .set(tag, content)
        .await
        .map_err(swarm_error)?;
    let downloader = blobs.downloader(&dialer.endpoint);
    let request = DownloadRequest::new(content, Shuffled::new(reachable), SplitStrategy::Split);
    let mut items = downloader
        .download_with_opts(request)
        .stream()
        .await
        .map_err(swarm_error)?;
    while let Some(item) = items.next().await {
        match item {
            DownloadProgressItem::Progress(bytes) => progress(bytes),
            DownloadProgressItem::Error(error) => return Err(swarm_error(error)),
            // One provider failing is fine while the others have it: what
            // counts is whether everything is here.
            _ => {}
        }
    }
    let have = blobs
        .remote()
        .local(content)
        .await
        .map_err(swarm_error)?;
    if !have.is_complete() {
        return Err(swarm_error("the transfer ended early"));
    }
    Ok(())
}
