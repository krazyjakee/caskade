//! Integration tests, compiled as one binary: a change relinks once instead
//! of once per file. Run one file's tests with `cargo test --test it <file>::`.

#[cfg(unix)]
mod across_networks;
/// caskade-core's cores on loopback, which the share tests start from;
/// not every helper is used here.
#[allow(dead_code)]
#[path = "../../caskade-core/tests/cores.rs"]
mod cores;
/// The internet of caskade-core's `scripts/two-networks.sh`.
#[cfg(unix)]
#[path = "../../caskade-core/tests/networks.rs"]
mod networks;
/// The other half of a scenario in network namespaces.
#[cfg(unix)]
#[path = "../../caskade-core/tests/peer.rs"]
mod peer;
mod share_items;
mod shared_cores;
mod shares;
#[cfg(unix)]
mod two_networks;
