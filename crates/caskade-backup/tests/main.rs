//! Integration tests, compiled as one binary: a change relinks once instead
//! of once per file. Run one file's tests with `cargo test --test it <file>::`.

#[cfg(feature = "net")]
mod peer_backups;
mod sync_policy;
