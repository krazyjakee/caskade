//! The library's themeable roots (#99): the grid or list of games and its
//! letters, choosing several games, the Emulated tab's platforms, the
//! collection dialog and adding a web game.

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root the library view renders.
pub const ANATOMY: &[Entry] = &[
    SELECTION_BAR,
    GAMES_REFRESH_START,
    BULK_EDITOR,
    PLATFORM_CARD,
    VIRTUAL_LIBRARY,
    ALPHABET_BAR,
    COLLECTION_DIALOG,
    ADD_WEB_GAME,
];

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

pub const PLATFORM_CARD: Entry = Entry {
    parts: &[
        ("art", "The hardware render on its glow."),
        ("title", "The platform's short name."),
        ("subtitle", "Its maker and kind of machine."),
        ("count", "How many games it has."),
        ("meta", "Playtime and last play."),
        ("playtime", "Time played on it."),
        ("last-played", "When one of its games was last played."),
    ],
    states: &[("data-art", &["platform", "generic"])],
    ..Entry::new(
        "platform-card",
        "PlatformGrid (apps/native/src/library/platform_grid.rs)",
        Source::App,
        "A platform in the library's breakdown of emulated games; opens its games.",
    )
};

pub const VIRTUAL_LIBRARY: Entry = Entry {
    parts: &[
        (
            "table-header",
            "The list's column headers; in the Library view, Title, Last played and Playtime sort by it.",
        ),
        ("rows", "Holds the rows, as tall as all of them."),
        (
            "virtual-row",
            "One row of games on screen; `data-row` is its number.",
        ),
        ("placeholder", "A game not loaded yet."),
    ],
    states: &[("data-layout", &["grid", "list"])],
    contains: &["game-row", "game-card"],
    ..Entry::new(
        "virtual-library",
        "VirtualLibrary (apps/native/src/library/grid.rs)",
        Source::App,
        "The games of the Library view or a collection's page (#94, #102), as a grid of cards or a list of rows; only the rows on screen are drawn. LT and RT page through them, letter by letter when sorted by title. `data-layout` is `grid` or `list`.",
    )
};

pub const ALPHABET_BAR: Entry = Entry {
    parts: &[(
        "letter",
        "One initial some game starts with (`#` for numbers and symbols); `data-selected` on the letter on screen.",
    )],
    ..Entry::new(
        "alphabet-bar",
        "VirtualLibrary (apps/native/src/library/grid.rs)",
        Source::App,
        "The letters beside a long library sorted by title; each jumps to its games.",
    )
};

pub const COLLECTION_DIALOG: Entry = Entry {
    parts: &[
        (
            "rules",
            "A smart collection's rules: which games, platform, title and sort.",
        ),
        ("matches", "How many games match the rules now."),
        ("save", "Create or Save (a `button`)."),
    ],
    contains: &["segmented", "text-field", "select", "button"],
    ..Entry::new(
        "collection-dialog",
        "CollectionDialog (apps/native/src/library/collection_dialog.rs)",
        Source::App,
        "Makes a collection, or edits one's name, description and rules (#99): from New Collection in Collections, Save as collection in the library (a smart one with the view's rules) and a collection's Edit… or Edit rules…. A new one is Manual or Smart.",
    )
};

pub const ADD_WEB_GAME: Entry = Entry {
    parts: &[("add", "Adds the game (a `button`).")],
    contains: &["text-field", "checkbox", "button"],
    ..Entry::new(
        "add-web-game",
        "AddWebGame (apps/native/src/library/web.rs)",
        Source::App,
        "Add web game in the library's Web tab (#99): the address of the page the game plays on and, where Caskade can keep offline copies, Play offline; with it, Caskade then asks where to keep the copy and opens the guided capture.",
    )
};
