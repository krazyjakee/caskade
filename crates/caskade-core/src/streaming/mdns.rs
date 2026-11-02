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

/// Asks `target` (the mDNS group, or a test's responder) and listens for
/// `wait`.
pub async fn browse(target: SocketAddr, wait: Duration) -> Result<Vec<Found>> {
    let bind = if target.ip().is_loopback() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
    } else {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
    };
    let socket = tokio::net::UdpSocket::bind(bind).await?;
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
    Ok(hosts(&answers))
}
