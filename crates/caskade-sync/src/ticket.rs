//! What pairing and share links have in common: an iroh ticket carrying an
//! address, a grant and an expiry, read leniently from pasted text.

use std::collections::BTreeSet;
use std::fmt;

use iroh::{EndpointAddr, EndpointId, TransportAddr};
use iroh_tickets::{ParseError, Ticket};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::grant::Grant;

/// A link's fields; `E` is its expiry in seconds since the Unix epoch.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LinkTicket<E> {
    pub(crate) addr: EndpointAddr,
    pub(crate) grant: Grant,
    pub(crate) expires_at: E,
}

#[derive(Serialize, Deserialize)]
enum Wire<E> {
    V1 {
        id: EndpointId,
        addrs: BTreeSet<TransportAddr>,
        grant: [u8; 32],
        expires_at: E,
    },
}

impl<E: Serialize + DeserializeOwned + Copy + fmt::Debug> LinkTicket<E> {
    pub(crate) fn encode(&self) -> Vec<u8> {
        postcard::to_stdvec(&Wire::V1 {
            id: self.addr.id,
            addrs: self.addr.addrs.clone(),
            grant: *self.grant.as_bytes(),
            expires_at: self.expires_at,
        })
        .expect("a link serializes")
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, ParseError> {
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

    /// `Debug` without the grant.
    pub(crate) fn debug(&self, f: &mut fmt::Formatter<'_>, name: &str) -> fmt::Result {
        f.debug_struct(name)
            .field("peer", &self.addr.id)
            .field("addrs", &self.addr.addrs.len())
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// Reads a link: the URL starting `prefix`, or the ticket alone, as pasted
/// (surrounding whitespace and case don't matter), and no longer than `max`.
/// [`Error::NotALink`] otherwise, with nothing quoted: the text is a
/// credential.
pub(crate) fn parse<T: Ticket>(text: &str, prefix: &str, max: usize) -> Result<T, Error> {
    let text = text.trim();
    if text.len() > max {
        return Err(Error::NotALink);
    }
    let ticket = strip_prefix_ignore_case(text, prefix).unwrap_or(text);
    let ticket = ticket.split(['&', '#']).next().unwrap_or_default();
    T::decode_string(&ticket.to_ascii_lowercase()).map_err(|_| Error::NotALink)
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}
