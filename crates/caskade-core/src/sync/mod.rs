//! Sync between the user's devices: the change log engine, its service,
//! the exchange with a paired device and what rides along with it.
#[cfg(feature = "net")]
pub mod artwork;
#[cfg(feature = "net")]
pub mod blobs;
pub mod change;
pub mod clock;
#[cfg(feature = "net")]
pub mod config_files;
#[cfg(feature = "net")]
pub mod copies;
pub mod engine;
#[cfg(feature = "net")]
pub mod exchange;
pub mod offers;
pub mod policy;
#[cfg(feature = "net")]
pub mod presence;
#[cfg(feature = "net")]
pub mod service;
pub mod settings;

pub use engine::SyncEngine;
#[cfg(feature = "net")]
pub use service::SyncService;
