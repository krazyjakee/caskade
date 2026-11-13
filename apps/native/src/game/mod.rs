//! A game's page (#100, the webview's `views/game`): its art, Play and its
//! choices, details, entries, saves, achievements, streaming and ports,
//! and what Play asks before a game starts ([`launch`]), which every Play
//! shares; and its editor ([`editor`]).

mod achievements;
mod anatomy;
mod back;
pub mod data;
pub mod editor;
mod elsewhere;
mod emulation;
mod entry_update;
mod igdb;
pub mod launch;
mod media;
mod page;
mod play_choice;
mod ports;
pub mod present;
mod saves;

pub use anatomy::ANATOMY;
pub(crate) use back::{GameBackLink, STORE_GAMES};
pub(crate) use elsewhere::{PlayingElsewhereMark, provide_playing_marks};
pub(crate) use media::{
    AgeRatingLabel, BOARDS, DEFAULT_BOARD, Screenshots, Shot, Videos, board_name, rating_name,
    shown_rating, use_preferred_board,
};
pub use page::GameView;

#[cfg(test)]
pub(crate) mod launch_tests;
#[cfg(test)]
pub(crate) mod page_tests;

use crate::ui::{Link, Svg, icons};
use dioxus_native::prelude::*;

/// An operating system's name, for people.
fn system_name(system: &str) -> String {
    match system {
        "windows" => "Windows".to_owned(),
        "mac" | "macos" => "macOS".to_owned(),
        "linux" => "Linux".to_owned(),
        other => other.to_owned(),
    }
}

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
