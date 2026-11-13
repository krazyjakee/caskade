//! The collections views' themeable roots (#102).

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root the collections views render of their own.
pub const ANATOMY: &[Entry] = &[PANEL, COLLECTION_CARD, NEW_COLLECTION, ADD_GAMES];

pub const PANEL: Entry = Entry {
    container: true,
    ..Entry::new(
        "panel",
        "CollectionsView (apps/native/src/collections/list.rs), CollectionView (apps/native/src/collections/page.rs)",
        Source::App,
        "A framed panel inside a view.",
    )
};

pub const COLLECTION_CARD: Entry = Entry {
    parts: &[
        ("art", "The mosaic of its games."),
        ("badge", "Smart or manual."),
        ("pin", "Shown on pinned collections."),
        ("count", "How many games it holds."),
        ("title", "The collection's name."),
        ("subtitle", "Its description."),
        ("meta", "Playtime and when it changed."),
    ],
    states: &[("data-kind", &["smart", "manual"])],
    ..Entry::new(
        "collection-card",
        "CollectionCard (apps/native/src/collections/card.rs)",
        Source::App,
        "A collection tile.",
    )
};

pub const NEW_COLLECTION: Entry = Entry {
    interaction: &["data-hovered", "data-focus-visible", "data-pressed"],
    ..Entry::new(
        "new-collection",
        "CollectionsView (apps/native/src/collections/list.rs)",
        Source::App,
        "The New Collection tile after the collection cards in Collections (#102); it opens the dialog that makes one. Its line says Make your first one when there are no collections yet, else Manual list or smart filter.",
    )
};

pub const ADD_GAMES: Entry = Entry {
    parts: &[("games", "The games found, a `checkbox` each.")],
    contains: &["text-field", "checkbox", "button"],
    ..Entry::new(
        "add-games",
        "AddGamesDialog (apps/native/src/collections/add_games.rs)",
        Source::App,
        "Add games… for a manual collection (#102): a search of the library and a checkbox per game that adds it to the collection or takes it out at once. The first 50 games show; a search finds the rest.",
    )
};
