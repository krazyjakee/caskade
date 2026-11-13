//! `caskade://install` deep links (#57): `?url=` from websites, `?path=`
//! from the CLI. Any web page can open these links, so a deep link only
//! becomes a pending install request; nothing is resolved, downloaded or
//! run until the user confirms it in the app.

use url::Url;

pub const SCHEME: &str = "caskade";

/// The path or URL a `caskade://install?url=…` / `?path=…` link names.
pub fn install_input(link: &Url) -> Result<String, String> {
    if link.scheme() != SCHEME {
        return Err(caskade_core::tr!("not a {scheme}: link", scheme = SCHEME));
    }
    // `caskade://install?…`, or `caskade:install?…` from strict handlers.
    let action = format!("{}{}", link.host_str().unwrap_or_default(), link.path());
    if action.trim_matches('/') != "install" {
        return Err(caskade_core::tr!("unknown deep link action"));
    }
    let mut input = None;
    for (key, value) in link.query_pairs() {
        if !matches!(key.as_ref(), "url" | "path") {
            continue;
        }
        if input.is_some() {
            return Err(caskade_core::tr!("a deep link installs one path or URL"));
        }
        input = Some((key == "url", value.into_owned()));
    }
    let (url, value) =
        input.ok_or_else(|| caskade_core::tr!("a deep link needs ?url= or ?path="))?;
    let web = Url::parse(&value)
        .is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https" | "magnet"));
    match (url, web) {
        (true, false) => Err(caskade_core::tr!(
            "?url= takes an HTTP(S) URL or a magnet link"
        )),
        (false, true) => Err(caskade_core::tr!("?path= takes a local path")),
        _ => Ok(value),
    }
}

/// Queues each install input from outside the app (deep links, and the
/// magnet links and torrent files the OS hands Caskade) in `requests` for
/// confirmation and tells the UI. Only an account that may install queues
/// one (#205); the UI is told why not, since a link has no caller to
/// answer. With the queued requests, whether the shell should bring its
/// window up.
pub async fn queue(
    events: &crate::events::Events,
    requests: Option<&crate::commands::install_requests::InstallRequests>,
    accounts: Option<&caskade_core::accounts::AccountService>,
    inputs: Vec<Result<String, String>>,
) -> (Vec<crate::commands::install_requests::InstallRequest>, bool) {
    let Some(requests) = requests else {
        tracing::warn!("install requests unavailable; deep links ignored");
        return (Vec::new(), false);
    };
    if let Err(error) =
        crate::permissions::require(accounts, caskade_core::accounts::Permission::Install)
    {
        tracing::warn!(%error, "refused deep links");
        if let Err(error) = events.emit(crate::events::Event::InstallRefused(
            crate::events::InstallRefused(error),
        )) {
            tracing::warn!(%error, "couldn't tell the UI about refused deep links");
        }
        return (Vec::new(), true);
    }
    let mut queued = Vec::new();
    for input in inputs {
        let result = match input {
            Ok(input) => requests.request_and_emit(events, input).await,
            Err(error) => Err(error),
        };
        // The link may carry tokens: log the outcome, never the link.
        match result {
            Ok(request) => {
                tracing::info!(request_id = %request.request_id, kind = ?request.kind, "deep link awaits confirmation");
                queued.push(request);
            }
            Err(error) => tracing::warn!(%error, "ignored a deep link"),
        }
    }
    let raise = !queued.is_empty();
    (queued, raise)
}

/// What opening links reaches: the services, when they started.
pub struct LinkServices<'a> {
    pub events: crate::events::Events,
    pub requests: Option<&'a crate::commands::install_requests::InstallRequests>,
    pub accounts: Option<&'a caskade_core::accounts::AccountService>,
    pub shares: Option<&'a caskade_share::Shares>,
    pub store_accounts: Option<&'a caskade_core::store_accounts::StoreAccounts>,
    pub opened_pairing_link: Option<&'a crate::commands::sync::OpenedPairingLink>,
}

impl<'a> LinkServices<'a> {
    /// Everything [`crate::start::start`] started.
    pub fn of(services: &'a crate::start::Services, events: crate::events::Events) -> Self {
        Self {
            events,
            requests: services.install_requests.as_ref(),
            accounts: Some(&services.accounts),
            shares: services.shares.as_ref(),
            store_accounts: Some(&services.store_accounts),
            opened_pairing_link: Some(&services.opened_pairing_link),
        }
    }
}

/// Opens `caskade://` links from outside the app: pairing links wait for an
/// admin (#226), share links ask their sharer (#246), store sign-ins finish
/// (#165), and the rest are install requests. Whether the shell should
/// bring its window up.
pub async fn open(services: LinkServices<'_>, links: Vec<Url>) -> bool {
    let mut raise = false;
    let (pairs, links): (Vec<_>, Vec<_>) = links
        .into_iter()
        .partition(crate::commands::sync::is_pair_link);
    for link in pairs {
        raise |= crate::commands::sync::hold_pair_link(
            &services.events,
            services.opened_pairing_link,
            &link,
        );
    }
    let (shares, links): (Vec<_>, Vec<_>) = links
        .into_iter()
        .partition(crate::commands::shares::is_share_link);
    for link in shares {
        raise = true;
        let queued = match (services.shares, services.requests) {
            (Some(shares), Some(requests)) => {
                match crate::permissions::require(
                    services.accounts,
                    caskade_core::accounts::Permission::Install,
                ) {
                    Ok(()) => crate::commands::shares::queue_share_link(
                        &services.events,
                        shares,
                        requests,
                        link.to_string(),
                    )
                    .await
                    .map(|_| ()),
                    Err(error) => Err(error),
                }
            }
            _ => Err(caskade_core::tr!("receiving shares isn't available")),
        };
        if let Err(error) = queued {
            tracing::info!("an opened share link didn't queue");
            services.events.send(crate::events::Event::InstallRefused(
                crate::events::InstallRefused(error),
            ));
        }
    }
    let (callbacks, installs): (Vec<_>, Vec<_>) = links
        .into_iter()
        .partition(crate::commands::store_accounts::is_callback);
    for link in callbacks {
        if let Some(store_accounts) = services.store_accounts {
            crate::commands::store_accounts::callback(store_accounts, services.accounts, &link)
                .await;
        }
    }
    if !installs.is_empty() {
        let (_, raised) = queue(
            &services.events,
            services.requests,
            services.accounts,
            installs.iter().map(install_input).collect(),
        )
        .await;
        raise |= raised;
    }
    raise
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<String, String> {
        install_input(&Url::parse(text).unwrap())
    }

    /// What reached the UI.
    #[derive(Clone, Default)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<crate::events::Event>>>);

    impl crate::events::Emit for Sink {
        fn emit(&self, event: crate::events::Event) -> Result<(), String> {
            self.0.lock().unwrap().push(event);
            Ok(())
        }
    }

    /// The shells' path: an install link waits for confirmation and a
    /// share link that can't be received says why; both raise the window.
    #[test]
    fn opened_links_queue_installs_and_explain_refused_shares() {
        crate::runtime::block_on(async {
            let store = caskade_core::db::LibraryStore::in_memory().await.unwrap();
            // The library's lone admin, signed in: it may install.
            let accounts = caskade_core::accounts::AccountService::open(
                store,
                caskade_core::bus::Bus::default(),
            )
            .await
            .unwrap();
            let requests = crate::commands::install_requests::InstallRequests::default();
            let sink = Sink::default();
            let services = || LinkServices {
                events: crate::events::Events::new(sink.clone()),
                requests: Some(&requests),
                accounts: Some(&accounts),
                shares: None,
                store_accounts: None,
                opened_pairing_link: None,
            };

            let install = Url::parse_with_params(
                "caskade://install",
                [("url", "https://example.invalid/game.zip")],
            )
            .unwrap();
            assert!(open(services(), vec![install]).await);
            let pending = requests.pending();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].value, "https://example.invalid/game.zip");
            assert!(matches!(
                sink.0.lock().unwrap().as_slice(),
                [crate::events::Event::InstallRequested(_)]
            ));

            let share = caskade_sync::share::ShareLink::new(
                caskade_sync::EndpointAddr::new(caskade_sync::Identity::ephemeral().id()),
                caskade_sync::Grant::generate(),
                None,
            );
            let share = Url::parse(&share.to_url()).unwrap();
            assert!(open(services(), vec![share]).await);
            assert_eq!(requests.pending().len(), 1, "nothing more is queued");
            assert!(matches!(
                sink.0.lock().unwrap().as_slice(),
                [_, crate::events::Event::InstallRefused(_)]
            ));

            assert!(!open(services(), Vec::new()).await);
        });
    }

    #[test]
    fn a_link_names_one_path_or_url() {
        assert_eq!(
            parse("caskade://install?url=https%3A%2F%2Fexample.com%2Fgame.zip%3Ft%3D1").unwrap(),
            "https://example.com/game.zip?t=1"
        );
        assert_eq!(
            parse("caskade://install/?path=%2Fhome%2Fme%2FGame%20Setup.exe").unwrap(),
            "/home/me/Game Setup.exe"
        );
        assert_eq!(
            parse("caskade:install?path=/tmp/game").unwrap(),
            "/tmp/game"
        );
        // A magnet link (#159), percent-encoded inside the deep link.
        assert_eq!(
            parse("caskade://install?url=magnet%3A%3Fxt%3Durn%3Abtih%3A0123456789abcdef0123456789abcdef01234567").unwrap(),
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"
        );
        for bad in [
            "caskade://install",
            "caskade://uninstall?path=/tmp/game",
            "caskade://install/extra?path=/tmp/game",
            "caskade://install?url=/tmp/game",
            "caskade://install?url=file:///etc/passwd",
            "caskade://install?path=https://example.com/game.zip",
            "caskade://install?url=https://example.com/a&path=/tmp/game",
            "caskade://install?url=https://example.com/a&url=https://example.com/b",
            "https://install?url=https://example.com/game.zip",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
