//! The collections views' themeable roots (#102), word for word the
//! webview's (`packages/ui/src/anatomy.ts`) apart from the component each
//! names.

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root the collections views render of their own.
pub const ANATOMY: &[Entry] = &[PANEL, COLLECTION_CARD];

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
