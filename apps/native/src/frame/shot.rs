//! The `shell` reference screen (#96): the real frame, over the fixture
//! library, open on the library's grid, and `home` (#98), open on Home, for
//! the theme screenshots (`crate::shots`). Their data loads after the first
//! render, so a shot waits until the cards are there ([`ready`],
//! [`home_ready`]).

use crate::data::{DataSource, use_data_source};
use crate::input::InputRoot;
use blitz_dom::{BaseDocument, LocalName};
use dioxus_native::prelude::*;
use std::sync::OnceLock;

/// The fixture library the screen shows: the webview's 24 plain games and
/// the emulated ones.
static SOURCE: OnceLock<DataSource> = OnceLock::new();

pub fn screen() -> Element {
    framed("/library?filter=all&sort=title&layout=grid")
}

/// The `home` reference screen (#98): the same frame opened on Home.
pub fn home() -> Element {
    framed("/")
}

fn framed(start: &'static str) -> Element {
    use_data_source(|| {
        SOURCE
            .get_or_init(|| {
                caskade_app::runtime::block_on(DataSource::fixtures(
                    caskade_ui_data::fixtures::DEFAULT_COUNT,
                ))
                .expect("the fixture library")
            })
            .clone()
    });
    rsx! {
        InputRoot {
            super::Frame { start }
        }
    }
}

/// Whether the screen has its data: the grid shows games.
pub fn ready(doc: &BaseDocument) -> bool {
    let mut found = false;
    doc.visit(|_, node| {
        found |= node.attr(LocalName::from("data-index")).is_some();
    });
    found
}

/// Whether Home has its data: its shelves show games.
pub fn home_ready(doc: &BaseDocument) -> bool {
    let mut cards = 0;
    let mut hero = false;
    doc.visit(|_, node| {
        cards += usize::from(node.attr(LocalName::from("data-cs")) == Some("game-card"));
        hero |= node.attr(LocalName::from("data-part")) == Some("eyebrow");
    });
    hero && cards >= 24
}
