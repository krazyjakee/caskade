//! Home's themeable roots (#98). Its hero is `ui::hero`'s entry.

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root Home renders of its own.
pub const ANATOMY: &[Entry] = &[SHELF_ROW];

pub const SHELF_ROW: Entry = Entry {
    parts: &[
        ("header", "The title row."),
        ("icon", "The shelf's icon."),
        ("title", "The shelf's name."),
        ("pager", "The LB/RB buttons, in console mode."),
        ("view-all", "The link to every game in the shelf."),
        ("items", "The row of cards."),
    ],
    slots: &["items"],
    ..Entry::new(
        "shelf-row",
        "ShelfRow (apps/native/src/home/shelf_row.rs)",
        Source::App,
        "A row of game cards that pages with LB/RB (Home's shelves).",
    )
};
