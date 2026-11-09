//! Integration tests, compiled as one binary: a change relinks once instead
//! of once per file. Run one file's tests with `cargo test --test it <file>::`.

/// caskade-core's cores on loopback, which the share tests start from;
/// not every helper is used here.
#[allow(dead_code)]
#[path = "../../caskade-core/tests/cores.rs"]
mod cores;
mod share_items;
mod shares;
