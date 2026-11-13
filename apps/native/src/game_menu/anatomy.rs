//! The game menus' themeable roots (#99).

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root the game menus render.
pub const ANATOMY: &[Entry] = &[QUICK_LOOK, COLLECTION_PICKER];

pub const QUICK_LOOK: Entry = Entry {
    parts: &[
        ("art", "The hero or cover (a `game-art`)."),
        ("close", "Closes it (a `button`)."),
        ("platform", "Its platforms."),
        ("title", "The game's title."),
        ("actions", "Play, and View more beneath it."),
        ("play", "Play, or Stop while it runs (a `button`)."),
        ("open", "View more: opens the game's page (a `button`)."),
        ("notice", "Why it can't be played."),
        ("facts", "Playtime, last play, size and release."),
        ("playtime", "Time played."),
        ("last-played", "When it was last played."),
        ("size", "Its size on disk."),
        ("released", "Its release date."),
        ("genres", "Its genres and tags."),
        ("description", "The game's description."),
    ],
    contains: &["game-art", "status-badge", "button"],
    ..Entry::new(
        "quick-look",
        "QuickLook (apps/native/src/game_menu/quick_look.rs)",
        Source::App,
        "A game at a glance over the current view: in desktop mode a column on the right that a click on a game opens, in console mode a modal from a pad's X on a card; X again plays it.",
    )
};

pub const COLLECTION_PICKER: Entry = Entry {
    parts: &[("collections", "The manual collections, a `checkbox` each.")],
    contains: &["checkbox", "text-field", "button"],
    ..Entry::new(
        "collection-picker",
        "CollectionPicker (apps/native/src/game_menu/collections.rs)",
        Source::App,
        "Add to collection… in a game's menu (#99): a checkbox per manual collection that puts the game in it or takes it out at once, and a new collection with the game in it. Smart collections pick their own games, so they aren't listed.",
    )
};
