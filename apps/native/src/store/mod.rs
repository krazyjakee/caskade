//! The Store (#102, the webview's `StoreView` and `views/store`): a
//! discovery catalogue of all of IGDB through the metadata server, not only
//! the library's games. Browse searches it (or lists the past year's
//! most-rated) with platform, genre and release-year filters; Recently
//! released and Upcoming list releases; a catalogue game's page says what
//! IGDB knows of it and where it is sold. Cards show the library's own
//! status for games matched to the same IGDB game; store links never claim
//! a price or that a game is on sale. The Store installs nothing itself.

mod card;
pub mod data;
mod game;
mod releases;
mod view;

#[cfg(test)]
pub(crate) mod harness;
#[cfg(test)]
mod tests;

pub use game::{STORE_LINKS, StoreGameView};
pub use view::StoreView;
