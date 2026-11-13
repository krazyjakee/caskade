//! What a start's arguments ask the running Caskade for (#88):
//! `caskade://` links, a magnet link or torrent file, `caskade dev
//! <folder>`. A second start's arrive through the single-instance endpoint
//! (`caskade_app::single_instance`) and show the window; this start's own
//! are taken once the services are up.

use caskade_app::single_instance::{Forwarded, Intent, intents};

use crate::launch::{self, Request};

/// A second start: show the window, then do what it asks.
pub fn take(forwarded: Forwarded) {
    launch::ask(Request::Show);
    act(intents(&forwarded.args, &forwarded.cwd));
}

/// This start's own arguments, once the services are up.
pub fn take_own() {
    act(intents(
        &std::env::args().collect::<Vec<_>>(),
        &std::env::current_dir().unwrap_or_default(),
    ));
}

fn act(intents: Vec<Intent>) {
    if intents.is_empty() {
        return;
    }
    caskade_app::runtime::spawn(async move {
        let services = crate::shell::services_ready().await;
        for intent in intents {
            let raise = match intent {
                Intent::Links(links) => {
                    caskade_app::deep_link::open(
                        caskade_app::deep_link::LinkServices::of(services, crate::shell::sink()),
                        links,
                    )
                    .await
                }
                Intent::Install(input) => {
                    caskade_app::deep_link::queue(
                        &crate::shell::sink(),
                        services.install_requests.as_ref(),
                        Some(&services.accounts),
                        vec![Ok(input)],
                    )
                    .await
                    .1
                }
                Intent::DevFolder(folder) => {
                    let loaded = caskade_app::dev_plugins::load(
                        caskade_app::dev_plugins::DevServices {
                            events: crate::shell::sink(),
                            themes: Some(&services.themes),
                            host: services.plugins.as_ref(),
                            bus: Some(&services.bus),
                            dev_plugins: Some(&services.dev_plugins),
                        },
                        &folder,
                    )
                    .await;
                    if let Err(error) = loaded {
                        tracing::warn!(%error, "the dev folder didn't load");
                    }
                    false
                }
                Intent::Console => {
                    // A second `caskade --console` (#107): a one-off start,
                    // not the toggle, so it isn't remembered.
                    crate::console::request_console();
                    true
                }
            };
            if raise {
                launch::ask(Request::Show);
            }
        }
    });
}
