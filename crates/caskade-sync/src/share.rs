//! Share links: `caskade://receive?t=<ticket>` (#246).
//!
//! A game shared once with another person: the sharer's address (local and,
//! once there is one, relay) and the share's grant, plus when the link
//! expires, if ever. The grant is for that share alone, not a paired device:
//! the sharer's Caskade admits a `share` session only while the share is
//! running, so Stop sharing makes the link fail at once, unlike an iroh
//! ticket on its own. The expiry inside the link only lets the receiver say
//! "this link expired" without dialling; the sharer enforces it.
//!
//! A link is a credential: only [`ShareLink::to_url`] writes it out.
//! Neither `Debug` nor anything else prints the grant.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iroh::{EndpointAddr, EndpointId, TransportAddr};
use iroh_tickets::{ParseError, Ticket};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::grant::Grant;

/// The URL a share link is sent as.
pub const SHARE_PREFIX: &str = "caskade://receive?t=";
/// The longest text read as a link: a ticket with many addresses is well
/// under it.
const MAX_LINK: usize = 4096;

/// A share link: where the sharer is, the share's grant, and when the link
/// expires (`None`: until the sharer stops sharing).
#[derive(Clone, PartialEq, Eq)]
pub struct ShareLink {
    addr: EndpointAddr,
    grant: Grant,
    /// Seconds since the Unix epoch.
    expires_at: Option<u64>,
}

impl ShareLink {
    /// A link to `addr` opening with `grant`, expiring at `expires_at`.
    pub fn new(addr: EndpointAddr, grant: Grant, expires_at: Option<SystemTime>) -> Self {
        Self {
            addr,
            grant,
            expires_at: expires_at.map(|at| {
                at.duration_since(UNIX_EPOCH)
                    .map_or(0, |since| since.as_secs())
            }),
        }
    }

    pub fn addr(&self) -> &EndpointAddr {
        &self.addr
    }

    pub fn peer(&self) -> EndpointId {
        self.addr.id
    }

    pub fn grant(&self) -> Grant {
        self.grant
    }

    /// When the link stops working; `None` until the sharer stops it.
    pub fn expires_at(&self) -> Option<SystemTime> {
        self.expires_at
            .map(|at| UNIX_EPOCH + Duration::from_secs(at))
    }

    /// Whether the link says it has expired by `now`.
    pub fn expired(&self, now: SystemTime) -> bool {
        self.expires_at().is_some_and(|at| now >= at)
    }

    /// Whether the link can reach the sharer through a relay, besides its
    /// direct addresses (game files never take that path).
    pub fn has_relay(&self) -> bool {
        self.addr
            .addrs
            .iter()
            .any(|addr| matches!(addr, TransportAddr::Relay(_)))
    }

    /// The `caskade://receive?t=…` URL.
    pub fn to_url(&self) -> String {
        format!("{SHARE_PREFIX}{}", self.encode_string())
    }

    /// Reads a link: the URL, or the ticket alone, as pasted (surrounding
    /// whitespace and case don't matter). [`Error::NotALink`] otherwise,
    /// with nothing quoted: the text is a credential.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let text = text.trim();
        if text.len() > MAX_LINK {
            return Err(Error::NotALink);
        }
        let ticket = strip_prefix_ignore_case(text, SHARE_PREFIX).unwrap_or(text);
        let ticket = ticket.split(['&', '#']).next().unwrap_or_default();
        Self::decode_string(&ticket.to_ascii_lowercase()).map_err(|_| Error::NotALink)
    }
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

#[derive(Serialize, Deserialize)]
enum Wire {
    V1 {
        id: EndpointId,
        addrs: BTreeSet<TransportAddr>,
        grant: [u8; 32],
        expires_at: Option<u64>,
    },
}

impl Ticket for ShareLink {
    const KIND: &'static str = "caskadeshare";

    fn encode_bytes(&self) -> Vec<u8> {
        postcard::to_stdvec(&Wire::V1 {
            id: self.addr.id,
            addrs: self.addr.addrs.clone(),
            grant: *self.grant.as_bytes(),
            expires_at: self.expires_at,
        })
        .expect("a share link serializes")
    }

    fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        let Wire::V1 {
            id,
            addrs,
            grant,
            expires_at,
        } = postcard::from_bytes(bytes)?;
        Ok(Self {
            addr: EndpointAddr { id, addrs },
            grant: Grant::from_bytes(grant),
            expires_at,
        })
    }
}

impl fmt::Debug for ShareLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShareLink")
            .field("peer", &self.addr.id)
            .field("addrs", &self.addr.addrs.len())
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl FromStr for ShareLink {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        Self::parse(text)
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;

    fn link(expires: Option<SystemTime>) -> ShareLink {
        let id = iroh::SecretKey::generate().public();
        let addr = EndpointAddr::from_parts(
            id,
            [
                TransportAddr::Ip("192.168.1.20:4100".parse::<SocketAddr>().unwrap()),
                TransportAddr::Relay("https://relay.example./".parse().unwrap()),
            ],
        );
        ShareLink::new(addr, Grant::generate(), expires)
    }

    #[test]
    fn a_link_round_trips_as_a_url_and_as_a_bare_ticket() {
        let week = SystemTime::now() + Duration::from_secs(7 * 24 * 3600);
        for link in [link(Some(week)), link(None)] {
            let url = link.to_url();
            assert!(url.starts_with("caskade://receive?t=caskadeshare"));
            assert_eq!(ShareLink::parse(&url).unwrap(), link);
            let ticket = url.strip_prefix(SHARE_PREFIX).unwrap();
            assert_eq!(ShareLink::parse(&format!("  {ticket}\n")).unwrap(), link);
            assert_eq!(ShareLink::parse(&url.to_uppercase()).unwrap(), link);
            assert!(link.has_relay());
        }
    }

    #[test]
    fn other_text_and_pairing_links_are_not_share_links() {
        let long = format!("{}{}", link(None).to_url(), "a".repeat(MAX_LINK));
        for text in ["", "hello", "caskade://receive?t=", "caskade://receive?t=caskadesharezz", &long] {
            assert!(matches!(ShareLink::parse(text), Err(Error::NotALink)), "{text}");
        }
        let pairing = crate::PairingLink::new(
            link(None).addr().clone(),
            Grant::generate(),
            SystemTime::now() + crate::LINK_LIFETIME,
        );
        assert!(ShareLink::parse(&pairing.to_url()).is_err());
    }

    #[test]
    fn a_link_expires_or_lasts_until_stopped() {
        let day = SystemTime::now() + Duration::from_secs(24 * 3600);
        let dated = link(Some(day));
        assert!(!dated.expired(SystemTime::now()));
        assert!(dated.expired(day + Duration::from_secs(1)));
        let open = link(None);
        assert_eq!(open.expires_at(), None);
        assert!(!open.expired(SystemTime::now() + Duration::from_secs(10 * 365 * 24 * 3600)));
    }

    #[test]
    fn debug_leaves_the_grant_out() {
        let link = link(None);
        let dumped = format!("{link:?}");
        assert!(!dumped.contains(&link.grant().to_hex()));
        assert!(!dumped.contains(&link.to_url()[SHARE_PREFIX.len()..]));
    }
}
