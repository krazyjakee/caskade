//! Downloads a plugin asked for stay inside its network grant (CSK-03): the
//! URL and every redirect must be on a granted host, and a name must
//! resolve to public addresses. A loopback, private, link-local or
//! unspecified address is reached only when the grant names that address
//! itself (`127.0.0.1`, `[::1]`, `192.168.1.20`, or `localhost` for
//! loopback), so a granted name that resolves to the LAN (DNS rebinding
//! included) is never connected to.
//!
//! The address rules are public because the plugin host applies them to a
//! backend's own `wasi:http` requests too, with its port rule on top. An
//! entry may name a port (`127.0.0.1:8384`, `[::1]:8080`); downloads match
//! its host only.
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use reqwest::Client;
use url::Url;

use super::{FetchConfig, FetchError, Result};

/// The marker a refused DNS answer carries through reqwest.
#[derive(Debug, thiserror::Error)]
#[error("{host} resolves only to addresses the plugin's network permission doesn't name")]
pub(super) struct Refused {
    pub(super) host: String,
}

/// Whether `host` matches an entry: the exact host, or any subdomain of
/// `*.domain`. Case-insensitive; the plugin host uses the same rule.
pub fn host_allowed(allowlist: &[String], host: &str) -> bool {
    allowlist.iter().any(|entry| host_matches(entry, host))
}

/// Whether one entry's host covers `host`.
pub fn host_matches(entry: &str, host: &str) -> bool {
    let Some((name, _)) = entry_parts(entry) else {
        return false;
    };
    let host = bare(host).trim_end_matches('.').to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    match name.strip_prefix("*.") {
        Some(domain) => host.len() > domain.len() + 1 && host.ends_with(&format!(".{domain}")),
        None => host == name,
    }
}

/// An entry's host, without an IPv6 address's brackets, and the port it
/// names: `example.com`, `127.0.0.1:8384`, `[::1]`, `[::1]:8080`. `None`
/// when the port isn't a number from 1 to 65535 or a bracket isn't closed.
pub fn entry_parts(entry: &str) -> Option<(&str, Option<u16>)> {
    let port = |text: &str| text.parse::<u16>().ok().filter(|port| *port != 0);
    if let Some(rest) = entry.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        return match after {
            "" => Some((host, None)),
            after => Some((host, Some(port(after.strip_prefix(':')?)?))),
        };
    }
    match entry.rsplit_once(':') {
        // A bare IPv6 address has more than one colon and names no port.
        Some((host, after)) if !host.contains(':') => Some((host, Some(port(after)?))),
        _ => Some((entry, None)),
    }
}

fn bare(host: &str) -> &str {
    host.trim_start_matches('[').trim_end_matches(']')
}

/// Whether the entry names `ip` itself (`localhost` names loopback).
pub fn names_ip(entry: &str, ip: IpAddr) -> bool {
    entry_parts(entry).is_some_and(|(name, _)| {
        name.parse::<IpAddr>().is_ok_and(|named| named == ip)
            || (name.eq_ignore_ascii_case("localhost") && ip.is_loopback())
    })
}

/// Whether the grant names `ip` itself.
fn named(allowlist: &[String], ip: IpAddr) -> bool {
    allowlist.iter().any(|entry| names_ip(entry, ip))
}

/// Not loopback, private, link-local, shared (CGNAT), unspecified,
/// broadcast, multicast or documentation space.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || a >= 240)
        }
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => public_ip(IpAddr::V4(v4)),
            None => {
                let first = ip.segments()[0];
                !(ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    || (first & 0xfe00) == 0xfc00
                    || (first & 0xffc0) == 0xfe80)
            }
        },
    }
}

fn reachable(allowlist: &[String], ip: IpAddr) -> bool {
    public_ip(ip) || named(allowlist, ip)
}

/// Refuses a URL (the download's or a redirect's) the grant doesn't cover.
pub(super) fn check(allowlist: &[String], url: &Url) -> Result<()> {
    let host = url.host_str().unwrap_or_default();
    let allowed = host_allowed(allowlist, host)
        && match url.host() {
            Some(url::Host::Domain(_)) => true,
            Some(url::Host::Ipv4(ip)) => reachable(allowlist, IpAddr::V4(ip)),
            Some(url::Host::Ipv6(ip)) => reachable(allowlist, IpAddr::V6(ip)),
            None => false,
        };
    if allowed {
        Ok(())
    } else {
        Err(FetchError::NotGranted { host: host.into() })
    }
}

/// The marker's host when `error` is a refused DNS answer.
pub(super) fn refused(error: &reqwest::Error) -> Option<String> {
    let mut source = std::error::Error::source(error);
    while let Some(inner) = source {
        if let Some(refused) = inner.downcast_ref::<Refused>() {
            return Some(refused.host.clone());
        }
        source = inner.source();
    }
    None
}

/// A client like the engine's whose DNS answers keep only addresses the
/// grant may reach.
pub(super) fn client(config: &FetchConfig, allowlist: &[String]) -> Result<Client> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(config.connect_timeout)
        .read_timeout(config.stall_timeout)
        .use_rustls_tls()
        .dns_resolver(Arc::new(Resolver(Arc::new(allowlist.to_vec()))))
        .build()
        .map_err(FetchError::http)
}

struct Resolver(Arc<Vec<String>>);

impl reqwest::dns::Resolve for Resolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(addresses(self.0.clone(), name.as_str().to_owned()))
    }
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

async fn addresses(
    allowlist: Arc<Vec<String>>,
    host: String,
) -> std::result::Result<reqwest::dns::Addrs, BoxError> {
    let lookup = host.clone();
    let found = tokio::task::spawn_blocking(move || {
        std::net::ToSocketAddrs::to_socket_addrs(&(lookup.as_str(), 0))
            .map(|addresses| addresses.collect::<Vec<SocketAddr>>())
    })
    .await??;
    let kept = filter(&allowlist, found);
    if kept.is_empty() {
        return Err(Box::new(Refused { host }));
    }
    Ok(Box::new(kept.into_iter()))
}

fn filter(allowlist: &[String], found: Vec<SocketAddr>) -> Vec<SocketAddr> {
    found
        .into_iter()
        .filter(|address| reachable(allowlist, address.ip()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|entry| (*entry).to_owned()).collect()
    }
    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn every_hop_must_be_a_granted_host_and_addresses_must_be_public_or_named() {
        let store = grant(&["store.example", "*.cdn.example"]);
        assert!(check(&store, &url("https://store.example/game.zip")).is_ok());
        assert!(check(&store, &url("https://eu.cdn.example/game.zip")).is_ok());
        for refused in [
            "https://other.example/",
            "http://127.0.0.1:8384/rest/system/config",
            "http://[::1]/",
            "http://192.168.1.20/",
            "http://localhost/",
        ] {
            assert!(
                matches!(
                    check(&store, &url(refused)),
                    Err(FetchError::NotGranted { .. })
                ),
                "{refused}"
            );
        }
        // A grant that names a local address reaches it.
        let lan = grant(&["192.168.1.20", "[::1]", "localhost"]);
        assert!(check(&lan, &url("http://192.168.1.20:8080/rom.zip")).is_ok());
        assert!(check(&lan, &url("http://[::1]/rom.zip")).is_ok());
        assert!(check(&lan, &url("http://localhost/rom.zip")).is_ok());
        assert!(check(&lan, &url("http://192.168.1.21/")).is_err());
    }

    #[test]
    fn dns_answers_keep_only_public_or_named_addresses() {
        let address = |text: &str| SocketAddr::new(text.parse().unwrap(), 0);
        let found = vec![
            address("127.0.0.1"),
            address("10.0.0.5"),
            address("169.254.169.254"),
            address("0.0.0.0"),
            address("fe80::1"),
            address("93.184.216.34"),
        ];
        assert_eq!(
            filter(&grant(&["rebind.example"]), found.clone()),
            vec![address("93.184.216.34")]
        );
        assert_eq!(
            filter(&grant(&["rebind.example", "10.0.0.5"]), found),
            vec![address("10.0.0.5"), address("93.184.216.34")]
        );
    }

    #[test]
    fn entries_may_name_a_port_and_downloads_match_their_host() {
        assert_eq!(entry_parts("example.com"), Some(("example.com", None)));
        assert_eq!(
            entry_parts("127.0.0.1:8384"),
            Some(("127.0.0.1", Some(8384)))
        );
        assert_eq!(entry_parts("[::1]"), Some(("::1", None)));
        assert_eq!(entry_parts("[::1]:8080"), Some(("::1", Some(8080))));
        assert_eq!(entry_parts("::1"), Some(("::1", None)));
        for malformed in [
            "example.com:",
            "example.com:0",
            "example.com:http",
            "[::1",
            "[::1]8080",
        ] {
            assert_eq!(entry_parts(malformed), None, "{malformed}");
        }
        let lan = grant(&["192.168.1.20:8080", "[::1]:8080"]);
        assert!(check(&lan, &url("http://192.168.1.20:8080/rom.zip")).is_ok());
        assert!(check(&lan, &url("http://[::1]:8080/rom.zip")).is_ok());
        assert!(!host_allowed(&grant(&["example.com:"]), "example.com"));
    }
}
