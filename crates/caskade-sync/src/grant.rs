//! Grants: thirty-two random bytes that say a dialler was given access.
//!
//! A grant is not a key. The connection is already encrypted and both ends
//! authenticated by TLS on their endpoint keys; nothing derives, signs or
//! encrypts with a grant. It is an authorization input: a paired device
//! presents the grant this device gave it, a pairing link carries a single-use
//! one, a share link another. The device that minted it decides what it opens.
//!
//! A grant never appears in `Debug` output, and it is compared in constant
//! time so an early-returning compare can't leak it a byte at a time.

use std::fmt;
use std::str::FromStr;

use rand::Rng as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use subtle::ConstantTimeEq as _;

use crate::error::Error;

/// The length of a grant, in bytes.
pub const GRANT_BYTES: usize = 32;

/// A bearer capability: thirty-two bytes from the OS random number generator.
///
/// It serializes as 64 lowercase hex characters. That form is for the opening
/// message and for the database row that remembers it; it must never reach
/// logs, events or the webview.
#[derive(Clone, Copy, Eq)]
pub struct Grant([u8; GRANT_BYTES]);

impl Grant {
    /// A fresh grant.
    pub fn generate() -> Self {
        let mut bytes = [0u8; GRANT_BYTES];
        rand::rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; GRANT_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; GRANT_BYTES] {
        &self.0
    }

    /// The 64-character lowercase hex form, for storage.
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }
}

impl PartialEq for Grant {
    fn eq(&self, other: &Self) -> bool {
        bool::from(self.0.ct_eq(&other.0))
    }
}

impl fmt::Debug for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Grant(..)")
    }
}

impl FromStr for Grant {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        let text = text.as_bytes();
        if text.len() != GRANT_BYTES * 2 {
            return Err(Error::Malformed);
        }
        let mut bytes = [0u8; GRANT_BYTES];
        let (pairs, _) = text.as_chunks::<2>();
        for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
            *byte = (nibble(*high)? << 4) | nibble(*low)?;
        }
        Ok(Self(bytes))
    }
}

impl Serialize for Grant {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Grant {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse()
            .map_err(|_| serde::de::Error::custom("not a grant"))
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn nibble(digit: u8) -> Result<u8, Error> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => Err(Error::Malformed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grant_round_trips_through_its_hex_form() {
        let grant = Grant::generate();
        let text = grant.to_hex();
        assert_eq!(text.len(), 64);
        assert_eq!(text.parse::<Grant>().unwrap(), grant);
        let json = serde_json::to_string(&grant).unwrap();
        assert_eq!(serde_json::from_str::<Grant>(&json).unwrap(), grant);
    }

    #[test]
    fn debug_hides_the_grant() {
        let grant = Grant::from_bytes([0xab; GRANT_BYTES]);
        assert_eq!(format!("{grant:?}"), "Grant(..)");
    }

    #[test]
    fn malformed_text_is_not_a_grant() {
        assert!("".parse::<Grant>().is_err());
        assert!("zz".repeat(32).parse::<Grant>().is_err());
        assert!("ab".repeat(31).parse::<Grant>().is_err());
    }
}
