//! The `shell` reference screen (#96): the real frame, over the fixture
//! library, open on the library's grid, for the theme screenshots
//! (`crate::shots`). Its data loads after the first render, so the shot
//! waits until the grid's cards are there ([`ready`]).

use crate::data::{DataSource, use_data_source};
use crate::input::InputRoot;
use blitz_dom::{BaseDocument, LocalName};
use dioxus_native::prelude::*;
use std::sync::OnceLock;

/// The fixture library the screen shows: the webview's 24 plain games and
/// the emulated ones.
static SOURCE: OnceLock<DataSource> = OnceLock::new();

pub fn screen() -> Element {
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
            super::Frame { start: "/library?filter=all&sort=title&layout=grid" }
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
