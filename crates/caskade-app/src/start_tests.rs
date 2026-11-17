use caskade_sync::{Ip, Relays};

use super::sync_reach;

/// Fixtures mode starts the real services: their endpoint stays on
/// loopback, with nothing that probes the router or the internet (#135).
/// `caskade-sync` checks that a loopback endpoint turns port mapping off.
#[test]
fn fixture_runs_keep_the_sync_endpoint_on_loopback() {
    let reach = sync_reach(true).expect("fixtures mode chooses the reach");
    assert!(matches!(reach.ip, Ip::Loopback { .. }), "{reach:?}");
    assert!(matches!(reach.relays, Relays::Never), "{reach:?}");
    assert!(!reach.internet_lookup, "{reach:?}");
    assert!(reach.lan.is_none(), "{reach:?}");
}

#[test]
fn the_app_binds_where_its_settings_say() {
    assert!(sync_reach(false).is_none());
}
