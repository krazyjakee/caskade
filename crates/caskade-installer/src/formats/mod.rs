//! Built-in format families. Feature flags describe payloads, not the host OS.

pub mod archive;
#[cfg(feature = "disc")]
pub mod disc;
#[cfg(feature = "linux-formats")]
pub mod linux;
#[cfg(feature = "macos-formats")]
pub mod macos;
pub mod portable;
pub mod rom;
#[cfg(feature = "windows-formats")]
pub mod windows;
