//! A game's page (#100, the webview's `views/game`): its art, Play and its
//! choices, details, entries, saves, achievements, streaming and ports,
//! and what Play asks before a game starts ([`launch`]), which every Play
//! shares.

mod back;
pub mod data;
mod entry_update;
pub mod launch;
mod page;
mod play_choice;
pub mod present;

pub use page::GameView;

#[cfg(test)]
pub(crate) mod launch_tests;
#[cfg(test)]
pub(crate) mod page_tests;

use crate::ui::{Link, Svg, icons};
use dioxus_native::prelude::*;

/// A link out of the app, opened in the browser.
#[component]
pub fn ExternalLink(
    href: String,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let url = href.clone();
    rsx! {
        Link {
            href,
            class,
            attributes,
            on_press: move |_| {
                if let Ok(url) = url::Url::parse(&url) {
                    caskade_app::navigation::open_in_browser(&url);
                }
            },
            {children}
            Svg { icon: icons::EXTERNAL_LINK, class: "size-3.5" }
        }
    }
}
