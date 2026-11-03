//! Connections between a household's Caskade devices.
//!
//! This crate is the floor of sync: an iroh endpoint with this device's
//! identity, and the opening every connection starts with. It has no Tauri
//! dependency and no database; `caskade-core` keeps paired devices and
//! decides who is admitted, through [`Admission`].
//!
//! ## What a connection is
//!
//! iroh is QUIC with TLS 1.3, where each endpoint's ed25519 public key is its
//! certificate. By the time a [`Session`] exists the connection is encrypted
//! and both sides have proved they hold the key behind the endpoint id the
//! other sees ([`Session::peer`]). Nothing in this crate adds a key exchange,
//! signature or challenge on top.
//!
//! The opening ([`opening`]) states what the connection is about, a
//! [`Purpose`], and presents a [`Grant`]: the grant the accepting device gave
//! the dialler when they paired, or the single-use grant in a pairing link.
//! The answer is accepted or [`REFUSAL`], one constant whatever the reason.
//!
//! Messages are one JSON object per line, each read with a bound set by this
//! build ([`FrameReader::read`]); no length a peer wrote is trusted.
//!
//! ## Reaching devices
//!
//! [`Reach::network`] finds devices on the local network over mDNS first
//! ([`lan`]), then through n0's address lookup, and uses n0's relays only
//! when allowed ([`Relays::Never`] turns them off). [`link`] reports how each
//! device is reached (lan, direct, relay or offline) and follows a connection
//! from a relay to a direct path.
//!
//! ## Identity
//!
//! [`Identity`] keeps the device key in the OS keychain
//! ([`KEYCHAIN_SERVICE`]) and never anywhere else. A missing or unreadable
//! key after first use is never replaced silently: see [`Loaded`].
//!
//! ## Runtime and lifetime
//!
//! No runtime is created here: the endpoint runs on the caller's tokio
//! runtime. [`SyncEndpoint`] holds iroh's `Router`, which aborts its accept
//! loop when dropped; hold it for the process lifetime and call
//! [`SyncEndpoint::shutdown`] at exit.
//!
//! ## Versions
//!
//! iroh 1.0.3 with iroh-blobs 0.103.1 and iroh-tickets 1.0.0, the set wobu
//! proved together (0.103.1 is n0's patch release of the yanked 0.103.0).
//! iroh-base, iroh-relay, iroh-dns and noq are held at the versions iroh 1.0.3
//! shipped with in `Cargo.lock`. Mismatched generations fail at resolve time
//! with an `ed25519-dalek` error that never names iroh.

pub mod copy;
pub mod device;
pub mod endpoint;
pub mod error;
pub mod frame;
pub mod grant;
pub mod identity;
pub mod lan;
pub mod link;
pub mod link_ticket;
pub mod opening;
pub mod pairing;
pub mod share;
pub mod swarm;

pub use device::{DeviceInfo, DeviceKind, DeviceOs};
pub use endpoint::{
    Admission, Config, InternetChanges, Ip, Reach, Relays, Session, Sessions, SyncEndpoint,
};
pub use error::{Error, Result};
pub use frame::{FrameReader, FrameWriter};
pub use grant::Grant;
pub use identity::{Identity, KEYCHAIN_SERVICE, KeyStore, Keychain, Loaded, Lost, MemoryKeyStore};
pub use iroh::{EndpointAddr, EndpointId, TransportAddr};
pub use lan::{Lan, Nearby, NearbyChanged};
pub use link::{Link, LinkChanged, Links};
pub use link_ticket::{LINK_LIFETIME, PairingLink};
pub use opening::{Purpose, REFUSAL};
pub use pairing::{CodePairing, Paired};

/// The ALPN Caskade's devices speak. It names the protocol and its version:
/// TLS negotiates it before any byte of the opening, so the opening carries no
/// version of its own.
pub const ALPN: &[u8] = b"caskade/sync/1";
