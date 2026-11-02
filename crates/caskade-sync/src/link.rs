//! How each device is reached right now: its link state.
//!
//! A connection holds several paths at once (a relay path and direct ones)
//! and one of them is selected to carry data. The link state comes from the
//! selected path: [`Link::Relay`] through a relay, [`Link::Lan`] for an
//! address on the local network, [`Link::Direct`] for any other IP address.
//! A device with no open connection is [`Link::Offline`].
//!
//! An address is on the local network when it's private, link-local or
//! loopback, when the device announced it on this network over mDNS, or when
//! it's an IPv6 address in the same /64 as one of this device's own (a
//! household's global IPv6 addresses share the router's prefix).
//!
//! [`Links`] follows every session's connection through its path events and
//! sends a [`LinkChanged`] whenever a device's best link changes, including
//! a relay path giving way to a direct one once holepunching succeeds.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt as _;
use iroh::endpoint::{Connection, PathEvent};
use iroh::{Endpoint, EndpointId, TransportAddr};
use serde::Serialize;
use tokio::sync::broadcast;

/// How a device is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Link {
    /// No open connection.
    Offline,
    /// Through a relay.
    Relay,
    /// Directly, over the internet.
    Direct,
    /// Directly, on the local network.
    Lan,
}

/// A device's link state changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkChanged {
    pub peer: EndpointId,
    pub link: Link,
}

/// The link a path to `addr` gives, from its address alone (see
/// [`Links::classify`] for the local network as this device sees it).
pub fn classify(addr: &TransportAddr) -> Link {
    match addr {
        TransportAddr::Relay(_) => Link::Relay,
        TransportAddr::Ip(addr) if is_local(addr.ip()) => Link::Lan,
        _ => Link::Direct,
    }
}

/// Whether two IPv6 addresses share a /64, the prefix a link's addresses
/// share.
fn same_v6_link(a: IpAddr, b: IpAddr) -> bool {
    match (a, b) {
        (IpAddr::V6(a), IpAddr::V6(b)) if a.to_ipv4_mapped().is_none() => {
            a.octets()[..8] == b.octets()[..8]
        }
        _ => false,
    }
}

/// Private, link-local, loopback and unique-local addresses: the local
/// network.
pub fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_private() || ip.is_link_local() || ip.is_loopback(),
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_local(IpAddr::V4(v4));
            }
            ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
        }
    }
}

/// Every open connection's link, per device, and the events that follow
/// them.
#[derive(Debug, Clone)]
pub struct Links {
    inner: Arc<Mutex<HashMap<EndpointId, HashMap<usize, Link>>>>,
    events: broadcast::Sender<LinkChanged>,
    /// Addresses devices announced on this network over mDNS.
    announced: Arc<Mutex<HashSet<IpAddr>>>,
    /// Each device's last link while it had an open connection.
    last: Arc<Mutex<HashMap<EndpointId, Link>>>,
    /// This device's endpoint, for its own addresses.
    own: Option<Endpoint>,
}

impl Default for Links {
    fn default() -> Self {
        Self {
            inner: Arc::default(),
            events: broadcast::channel(64).0,
            announced: Arc::default(),
            last: Arc::default(),
            own: None,
        }
    }
}

impl Links {
    pub(crate) fn new(endpoint: &Endpoint) -> Self {
        Self {
            own: Some(endpoint.clone()),
            ..Self::default()
        }
    }

    /// Records addresses a device announced on this network.
    pub(crate) fn announced(&self, addrs: impl IntoIterator<Item = IpAddr>) {
        let mut announced = self.announced.lock().unwrap_or_else(|e| e.into_inner());
        announced.extend(addrs);
    }

    /// The link a path to `addr` gives, as this device sees its network.
    pub fn classify(&self, addr: &TransportAddr) -> Link {
        let TransportAddr::Ip(socket) = addr else {
            return classify(addr);
        };
        let ip = socket.ip();
        let local = is_local(ip)
            || self
                .announced
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&ip)
            || self.own.as_ref().is_some_and(|endpoint| {
                endpoint
                    .addr()
                    .ip_addrs()
                    .any(|own| same_v6_link(own.ip(), ip))
            });
        if local { Link::Lan } else { Link::Direct }
    }

    /// The link `connection`'s selected path gives, or [`Link::Offline`] when
    /// it has none (it's closed, or nothing is selected yet).
    pub fn of(&self, connection: &Connection) -> Link {
        if connection.close_reason().is_some() {
            return Link::Offline;
        }
        connection
            .paths()
            .iter()
            .find(|path| path.is_selected())
            .map_or(Link::Offline, |path| self.classify(path.remote_addr()))
    }

    /// The best link to `peer` over all its open connections.
    pub fn get(&self, peer: EndpointId) -> Link {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        best(inner.get(&peer))
    }

    /// The best link `peer` had the last time it had an open connection,
    /// whether or not one is open now.
    pub fn last(&self, peer: EndpointId) -> Option<Link> {
        let last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        last.get(&peer).copied()
    }

    /// Every device with an open connection and its best link.
    pub fn all(&self) -> Vec<(EndpointId, Link)> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .iter()
            .map(|(peer, connections)| (*peer, best(Some(connections))))
            .filter(|(_, link)| *link != Link::Offline)
            .collect()
    }

    /// Changes as they happen. A receiver that falls behind gets
    /// `RecvError::Lagged` and can read [`Links::all`] again.
    pub fn subscribe(&self) -> broadcast::Receiver<LinkChanged> {
        self.events.subscribe()
    }

    /// Follows `connection` until it closes, on the current runtime.
    pub(crate) fn track(&self, connection: &Connection) {
        let links = self.clone();
        let connection = connection.clone();
        let peer = connection.remote_id();
        let id = connection.stable_id();
        let mut events = connection.path_events();
        self.set(peer, id, Some(self.of(&connection)));
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    event = events.next() => match event {
                        Some(PathEvent::Lagged { .. }) | Some(_) => {
                            let link = links.of(&connection);
                            links.set(peer, id, Some(link));
                        }
                        None => break,
                    },
                    _ = connection.closed() => break,
                }
            }
            links.set(peer, id, None);
        });
    }

    fn set(&self, peer: EndpointId, connection: usize, link: Option<Link>) {
        let (before, after) = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let before = best(inner.get(&peer));
            let connections = inner.entry(peer).or_default();
            match link {
                Some(link) => {
                    connections.insert(connection, link);
                }
                None => {
                    connections.remove(&connection);
                }
            }
            let after = best(Some(connections));
            if connections.is_empty() {
                inner.remove(&peer);
            }
            (before, after)
        };
        if after != Link::Offline {
            let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
            last.insert(peer, after);
        }
        if before != after {
            let _ = self.events.send(LinkChanged { peer, link: after });
        }
    }
}

fn best(connections: Option<&HashMap<usize, Link>>) -> Link {
    connections
        .and_then(|connections| connections.values().max().copied())
        .unwrap_or(Link::Offline)
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;

    fn ip(text: &str) -> TransportAddr {
        TransportAddr::Ip(text.parse::<SocketAddr>().unwrap())
    }

    #[test]
    fn paths_classify_by_address() {
        assert_eq!(classify(&ip("192.168.1.20:4000")), Link::Lan);
        assert_eq!(classify(&ip("10.0.0.2:4000")), Link::Lan);
        assert_eq!(classify(&ip("172.16.5.1:4000")), Link::Lan);
        assert_eq!(classify(&ip("169.254.10.1:4000")), Link::Lan);
        assert_eq!(classify(&ip("127.0.0.1:4000")), Link::Lan);
        assert_eq!(classify(&ip("[fe80::1]:4000")), Link::Lan);
        assert_eq!(classify(&ip("[fd12:3456::1]:4000")), Link::Lan);
        assert_eq!(classify(&ip("[::ffff:192.168.0.3]:4000")), Link::Lan);
        assert_eq!(classify(&ip("81.2.69.160:4000")), Link::Direct);
        assert_eq!(classify(&ip("[2a00:1450::1]:4000")), Link::Direct);
        let relay = TransportAddr::Relay("https://relay.example./".parse().unwrap());
        assert_eq!(classify(&relay), Link::Relay);
    }

    #[test]
    fn announced_and_same_prefix_addresses_are_the_local_network() {
        let links = Links::default();
        assert_eq!(
            links.classify(&ip("[2a02:8012:f580::1]:4000")),
            Link::Direct
        );
        links.announced(["2a02:8012:f580::1".parse().unwrap()]);
        assert_eq!(links.classify(&ip("[2a02:8012:f580::1]:4000")), Link::Lan);
        assert!(same_v6_link(
            "2a02:8012:f580:0:1::1".parse().unwrap(),
            "2a02:8012:f580:0:2::2".parse().unwrap()
        ));
        assert!(!same_v6_link(
            "2a02:8012:f580:1::1".parse().unwrap(),
            "2a02:8012:f580:2::1".parse().unwrap()
        ));
        assert!(!same_v6_link(
            "::ffff:81.2.69.160".parse().unwrap(),
            "::ffff:81.2.69.161".parse().unwrap()
        ));
    }

    #[test]
    fn a_device_s_link_is_its_best_connection() {
        let links = Links::default();
        let peer = iroh::SecretKey::generate().public();
        let mut events = links.subscribe();
        links.set(peer, 1, Some(Link::Relay));
        links.set(peer, 2, Some(Link::Lan));
        assert_eq!(links.get(peer), Link::Lan);
        links.set(peer, 2, None);
        assert_eq!(links.get(peer), Link::Relay);
        links.set(peer, 1, None);
        assert_eq!(links.get(peer), Link::Offline);
        let seen: Vec<Link> = std::iter::from_fn(|| events.try_recv().ok())
            .map(|event| event.link)
            .collect();
        assert_eq!(seen, [Link::Relay, Link::Lan, Link::Relay, Link::Offline]);
        assert!(links.all().is_empty());
        // The last link it had open stays known once it closed.
        assert_eq!(links.last(peer), Some(Link::Relay));
        assert_eq!(links.last(iroh::SecretKey::generate().public()), None);
    }
}
