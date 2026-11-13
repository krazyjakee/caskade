//! Pairing links: `caskade://pair?t=<ticket>`.
//!
//! The ticket is an iroh ticket (`iroh_tickets::Ticket`: a kind prefix, then
//! unpadded lowercase base32): this device's address and a single-use pairing
//! grant, plus when the link expires. The device that minted it keeps the
//! grant and its expiry; the expiry inside the link only lets the receiving
//! device say "this link expired" without dialling.
//!
//! A link's `Display` is the credential, because producing it is the point.
//! Its `Debug` leaves the grant out.

use std::fmt;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iroh::{EndpointAddr, EndpointId};
use iroh_tickets::{ParseError, Ticket};

use crate::error::Error;
use crate::grant::Grant;
use crate::ticket::{self, LinkTicket};

/// How long a pairing link works.
pub const LINK_LIFETIME: Duration = Duration::from_secs(10 * 60);

/// The URL a pairing link is shared as.
pub const LINK_PREFIX: &str = "caskade://pair?t=";

/// A pairing link: where the device is, a single-use grant, and when it
/// expires.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingLink(LinkTicket<u64>);

impl PairingLink {
    /// A link to `addr` opening with `grant`, expiring at `expires_at`.
    pub fn new(addr: EndpointAddr, grant: Grant, expires_at: SystemTime) -> Self {
        Self(LinkTicket {
            addr,
            grant,
            expires_at: expires_at
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        })
    }

    pub fn addr(&self) -> &EndpointAddr {
        &self.0.addr
    }

    pub fn peer(&self) -> EndpointId {
        self.0.addr.id
    }

    pub fn grant(&self) -> Grant {
        self.0.grant
    }

    pub fn expires_at(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(self.0.expires_at)
    }

    /// Whether the link says it has expired by `now`.
    pub fn expired(&self, now: SystemTime) -> bool {
        now >= self.expires_at()
    }

    /// The `caskade://pair?t=…` URL.
    pub fn to_url(&self) -> String {
        format!("{LINK_PREFIX}{}", self.encode_string())
    }

    /// Reads a link: the URL, or the ticket alone, as pasted (surrounding
    /// whitespace and case don't matter). [`Error::NotALink`] otherwise, with
    /// nothing quoted: the text is a credential.
    pub fn parse(text: &str) -> Result<Self, Error> {
        ticket::parse(text, LINK_PREFIX, usize::MAX)
    }
}

impl Ticket for PairingLink {
    const KIND: &'static str = "caskadepair";

    fn encode_bytes(&self) -> Vec<u8> {
        self.0.encode()
    }

    fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        LinkTicket::decode(bytes).map(Self)
    }
}

impl fmt::Display for PairingLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_url())
    }
}

impl fmt::Debug for PairingLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.debug(f, "PairingLink")
    }
}

impl FromStr for PairingLink {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        Self::parse(text)
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use iroh::TransportAddr;

    use super::*;

    fn link() -> PairingLink {
        let id = iroh::SecretKey::generate().public();
        let addr = EndpointAddr::from_parts(
            id,
            [
                TransportAddr::Ip("192.168.1.20:4100".parse::<SocketAddr>().unwrap()),
                TransportAddr::Relay("https://relay.example./".parse().unwrap()),
            ],
        );
        PairingLink::new(addr, Grant::generate(), SystemTime::now() + LINK_LIFETIME)
    }

    #[test]
    fn a_link_round_trips_as_a_url_and_as_a_bare_ticket() {
        let link = link();
        let url = link.to_url();
        assert!(url.starts_with("caskade://pair?t=caskadepair"));
        assert_eq!(PairingLink::parse(&url).unwrap(), link);
        let ticket = url.strip_prefix(LINK_PREFIX).unwrap();
        assert_eq!(PairingLink::parse(&format!("  {ticket}\n")).unwrap(), link);
        assert_eq!(PairingLink::parse(&url.to_uppercase()).unwrap(), link);
    }

    #[test]
    fn other_text_is_not_a_link() {
        for text in [
            "",
            "hello",
            "caskade://pair?t=",
            "caskade://pair?t=caskadepairzzzz",
            "endpointabc",
        ] {
            assert!(
                matches!(PairingLink::parse(text), Err(Error::NotALink)),
                "{text}"
            );
        }
        let truncated = link().to_url();
        let truncated = &truncated[..truncated.len() - 10];
        assert!(PairingLink::parse(truncated).is_err());
    }

    #[test]
    fn expiry_is_carried_to_the_second() {
        let link = link();
        assert!(!link.expired(SystemTime::now()));
        assert!(link.expired(SystemTime::now() + LINK_LIFETIME + Duration::from_secs(1)));
    }

    #[test]
    fn debug_leaves_the_grant_out() {
        let link = link();
        let dumped = format!("{link:?}");
        assert!(!dumped.contains(&link.grant().to_hex()));
        assert!(!dumped.contains(&link.to_url()[LINK_PREFIX.len()..]));
    }
}
