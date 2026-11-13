//! Which of a nearby device's addresses to show first.
//!
//! A device announces every address it has, Docker's and VM bridges'
//! included, and those sort first (`172.17.0.1` before `192.168.…`). Every
//! Docker host has the same `172.17.0.1`, so it says nothing about where the
//! device is. Connecting tries all of them; this only orders the list so the
//! one shown is the address on the network both devices share.

use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use netdev::ipnet::IpNet;

/// This machine's networks, split by whether the interface is hardware.
#[derive(Debug, Clone, Default)]
pub(super) struct Networks {
    /// Wired and wireless adapters: the networks other devices are on.
    real: Vec<IpNet>,
    /// Bridges, tunnels and other virtual interfaces (Docker, VMs, VPNs).
    virt: Vec<IpNet>,
}

/// How long [`networks`]' answer is reused. The devices view asks on every
/// refresh, and listing the interfaces reads several files for each one (a
/// machine running Docker has dozens).
const NETWORKS_FOR: Duration = Duration::from_secs(30);

/// This machine's networks, as listed at most [`NETWORKS_FOR`] ago.
pub(super) fn networks() -> Networks {
    static LISTED: Mutex<Option<(Instant, Networks)>> = Mutex::new(None);
    let mut listed = crate::lock(&LISTED);
    if let Some((at, networks)) = listed.as_ref()
        && at.elapsed() < NETWORKS_FOR
    {
        return networks.clone();
    }
    let mut networks = Networks::default();
    for interface in netdev::get_interfaces() {
        if !interface.is_up() || interface.is_loopback() {
            continue;
        }
        let nets = interface
            .ipv4
            .iter()
            .copied()
            .map(IpNet::V4)
            .chain(interface.ipv6.iter().copied().map(IpNet::V6));
        if interface.is_physical() {
            networks.real.extend(nets);
        } else {
            networks.virt.extend(nets);
        }
    }
    *listed = Some((Instant::now(), networks.clone()));
    networks
}

/// Orders `addrs` best first: on one of this machine's real networks, then
/// any other local IPv4 address, then the rest. An address on one of this
/// machine's virtual networks comes last: that bridge is this machine's own
/// (Docker's `172.17.0.0/16` exists on every Docker host), not a way to the
/// other device. Ties keep their order.
pub(super) fn rank(addrs: &mut [SocketAddr], networks: &Networks) {
    let on = |nets: &[IpNet], ip: IpAddr| nets.iter().any(|net| net.contains(&ip));
    addrs.sort_by_key(|addr| {
        let ip = addr.ip().to_canonical();
        if on(&networks.virt, ip) {
            3
        } else if on(&networks.real, ip) {
            0
        } else if ip.is_ipv4() && caskade_sync::link::is_local(ip) && !ip.is_loopback() {
            1
        } else {
            2
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nets(list: &[&str]) -> Vec<IpNet> {
        list.iter().map(|net| net.parse().unwrap()).collect()
    }

    fn ranked(addrs: &[&str], networks: &Networks) -> Vec<String> {
        let mut addrs: Vec<SocketAddr> = addrs.iter().map(|addr| addr.parse().unwrap()).collect();
        rank(&mut addrs, networks);
        addrs.iter().map(SocketAddr::to_string).collect()
    }

    #[test]
    fn the_shared_network_comes_before_a_docker_bridge() {
        let networks = Networks {
            real: nets(&["192.168.0.129/16"]),
            virt: nets(&["172.17.0.1/16"]),
        };
        let addrs = ["172.17.0.1:39572", "[fe80::1]:39572", "192.168.0.155:39572"];
        assert_eq!(
            ranked(&addrs, &networks),
            ["192.168.0.155:39572", "[fe80::1]:39572", "172.17.0.1:39572"]
        );
    }

    #[test]
    fn without_a_shared_network_a_local_ipv4_address_comes_first() {
        let networks = Networks {
            real: nets(&["10.0.0.5/24"]),
            virt: Vec::new(),
        };
        let addrs = ["[fd00::7]:1", "8.8.8.8:1", "192.168.1.20:1", "172.17.0.1:1"];
        assert_eq!(
            ranked(&addrs, &networks),
            ["192.168.1.20:1", "172.17.0.1:1", "[fd00::7]:1", "8.8.8.8:1"]
        );
    }
}
