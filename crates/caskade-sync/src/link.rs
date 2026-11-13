//! How each device is reached right now: its link state.
//!
//! A connection holds several paths at once (a relay path and direct ones)
//! and one of them is selected to carry data. The link state comes from the
//! selected path: [`Link::Relay`] through a relay, [`Link::Lan`] for an
//! address on the local network, [`Link::Direct`] for any other IP address.
//! A device with no open connection is [`Link::Offline`].
//!
//! A selected relay path also has to be heard from. iroh falls back to the
//! relay path when a direct one times out, however long that path has been
//! silent, and keeps it selected until the connection times out too: after a
//! cable is pulled that's a link in name only, so it's [`Link::Offline`]
//! until something comes back over it.
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
use std::time::{Duration, Instant};

use futures_util::StreamExt as _;
use iroh::endpoint::{Connection, PathEvent};
use iroh::{Endpoint, EndpointId, TransportAddr};
use tokio::sync::broadcast;

/// How long a relay path may hear nothing before it stops counting. Each end
/// sends a keep-alive on every open path about every 5 s, and iroh's own limit
/// on a direct path is 15 s: a relay path that went at the same moment as the
/// direct one has already run out when iroh falls back to it.
const RELAY_SILENCE: Duration = Duration::from_secs(12);
/// How often a connection's relay paths are looked at.
const RELAY_CHECK: Duration = Duration::from_secs(2);

/// How a device is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Link {
    /// No open connection.
    Offline,
    /// Through a relay that's answering.
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
fn classify(addr: &TransportAddr) -> Link {
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

/// When a connection's relay paths last heard anything.
#[derive(Debug, Default)]
struct RelayHeard {
    /// Datagrams received over relay paths so far.
    received: u64,
    at: Option<Instant>,
}

impl RelayHeard {
    /// `link`, the selected path's, given that relay paths have received
    /// `received` datagrams by `now`.
    fn link(&mut self, link: Link, received: u64, now: Instant) -> Link {
        if received != self.received {
            self.received = received;
            self.at = Some(now);
        }
        let heard = self
            .at
            .is_some_and(|at| now.saturating_duration_since(at) < RELAY_SILENCE);
        if link == Link::Relay && !heard {
            Link::Offline
        } else {
            link
        }
    }
}

/// Datagrams received over `connection`'s relay paths so far.
fn relay_received(connection: &Connection) -> u64 {
    connection
        .paths()
        .iter()
        .filter(|path| path.is_relay())
        .map(|path| path.stats().udp_rx.datagrams)
        .sum()
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
    fn of(&self, connection: &Connection) -> Link {
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

    /// Changes as they happen. A receiver that falls behind gets
    /// `RecvError::Lagged` and can read [`Links::get`] again.
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
        let mut heard = RelayHeard::default();
        self.follow(&connection, &mut heard);
        tokio::spawn(async move {
            let mut check = tokio::time::interval(RELAY_CHECK);
            loop {
                tokio::select! {
                    event = events.next() => match event {
                        Some(PathEvent::Lagged { .. }) | Some(_) => {}
                        None => break,
                    },
                    _ = check.tick() => {}
                    _ = connection.closed() => break,
                }
                links.follow(&connection, &mut heard);
            }
            links.set(peer, id, None);
        });
    }

    /// Records `connection`'s link as it stands. A relay gone quiet leaves
    /// nothing behind to call the device reachable through it.
    fn follow(&self, connection: &Connection, heard: &mut RelayHeard) {
        let peer = connection.remote_id();
        let selected = self.of(connection);
        let link = heard.link(selected, relay_received(connection), Instant::now());
        if selected != link {
            let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
            last.remove(&peer);
        }
        self.set(peer, connection.stable_id(), Some(link));
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
        // The last link it had open stays known once it closed.
        assert_eq!(links.last(peer), Some(Link::Relay));
        assert_eq!(links.last(iroh::SecretKey::generate().public()), None);
    }

    #[test]
    fn a_relay_path_counts_while_it_hears_something() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut heard = RelayHeard::default();
        assert_eq!(heard.link(Link::Relay, 6, at(0)), Link::Relay);
        // Keep-alives every 5 s or so.
        assert_eq!(heard.link(Link::Relay, 6, at(5)), Link::Relay);
        assert_eq!(heard.link(Link::Relay, 7, at(6)), Link::Relay);
        assert_eq!(heard.link(Link::Relay, 7, at(17)), Link::Relay);
        // Silence ends it, and the next thing heard brings it back.
        assert_eq!(heard.link(Link::Relay, 7, at(18)), Link::Offline);
        assert_eq!(heard.link(Link::Relay, 8, at(30)), Link::Relay);
    }

    /// The cable pulled: iroh falls back to a relay path that went quiet at
    /// the same moment as the direct one, which is already out of time.
    #[test]
    fn falling_back_to_a_silent_relay_path_is_offline_at_once() {
        let start = Instant::now();
        let mut heard = RelayHeard::default();
        assert_eq!(heard.link(Link::Lan, 6, start), Link::Lan);
        let timed_out = start + Duration::from_secs(15);
        assert_eq!(heard.link(Link::Lan, 6, timed_out), Link::Lan);
        assert_eq!(heard.link(Link::Relay, 6, timed_out), Link::Offline);
    }

    #[test]
    fn a_relay_path_never_heard_from_is_no_link() {
        let mut heard = RelayHeard::default();
        assert_eq!(heard.link(Link::Relay, 0, Instant::now()), Link::Offline);
    }

    #[test]
    fn direct_paths_need_no_word_from_a_relay() {
        let later = Instant::now() + Duration::from_secs(60);
        let mut heard = RelayHeard::default();
        assert_eq!(heard.link(Link::Lan, 0, later), Link::Lan);
        assert_eq!(heard.link(Link::Direct, 0, later), Link::Direct);
    }
}
