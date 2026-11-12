//! The library's virtual grid and list (#94): see [`grid`] and [`window`].

// The Library view (#99) mounts it; until then the tests and the scroll
// benchmark do.
#![allow(dead_code)]

mod anatomy;
mod art;
pub mod bench;
mod grid;
// The library view around the grid (#99).
mod view;
// Making and editing collections; the library saves its view as one (#99).
pub mod collection_dialog;
// The Emulated tab's breakdown by platform, and how platforms show (#99).
mod platform_grid;
pub mod platforms;
// The Web tab: its mosaic, Add web game and the web games' dialogs (#99).
pub mod web;
// Choosing several games, and the bar and dialogs that act on them (#99).
mod bulk_edit;
mod selection;
mod selection_bar;
#[cfg(test)]
pub(crate) mod view_tests;
pub mod window;

#[cfg(test)]
mod collection_tests;
#[cfg(test)]
mod platform_tests;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use art::{SCHEME as ART_SCHEME, fetch as fetch_art, fetch_remote as fetch_remote_art};

/// A game's art for a small place (a search suggestion): the 96 px
/// thumbnail.
pub fn art_url(art: Option<&caskade_core::model::Artwork>) -> Option<String> {
    art::url(art, art::Variant::Icon)
}

/// A game page's hero: its hero art, else its cover, at 960 px.
pub fn art_url_hero(artwork: &[caskade_core::model::Artwork]) -> Option<String> {
    use caskade_core::model::ArtworkKind;
    let hero = artwork
        .iter()
        .find(|art| art.kind == ArtworkKind::Hero)
        .or_else(|| artwork.iter().find(|art| art.kind == ArtworkKind::Cover));
    art::url(hero, art::Variant::Hero)
}

/// A picture's original, for where it fills the window (a screenshot).
pub fn art_url_original(art: &caskade_core::model::Artwork) -> Option<String> {
    cached_url(&art.cached_path)
}

/// A file in the artwork cache (an achievement's picture, say), by its
/// cached path.
pub fn cached_url(cached_path: &str) -> Option<String> {
    (!cached_path.is_empty()).then(|| {
        format!(
            "{}://localhost/{}",
            art::SCHEME,
            cached_path.trim_start_matches('/')
        )
    })
}

/// A game's art for a card or tile: the 320 px thumbnail.
pub fn art_url_card(art: Option<&caskade_core::model::Artwork>) -> Option<String> {
    art::url(art, art::Variant::Card)
}
pub use anatomy::ANATOMY;
pub(crate) use bulk_edit::{ListInput, TextAreaField, field_label};
pub(crate) use grid::Game as LibraryCard;
#[allow(unused_imports)]
pub use grid::{Events, VirtualLibrary};
pub(crate) use selection::use_selection_provider;
pub(crate) use selection_bar::{AllIds, AllIdsProp, SelectionBar};
#[allow(unused_imports)]
pub use view::{DropInstall, LibraryView};
pub use window::Layout;
pub(crate) use window::{GRID_GAP, GRID_ROW, MIN_CARD};

/// A Unix time in milliseconds on the user's clock.
pub fn local_date_time(ms: i64) -> caskade_ui_i18n::intl::DateTime {
    use chrono::{Datelike, TimeZone, Timelike};
    let local = chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(|| chrono::Local.timestamp_millis_opt(0).unwrap());
    caskade_ui_i18n::intl::DateTime {
        year: local.year(),
        month: local.month() as u8,
        day: local.day() as u8,
        hour: local.hour() as u8,
        minute: local.minute() as u8,
        second: local.second() as u8,
    }
}
