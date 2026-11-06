//! What a client asks a paired host itself, over its HTTPS port with the
//! client's certificate and the host's pinned one (STREAMING-PLAN §5.3,
//! #264): whether it still knows this device, what it is playing, which
//! games it offers, and to quit the game. Moonlight only streams.
//!
//! Also Wake-on-LAN, for a host that sleeps.

use super::{
    api,
    pairing::{Endpoint, Identity},
};
use crate::{Error, Result, db::StreamApp};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::Invalid(error.to_string())
}

fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(xml[start..end].trim())
}

/// XML text with its entities undone.
fn text(xml: &str) -> String {
    xml.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// What a host says over its HTTPS port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostState {
    /// It still knows this device.
    pub paired: bool,
    /// The id of the game it is playing.
    pub playing: Option<String>,
}

/// Reads `/serverinfo`'s answer over HTTPS.
pub fn host_state(xml: &str) -> HostState {
    HostState {
        paired: tag(xml, "PairStatus") == Some("1"),
        playing: tag(xml, "currentgame")
            .filter(|game| !game.is_empty() && *game != "0")
            .map(str::to_owned),
    }
}

/// Reads `/applist`'s answer.
pub fn app_list(xml: &str) -> Vec<StreamApp> {
    xml.split("<App>")
        .skip(1)
        .filter_map(|app| {
            let app = &app[..app.find("</App>")?];
            Some(StreamApp {
                id: tag(app, "ID")?.to_owned(),
                name: text(tag(app, "AppTitle")?),
            })
        })
        .filter(|app| !app.id.is_empty() && !app.name.is_empty())
        .collect()
}

/// The status code a host's XML answer gives.
fn status(xml: &str) -> Option<u16> {
    let at = xml.find("status_code=\"")? + "status_code=\"".len();
    xml[at..].split('"').next()?.parse().ok()
}

/// A paired host, reached with this device's certificate.
#[derive(Clone)]
pub struct HostLink {
    endpoint: Endpoint,
    unique_id: String,
    client: reqwest::Client,
}

impl HostLink {
    /// The host at `endpoint` (its HTTPS port), pinned to the certificate
    /// pairing recorded.
    pub fn new(endpoint: Endpoint, identity: &Identity, server_cert_pem: &str) -> Result<Self> {
        let server = CertificateDer::from_pem_slice(server_cert_pem.as_bytes()).map_err(invalid)?;
        let client =
            CertificateDer::from_pem_slice(identity.cert_pem.as_bytes()).map_err(invalid)?;
        let key = PrivateKeyDer::Pkcs8(
            PrivatePkcs8KeyDer::from_pem_slice(identity.key_pem.as_bytes()).map_err(invalid)?,
        );
        let tls = api::pinned_tls(server, Some((client, key)))?;
        let client = reqwest::Client::builder()
            // The host closes idle connections.
            .pool_max_idle_per_host(0)
            .use_preconfigured_tls(tls)
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(crate::error::net)?;
        Ok(Self {
            endpoint,
            unique_id: identity.unique_id.clone(),
            client,
        })
    }

    async fn get(&self, path: &str) -> std::result::Result<String, Failure> {
        let url = self.endpoint.https.join(path).map_err(invalid)?;
        let response = self
            .client
            .get(url)
            .query(&[
                ("uniqueid", self.unique_id.as_str()),
                ("uuid", &uuid::Uuid::new_v4().simple().to_string()),
            ])
            .send()
            .await
            .map_err(|error| {
                if tls_failure(&error) {
                    Failure::Refused
                } else {
                    Failure::Other(crate::error::net(error))
                }
            })?;
        let code = response.status();
        let body = response.text().await.map_err(crate::error::net)?;
        let answered = status(&body).unwrap_or(code.as_u16());
        if matches!(code.as_u16(), 401 | 403) || matches!(answered, 401 | 403) {
            return Err(Failure::Refused);
        }
        if !code.is_success() || answered != 200 {
            return Err(Failure::Other(Error::Invalid(format!("{code}: {body}"))));
        }
        Ok(body)
    }

    /// Whether the host knows this device, and what it plays. A host that
    /// refuses this device's certificate, or whose own changed, doesn't
    /// know it.
    pub async fn state(&self) -> Result<HostState> {
        match self.get("serverinfo").await {
            Ok(body) => Ok(host_state(&body)),
            Err(Failure::Refused) => Ok(HostState {
                paired: false,
                playing: None,
            }),
            Err(Failure::Other(error)) => Err(error),
        }
    }

    /// The games the host offers this device.
    pub async fn apps(&self) -> Result<Vec<StreamApp>> {
        Ok(app_list(
            &self.get("applist").await.map_err(Failure::into_error)?,
        ))
    }

    /// Quits the game the host is playing.
    pub async fn quit(&self) -> Result<()> {
        let body = self.get("cancel").await.map_err(Failure::into_error)?;
        match tag(&body, "cancel") {
            Some("0") => Err(Error::Invalid(crate::tr!(
                "the device didn't stop its game"
            ))),
            _ => Ok(()),
        }
    }
}

/// Why a request failed.
enum Failure {
    /// The host refused this device, or isn't the device paired with.
    Refused,
    Other(Error),
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Other(error)
    }
}

impl Failure {
    fn into_error(self) -> Error {
        match self {
            Self::Refused => Error::Invalid(crate::tr!("the device no longer knows this one")),
            Self::Other(error) => error,
        }
    }
}

/// Whether TLS failed: the host refused this device's certificate, or
/// its own isn't the one pairing recorded.
fn tls_failure(error: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = source {
        if error.downcast_ref::<rustls::Error>().is_some()
            || error
                .downcast_ref::<std::io::Error>()
                .and_then(|io| io.get_ref())
                .is_some_and(|inner| inner.downcast_ref::<rustls::Error>().is_some())
        {
            return true;
        }
        // Errors that crossed the HTTP client's own boxing keep only
        // their description.
        let described = format!("{error:?}");
        if [
            "AlertReceived",
            "InvalidCertificate",
            "NoCertificatesPresented",
        ]
        .iter()
        .any(|kind| described.contains(kind))
        {
            return true;
        }
        source = error.source();
    }
    false
}

/// A Wake-on-LAN magic packet for `mac`.
pub fn magic_packet(mac: [u8; 6]) -> [u8; 102] {
    let mut packet = [0xff; 102];
    for chunk in packet[6..].chunks_mut(6) {
        chunk.copy_from_slice(&mac);
    }
    packet
}

/// The ports Moonlight sends magic packets to: the usual ones and the
/// streaming ports, which a router may forward.
pub const WAKE_PORTS: [u16; 6] = [7, 9, 47998, 47999, 48000, 48010];

/// Where the magic packet for a host at `addresses` goes: every network's
/// broadcast address and each address, on each of [`WAKE_PORTS`]. The
/// all-networks address alone leaves by the default route, which on a machine
/// with a second card or a VPN isn't the network the host sleeps on.
pub fn wake_targets(addresses: &[IpAddr]) -> Vec<SocketAddr> {
    targets_through(&network_broadcasts(), addresses)
}

fn targets_through(broadcasts: &[Ipv4Addr], addresses: &[IpAddr]) -> Vec<SocketAddr> {
    std::iter::once(Ipv4Addr::BROADCAST)
        .chain(broadcasts.iter().copied())
        .map(IpAddr::V4)
        .chain(addresses.iter().copied().filter(IpAddr::is_ipv4))
        .flat_map(|address| WAKE_PORTS.map(|port| SocketAddr::new(address, port)))
        .collect()
}

/// The broadcast address of each network this machine is on.
fn network_broadcasts() -> Vec<Ipv4Addr> {
    let mut broadcasts: Vec<Ipv4Addr> = netdev::get_interfaces()
        .iter()
        .filter(|interface| !interface.is_loopback())
        .flat_map(|interface| interface.ipv4.iter().map(|net| net.broadcast()))
        .collect();
    broadcasts.sort();
    broadcasts.dedup();
    broadcasts
}

/// Sends the magic packet for `mac` to `targets`.
pub async fn wake(mac: [u8; 6], targets: &[SocketAddr]) -> Result<()> {
    let socket =
        tokio::net::UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)).await?;
    socket.set_broadcast(true)?;
    let packet = magic_packet(mac);
    let mut sent = false;
    for target in targets {
        sent |= socket.send_to(&packet, target).await.is_ok();
    }
    if sent {
        Ok(())
    } else {
        Err(Error::Invalid(crate::tr!(
            "the wake-up message couldn't be sent"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_says_whether_it_knows_this_device_and_what_it_plays() {
        let paired = r#"<?xml version="1.0" encoding="utf-8"?><root status_code="200"><hostname>den</hostname><PairStatus>1</PairStatus><currentgame>1597897691</currentgame><state>SUNSHINE_SERVER_BUSY</state></root>"#;
        assert_eq!(
            host_state(paired),
            HostState {
                paired: true,
                playing: Some("1597897691".into())
            }
        );
        let idle = r#"<root status_code="200"><PairStatus>0</PairStatus><currentgame>0</currentgame></root>"#;
        assert_eq!(
            host_state(idle),
            HostState {
                paired: false,
                playing: None
            }
        );
    }

    #[test]
    fn app_lists_read_titles_and_ids() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?><root status_code="200"><App><IsHdrSupported>0</IsHdrSupported><AppTitle>Ori &amp; the Will</AppTitle><ID>881</ID></App><App><AppTitle>Caskade</AppTitle><ID>12</ID></App><App><AppTitle></AppTitle><ID>3</ID></App></root>"#;
        assert_eq!(
            app_list(xml),
            [
                StreamApp {
                    id: "881".into(),
                    name: "Ori & the Will".into()
                },
                StreamApp {
                    id: "12".into(),
                    name: "Caskade".into()
                },
            ]
        );
        assert_eq!(status(xml), Some(200));
        assert_eq!(
            status(r#"<root status_code="401" status_message="x"/>"#),
            Some(401)
        );
    }

    #[test]
    fn magic_packets_repeat_the_mac_sixteen_times() {
        let mac = [0xa4, 0xb1, 0xc1, 0x09, 0xdb, 0x98];
        let packet = magic_packet(mac);
        assert_eq!(&packet[..6], &[0xff; 6]);
        assert!(packet[6..].chunks(6).all(|chunk| chunk == mac));
        assert_eq!(packet[6..].chunks(6).count(), 16);
    }

    #[test]
    fn a_wake_up_leaves_by_every_network_not_just_the_default_route() {
        let targets = targets_through(
            &[Ipv4Addr::new(192, 168, 77, 255)],
            &[IpAddr::from([192, 168, 77, 12])],
        );
        for address in [
            [255, 255, 255, 255],
            [192, 168, 77, 255],
            [192, 168, 77, 12],
        ] {
            let target = SocketAddr::new(IpAddr::from(address), 9);
            assert!(targets.contains(&target), "{target} in {targets:?}");
        }
    }
}
