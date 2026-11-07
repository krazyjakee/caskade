//! Finding streaming hosts on the local network (STREAMING-PLAN §6.1):
//! one mDNS question for `_nvstream._tcp.local`, asked from an ordinary
//! port so responders answer it directly (RFC 6762 §6.7, legacy unicast),
//! and the answers' PTR, SRV and A records put together. The instance name
//! is the host's OS host name; its name in Caskade comes from
//! `/serverinfo`.

use crate::{Error, Result};
use hickory_proto::{
    op::{Message, MessageType, OpCode, Query},
    rr::{Name, RData, RecordType},
};
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::{Duration, Instant},
};

/// The service Sunshine announces.
pub const SERVICE: &str = "_nvstream._tcp.local.";
/// Where mDNS questions go.
pub const MULTICAST: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 251)), 5353);

/// A host that answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Its mDNS instance name.
    pub instance: String,
    pub address: IpAddr,
    /// Its HTTP port (`/serverinfo`).
    pub port: u16,
}

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::Invalid(error.to_string())
}

/// The question.
pub fn query() -> Result<Vec<u8>> {
    let mut message = Message::new(
        uuid::Uuid::new_v4().as_u128() as u16,
        MessageType::Query,
        OpCode::Query,
    );
    message.add_query(Query::query(
        Name::from_ascii(SERVICE).map_err(invalid)?,
        RecordType::PTR,
    ));
    message.to_vec().map_err(invalid)
}

/// The hosts in `answers` (each with the address it came from).
pub fn hosts(answers: &[(Vec<u8>, IpAddr)]) -> Vec<Found> {
    let service = Name::from_ascii(SERVICE).ok();
    let mut instances: Vec<(Name, IpAddr)> = Vec::new();
    let mut services: BTreeMap<Name, (Name, u16)> = BTreeMap::new();
    let mut addresses: BTreeMap<Name, IpAddr> = BTreeMap::new();
    for (bytes, from) in answers {
        let Ok(message) = Message::from_vec(bytes) else {
            continue;
        };
        if message.metadata.message_type != MessageType::Response {
            continue;
        }
        for record in message.answers.iter().chain(&message.additionals) {
            match &record.data {
                RData::PTR(ptr) if Some(&record.name) == service.as_ref() => {
                    if !instances.iter().any(|(name, _)| name == &ptr.0) {
                        instances.push((ptr.0.clone(), *from));
                    }
                }
                RData::SRV(srv) => {
                    services.insert(record.name.clone(), (srv.target.clone(), srv.port));
                }
                RData::A(a) => {
                    addresses.insert(record.name.clone(), IpAddr::V4(a.0));
                }
                _ => {}
            }
        }
    }
    instances
        .into_iter()
        .filter_map(|(instance, from)| {
            let (target, port) = services.get(&instance)?;
            let label = instance.iter().next()?;
            Some(Found {
                instance: String::from_utf8_lossy(label).into_owned(),
                // The record's address, else where the answer came from.
                address: addresses.get(target).copied().unwrap_or(from),
                port: *port,
            })
        })
        .collect()
}

/// Asks `target` (the mDNS group, or a test's responder) from every network
/// this machine is on, and listens for `wait`. A socket left to the OS asks
/// only over the default route, so a PC with a second network card, a VPN
/// or Docker would never find the hosts on its LAN.
pub async fn browse(target: SocketAddr, wait: Duration) -> Result<Vec<Found>> {
    let from = if target.ip().is_loopback() {
        vec![Ipv4Addr::LOCALHOST]
    } else {
        networks()
    };
    browse_from(target, &from, wait).await
}

/// How long [`networks`]' answer is reused. Hosts are looked for every few
/// seconds, and listing the interfaces reads several files for each one (a
/// machine running Docker has dozens), megabytes of allocations a time.
const NETWORKS_FOR: Duration = Duration::from_secs(30);

/// This machine's addresses on the networks a question can leave by, as
/// listed at most [`NETWORKS_FOR`] ago.
fn networks() -> Vec<Ipv4Addr> {
    static LISTED: std::sync::Mutex<Option<(Instant, Vec<Ipv4Addr>)>> = std::sync::Mutex::new(None);
    let mut listed = LISTED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, addresses)) = listed.as_ref()
        && at.elapsed() < NETWORKS_FOR
    {
        return addresses.clone();
    }
    let addresses = list_networks();
    *listed = Some((Instant::now(), addresses.clone()));
    addresses
}

fn list_networks() -> Vec<Ipv4Addr> {
    let mut addresses: Vec<Ipv4Addr> = netdev::get_interfaces()
        .iter()
        .filter(|interface| interface.is_up() && !interface.is_loopback())
        .flat_map(|interface| interface.ipv4.iter().map(|net| net.addr()))
        .collect();
    addresses.sort();
    addresses.dedup();
    addresses
}

/// Asks from each of `addresses`, or as the OS chooses without any. An
/// address that can't ask leaves the others to; only all failing is an error.
async fn browse_from(
    target: SocketAddr,
    addresses: &[Ipv4Addr],
    wait: Duration,
) -> Result<Vec<Found>> {
    let mut asking = tokio::task::JoinSet::new();
    let any = [Ipv4Addr::UNSPECIFIED];
    let addresses = if addresses.is_empty() {
        &any
    } else {
        addresses
    };
    for address in addresses {
        asking.spawn(ask(*address, target, wait));
    }
    let mut answers = Vec::new();
    let mut failure = None;
    let mut asked = false;
    while let Some(result) = asking.join_next().await {
        match result {
            Ok(Ok(mut heard)) => {
                asked = true;
                answers.append(&mut heard);
            }
            Ok(Err(error)) => failure = Some(error),
            Err(error) => tracing::debug!(%error, "an mDNS question stopped"),
        }
    }
    match failure {
        Some(error) if !asked => Err(error),
        _ => Ok(hosts(&answers)),
    }
}

/// One question from `address`, and what comes back within `wait`.
async fn ask(
    address: Ipv4Addr,
    target: SocketAddr,
    wait: Duration,
) -> Result<Vec<(Vec<u8>, IpAddr)>> {
    let socket = tokio::net::UdpSocket::bind(SocketAddr::new(IpAddr::V4(address), 0)).await?;
    if !address.is_unspecified() && target.ip().is_multicast() {
        socket2::SockRef::from(&socket).set_multicast_if_v4(&address)?;
    }
    socket.send_to(&query()?, target).await?;
    let deadline = Instant::now() + wait;
    let mut answers = Vec::new();
    let mut buffer = vec![0_u8; 9000];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match tokio::time::timeout(left, socket.recv_from(&mut buffer)).await {
            Ok(Ok((read, from))) => answers.push((buffer[..read].to_vec(), from.ip())),
            Ok(Err(error)) => return Err(error.into()),
            Err(_) => break,
        }
    }
    Ok(answers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unusable address leaves the question to the others, and when
    /// nothing can ask, that's the error.
    #[tokio::test]
    async fn one_network_that_cant_ask_leaves_it_to_the_others() {
        let responder = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let target = responder.local_addr().unwrap();
        let unusable = Ipv4Addr::new(192, 0, 2, 1);
        tokio::spawn(async move {
            let mut buffer = [0_u8; 512];
            while let Ok((_, from)) = responder.recv_from(&mut buffer).await {
                let _ = responder.send_to(b"heard", from).await;
            }
        });
        let wait = Duration::from_millis(200);
        let heard = browse_from(target, &[unusable, Ipv4Addr::LOCALHOST], wait).await;
        assert!(heard.unwrap().is_empty(), "no hosts in that answer");
        assert!(browse_from(target, &[unusable], wait).await.is_err());
    }

    #[test]
    fn networks_leave_out_loopback() {
        assert!(!networks().contains(&Ipv4Addr::LOCALHOST));
    }
}
