//! Game streaming between a household's devices.
//! Caskade installs, configures and drives two separate GPLv3 programs,
//! never linked or patched, and never shown by name: Sunshine on the
//! device that runs games (the host, [`host`]) and Moonlight on the device
//! that shows them (the client).

#[cfg(feature = "net")]
pub mod api;
pub mod client;
pub mod existing;
#[cfg(feature = "net")]
pub mod gamepads;
#[cfg(feature = "net")]
pub mod gamestream;
pub mod host;
#[cfg(feature = "net")]
pub mod hosts;
#[cfg(feature = "net")]
pub mod linked;
#[cfg(feature = "net")]
pub mod mdns;
#[cfg(feature = "net")]
pub mod pairing;
#[cfg(feature = "net")]
pub mod play;
#[cfg(feature = "net")]
pub mod publish;
pub mod secrets;
#[cfg(all(test, feature = "net"))]
mod testing;

pub use client::{Client, StreamingClient};
pub use host::{HostOutcome, HostState, HostStatus, StreamingHost};
