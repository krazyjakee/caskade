//! The library's themeable roots for choosing several games (#99), word for
//! word the webview's (`packages/ui/src/anatomy.ts`) apart from the
//! component each names.

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root the library's selection renders.
pub const ANATOMY: &[Entry] = &[SELECTION_BAR, GAMES_REFRESH_START, BULK_EDITOR];

pub const SELECTION_BAR: Entry = Entry {
    parts: &[
        ("count", "How many games are chosen."),
        (
            "select-loaded",
            "Chooses the games loaded so far (a `button`).",
        ),
        (
            "select-all",
            "Chooses every game the view finds, with how many (a `button`).",
        ),
        ("clear", "Chooses none (a `button`)."),
        ("edit", "Opens the bulk editor (a `button`)."),
        (
            "refresh",
            "Refreshes the chosen games' details (a `button`).",
        ),
        ("done", "Leaves selection mode (a `button`)."),
    ],
    contains: &["button"],
    ..Entry::new(
        "selection-bar",
        "SelectionBar (apps/native/src/library/selection_bar.rs)",
        Source::App,
        "The bar of a library or collection view in selection mode: how many games are chosen, choosing the loaded ones or all of them, clearing, the bulk editor and Done.",
    )
};

pub const GAMES_REFRESH_START: Entry = Entry {
    parts: &[("start", "Starts the refresh (a `button`).")],
    contains: &["button", "segmented"],
    ..Entry::new(
        "games-refresh-start",
        "RefreshDialog (apps/native/src/library/selection_bar.rs)",
        Source::App,
        "Starts refreshing the chosen games' details and artwork from the built-in sources: missing details only, or the automatic ones fetched again.",
    )
};

pub const BULK_EDITOR: Entry = Entry {
    parts: &[
        ("fields", "The fields, each changed only when ticked."),
        (
            "field",
            "One field (`data-field`; `data-enabled` when ticked).",
        ),
        ("list", "A list field's values and its add field."),
        ("chip", "One value of a list."),
        ("remove", "Removes a value from its list (a `button`)."),
        ("results", "The preview or the result."),
        (
            "summary",
            "How many games change, already have it, can't take it or are gone.",
        ),
        ("change", "One game and what changes in it."),
        ("rejected", "A game that can't take the edit, and why."),
        (
            "footer",
            "Status, Back or Cancel, and Preview, Apply or Done.",
        ),
        ("status", "Working, or what went wrong."),
        ("cancel", "Back to the fields, or Cancel (a `button`)."),
        ("apply", "Preview, Apply or Done (a `button`)."),
    ],
    states: &[("data-step", &["edit", "preview", "done"])],
    contains: &["button", "checkbox", "segmented", "text-field", "slider"],
    ..Entry::new(
        "bulk-editor",
        "BulkEditDialog (apps/native/src/library/bulk_edit.rs)",
        Source::App,
        "One edit on the chosen games: the fields to change, a preview of what changes in which game, and what saved.",
    )
};
