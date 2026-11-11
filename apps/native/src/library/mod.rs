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
// Choosing several games, and the bar and dialogs that act on them (#99).
mod bulk_edit;
mod selection;
mod selection_bar;
#[cfg(test)]
pub(crate) mod view_tests;
pub mod window;

#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use art::{SCHEME as ART_SCHEME, fetch as fetch_art};

/// A game's art for a small place (a search suggestion): the 96 px
/// thumbnail.
pub fn art_url(art: Option<&caskade_core::model::Artwork>) -> Option<String> {
    art::url(art, art::Variant::Icon)
}
pub use anatomy::ANATOMY;
#[allow(unused_imports)]
pub use grid::{Events, VirtualLibrary};
#[allow(unused_imports)]
pub use view::{DropInstall, LibraryView};
pub use window::Layout;

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
