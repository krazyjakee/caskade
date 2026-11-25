//! What the services may reach (#135, #167), decided once in [`super::start`].

/// Network policy for this run of the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Network {
    /// The internet and the LAN, as the settings allow: every release build.
    Open,
    /// 127.0.0.1 only, for the window tests' runs: sync binds on loopback,
    /// the online lookups (IGDB matching, Hasheous, Ludusavi, ports) don't
    /// run, servers asked on demand (the metadata server, the add-on
    /// catalogue, GitHub's releases) are [`NOWHERE`], so each request fails
    /// as it does offline, and streaming hosts are looked for there rather
    /// than by multicast.
    Loopback,
}

/// A loopback port nothing listens on: a request there is refused at once.
pub(super) const NOWHERE: &str = "http://127.0.0.1:1";

impl Network {
    /// This run's: loopback only in a debug build in fixtures mode
    /// (`CASKADE_FIXTURES=1`, as the native shell reads it) or when told so
    /// (`CASKADE_NETWORK=loopback`, which `caskade-test-desktop` gives every
    /// program it starts).
    pub(super) fn of_this_run() -> Self {
        Self::chosen(cfg!(debug_assertions), |name| std::env::var(name).ok())
    }

    /// The policy for a build (`debug`) and its environment (`var`).
    fn chosen(debug: bool, var: impl Fn(&str) -> Option<String>) -> Self {
        let loopback = var("CASKADE_FIXTURES").as_deref() == Some("1")
            || var("CASKADE_NETWORK").as_deref() == Some("loopback");
        if debug && loopback {
            Self::Loopback
        } else {
            Self::Open
        }
    }

    /// Where the sync endpoint binds whatever the settings say: on loopback
    /// only, with no relays, address lookup, mDNS or router port mapping, so
    /// sync sends nothing beyond it (#135). `None`: where the settings say.
    pub(super) fn sync_reach(self) -> Option<caskade_core::devices::Reach> {
        (self == Self::Loopback).then(caskade_core::devices::Reach::loopback)
    }

    /// Whether the online lookups run (while `core.metadata.online` allows).
    pub(super) fn online_lookups(self) -> bool {
        self == Self::Open
    }

    /// The server to ask in place of `real`.
    pub(super) fn server(self, real: &str) -> &str {
        match self {
            Self::Open => real,
            Self::Loopback => NOWHERE,
        }
    }

    /// GitHub's address, for game updates.
    pub(super) fn web(self) -> url::Url {
        let web = format!("{}/", self.server("https://github.com"));
        url::Url::parse(&web).expect("valid URL")
    }

    /// Where streaming hosts are asked for over mDNS.
    pub(super) fn mdns(self) -> std::net::SocketAddr {
        match self {
            Self::Open => caskade_streaming::mdns::MULTICAST,
            Self::Loopback => std::net::SocketAddr::from(([127, 0, 0, 1], 1)),
        }
    }
}

#[cfg(test)]
#[path = "../start_tests.rs"]
mod tests;
