use caskade_sync::{Ip, Relays};

use super::{NOWHERE, Network};

/// The policy a build and environment choose.
fn chosen(debug: bool, vars: &[(&str, &str)]) -> Network {
    Network::chosen(debug, |name| {
        vars.iter()
            .find(|(var, _)| *var == name)
            .map(|(_, value)| (*value).to_owned())
    })
}

/// Fixtures mode, and every program a private test desktop starts, run on
/// loopback only (#167); a release build never does, whatever it's told.
#[test]
fn fixture_and_test_desktop_runs_are_loopback_only() {
    assert_eq!(
        chosen(true, &[("CASKADE_FIXTURES", "1")]),
        Network::Loopback
    );
    assert_eq!(
        chosen(true, &[("CASKADE_NETWORK", "loopback")]),
        Network::Loopback
    );
    assert_eq!(chosen(true, &[]), Network::Open);
    assert_eq!(chosen(true, &[("CASKADE_FIXTURES", "0")]), Network::Open);
    assert_eq!(
        chosen(
            false,
            &[("CASKADE_FIXTURES", "1"), ("CASKADE_NETWORK", "loopback")]
        ),
        Network::Open
    );
}

/// On loopback only, nothing a service starts or asks leaves the machine:
/// no online lookups, servers and GitHub on a refused loopback port, mDNS
/// asked there (#167).
#[test]
fn loopback_only_keeps_every_service_on_loopback() {
    let network = Network::Loopback;
    assert!(!network.online_lookups());
    for real in [caskade_core::igdb::BASE_URL, caskade_addons::DEFAULT_ORIGIN] {
        let server = url::Url::parse(network.server(real)).unwrap();
        assert_eq!(server.host_str(), Some("127.0.0.1"), "{real}");
    }
    assert_eq!(network.server("https://example.com"), NOWHERE);
    assert_eq!(network.web().host_str(), Some("127.0.0.1"));
    assert!(network.mdns().ip().is_loopback());
}

/// Fixtures mode starts the real services: their endpoint stays on
/// loopback, with nothing that probes the router or the internet (#135).
/// `caskade-sync` checks that a loopback endpoint turns port mapping off.
#[test]
fn fixture_runs_keep_the_sync_endpoint_on_loopback() {
    let reach = Network::Loopback
        .sync_reach()
        .expect("loopback only chooses the reach");
    assert!(matches!(reach.ip, Ip::Loopback { .. }), "{reach:?}");
    assert!(matches!(reach.relays, Relays::Never), "{reach:?}");
    assert!(!reach.internet_lookup, "{reach:?}");
    assert!(reach.lan.is_none(), "{reach:?}");
}

/// Otherwise the services reach what they always did.
#[test]
fn the_app_reaches_where_its_settings_say() {
    let network = Network::Open;
    assert!(network.sync_reach().is_none());
    assert!(network.online_lookups());
    assert_eq!(
        network.server(caskade_core::igdb::BASE_URL),
        caskade_core::igdb::BASE_URL
    );
    assert_eq!(network.web().as_str(), "https://github.com/");
    assert_eq!(network.mdns(), caskade_streaming::mdns::MULTICAST);
}
