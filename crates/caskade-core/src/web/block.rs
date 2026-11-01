//! Ad and tracking hosts a web game's window refuses (WEB-GAMES-PLAN §6),
//! while capturing and while playing a bundle. A short built-in list for
//! now: the hosts portals' and games' ads and analytics come from.

/// Hosts refused with their subdomains.
const HOSTS: &[&str] = &[
    "2mdn.net",
    "33across.com",
    "adform.net",
    "adinplay.com",
    "adnxs.com",
    "adsafeprotected.com",
    "adservice.google.com",
    "amazon-adsystem.com",
    "amplitude.com",
    "applovin.com",
    "casalemedia.com",
    "clarity.ms",
    "cloudflareinsights.com",
    "cpmstar.com",
    "criteo.com",
    "criteo.net",
    "doubleclick.net",
    "google-analytics.com",
    "googleadservices.com",
    "googlesyndication.com",
    "googletagmanager.com",
    "googletagservices.com",
    "hotjar.com",
    "imasdk.googleapis.com",
    "indexww.com",
    "moatads.com",
    "openx.net",
    "outbrain.com",
    "playwire.com",
    "pubmatic.com",
    "quantserve.com",
    "rubiconproject.com",
    "scorecardresearch.com",
    "sharethrough.com",
    "smartadserver.com",
    "taboola.com",
    "teads.tv",
    "unityads.unity3d.com",
    "venatusmedia.com",
];

/// Whether requests to `host` are refused.
pub fn blocked(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    HOSTS.iter().any(|blocked| {
        host == *blocked
            || host
                .strip_suffix(blocked)
                .is_some_and(|rest| rest.ends_with('.'))
    })
}

#[cfg(test)]
mod tests {
    use super::blocked;

    #[test]
    fn ad_hosts_and_their_subdomains_are_blocked() {
        assert!(blocked("doubleclick.net"));
        assert!(blocked("securepubads.g.doubleclick.net"));
        assert!(blocked("PageAd2.GoogleSyndication.com."));
        assert!(blocked("imasdk.googleapis.com"));
        assert!(!blocked("googleapis.com"), "fonts and APIs games use");
        assert!(!blocked("notdoubleclick.net"));
        assert!(!blocked("html-classic.itch.zone"));
        assert!(!blocked("games.poki.com"));
    }
}
