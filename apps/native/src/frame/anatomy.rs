//! The frame's themeable roots (#96), word for word the webview's
//! (`packages/ui/src/anatomy.ts`) apart from the component each names.

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every root the frame renders.
pub const ANATOMY: &[Entry] = &[
    APP_SHELL,
    BACKDROP,
    SIDEBAR,
    TOPBAR,
    WINDOW_CONTROLS,
    VIEW,
    STATUS_BAR,
    VIEW_ERROR,
    SEARCH,
    SEARCH_POPOVER,
    TYPE_SEARCH,
    NOTIFICATIONS,
    NOTIFICATION,
    CONSOLE_SHELL,
    CONSOLE_SIDEBAR,
    CONSOLE_TOPBAR,
    HINT_BAR,
    GAME_CURTAIN,
];

pub const CONSOLE_SHELL: Entry = Entry {
    parts: &[("main", "The column right of the sidebar.")],
    contains: &[
        "backdrop",
        "console-sidebar",
        "console-topbar",
        "view",
        "hint-bar",
    ],
    container: true,
    ..Entry::new(
        "console-shell",
        "ConsoleShell (apps/native/src/frame/console.rs)",
        Source::App,
        "Console mode's window (`[data-mode=\"console\"]` on the root): the console sidebar, top bar and view, and the button-hint bar. `data-cursor` is `hidden` while the pointer rests.",
    )
};

pub const CONSOLE_SIDEBAR: Entry = Entry {
    parts: &[
        ("brand", "The logo and name."),
        ("nav", "The links."),
        (
            "nav-item",
            "A link; the current one has `data-status=\"active\"`.",
        ),
        ("icon", "A link's icon."),
        ("label", "A link's text."),
        (
            "sub-nav",
            "The links under a link: the Library's filters, open (`data-open`) while the Library is.",
        ),
        (
            "sub-item",
            "A filter; the current one has `data-status=\"active\"`.",
        ),
        ("count", "How many games a filter has."),
        ("plugin-nav", "Plugin views that show in console mode."),
        ("leave", "Goes back to desktop mode."),
    ],
    slots: &["plugin-nav"],
    ..Entry::new(
        "console-sidebar",
        "ConsoleSidebar (apps/native/src/frame/console.rs)",
        Source::App,
        "Console mode's navigation: big targets.",
    )
};

pub const CONSOLE_TOPBAR: Entry = Entry {
    parts: &[
        ("actions", "The items at the end of the bar."),
        ("activity", "The link to Activity."),
        (
            "job-count",
            "The number of running jobs; `data-status` is `active` or `needs-user`.",
        ),
        (
            "notification-count",
            "The number of unread notifications, apart from the jobs.",
        ),
        (
            "avatar",
            "The signed-in account's button, which opens the account menu (an `avatar` inside).",
        ),
    ],
    contains: &["search", "select", "avatar"],
    ..Entry::new(
        "console-topbar",
        "ConsoleTopbar (apps/native/src/frame/console.rs)",
        Source::App,
        "Console mode's top bar: search and activity.",
    )
};

pub const HINT_BAR: Entry = Entry {
    parts: &[
        (
            "hint",
            "One button and what it does; `data-action` is the action (`confirm`, `back`, `tab.next`, …).",
        ),
        (
            "glyph",
            "The button's label; face buttons carry `data-tone` (`installed`, `danger`, `accent`, `warning`).",
        ),
        ("label", "What the button does."),
        ("clock", "The time."),
    ],
    ..Entry::new(
        "hint-bar",
        "HintBar (apps/native/src/console/hint_bar.rs)",
        Source::App,
        "What each controller button does where focus is. `data-glyphs` is the controller family in hand: `xbox`, `playstation`, `nintendo`, `generic`, or `keyboard`.",
    )
};

pub const GAME_CURTAIN: Entry = Entry {
    parts: &[
        ("title", "The game's title."),
        (
            "hint",
            "The guide button, which opens the quick menu over it.",
        ),
    ],
    states: &[("data-state", &["covered", "open"])],
    ..Entry::new(
        "game-curtain",
        "GameCurtain (apps/native/src/frame/console.rs)",
        Source::App,
        "Console mode's black screen behind a running game: it fades in when a game starts, so the game's window takes over without Caskade minimizing, and fades out when the last game exits. Always present in console mode; `data-state` is `covered` while it is down.",
    )
};

pub const APP_SHELL: Entry = Entry {
    parts: &[("main", "The column right of the sidebar.")],
    contains: &["backdrop", "sidebar", "topbar", "view", "status-bar"],
    container: true,
    ..Entry::new(
        "app-shell",
        "Frame (apps/native/src/frame/mod.rs)",
        Source::App,
        "The window: sidebar, topbar, current view and persistent status bar.",
    )
};

pub const BACKDROP: Entry = Entry {
    ..Entry::new(
        "backdrop",
        "Frame (apps/native/src/frame/mod.rs)",
        Source::App,
        "The decorative layer behind everything.",
    )
};

pub const SIDEBAR: Entry = Entry {
    parts: &[
        ("brand", "The logo and name."),
        ("nav", "A group of links."),
        (
            "nav-item",
            "A link; the current one has `data-status=\"active\"`.",
        ),
        ("icon", "A link's icon."),
        ("label", "A link's text."),
        ("divider", "The rule between the groups."),
        (
            "sub-nav",
            "The links under a link: the Library's filters, open (`data-open`) while the Library is.",
        ),
        (
            "sub-item",
            "A filter; the current one has `data-status=\"active\"`.",
        ),
        ("count", "How many games a filter has."),
        ("plugin-nav", "The links to plugin views."),
    ],
    slots: &["plugin-nav"],
    ..Entry::new(
        "sidebar",
        "Sidebar (apps/native/src/frame/sidebar.rs)",
        Source::App,
        "The navigation column.",
    )
};

pub const TOPBAR: Entry = Entry {
    parts: &[
        ("actions", "The items at the end of the bar."),
        ("activity", "The link to Activity."),
        (
            "job-count",
            "The number of running jobs; `data-status` is `active` or `needs-user`.",
        ),
        (
            "notification-count",
            "The number of unread notifications, apart from the jobs.",
        ),
        (
            "avatar",
            "The signed-in account's button, which opens the account menu (an `avatar` inside).",
        ),
        (
            "console",
            "Enters console mode, where Caskade draws no window controls (macOS, the browser preview).",
        ),
    ],
    contains: &["search", "window-controls", "avatar"],
    slots: &["actions"],
    ..Entry::new(
        "topbar",
        "Topbar (apps/native/src/frame/topbar.rs)",
        Source::App,
        "The bar above the view: search, quick actions and the window controls. It is the window's title bar: dragging it moves the window.",
    )
};

pub const WINDOW_CONTROLS: Entry = Entry {
    parts: &[
        ("minimize", "Minimizes the window."),
        (
            "console",
            "Enters console mode (fullscreen, for a controller).",
        ),
        (
            "maximize",
            "Maximizes the window, or restores it when maximized.",
        ),
        ("close", "Closes the window."),
    ],
    states: &[("data-status", &["restored", "maximized"])],
    ..Entry::new(
        "window-controls",
        "WindowControls (apps/native/src/frame/window.rs)",
        Source::App,
        "Minimize, maximize and close at the end of the topbar (Windows and Linux; macOS keeps its traffic lights). Each button carries `data-hovered` and `data-pressed`.",
    )
};

pub const VIEW: Entry = Entry {
    container: true,
    ..Entry::new(
        "view",
        "Frame (apps/native/src/frame/mod.rs)",
        Source::App,
        "The scrolling area that shows the current page.",
    )
};

pub const STATUS_BAR: Entry = Entry {
    parts: &[
        ("spinner", "The activity spinner; hidden while idle."),
        (
            "activity",
            "A concise description of current work or queue status.",
        ),
        ("version", "The application version."),
        (
            "update",
            "A link to Settings → General once a check finds a new Caskade version; `data-status` is `available`, or `ready` once it is installed and waits for a restart.",
        ),
        (
            "sync",
            "With paired devices (#231): how many are in reach and the best way one is reached (a `peer-link`), linked to Settings → Devices; `data-status` is `lan`, `direct`, `relay`, `offline` or `syncing`.",
        ),
    ],
    ..Entry::new(
        "status-bar",
        "StatusBar (apps/native/src/frame/status_bar.rs)",
        Source::App,
        "Persistent background activity, application version and an available Caskade update. IGDB and Hasheous are credited in Settings → About (`about`).",
    )
};

pub const VIEW_ERROR: Entry = Entry {
    parts: &[
        ("title", "The heading."),
        ("message", "The error's text."),
        ("actions", "Try again and Open the library, or Reload."),
    ],
    ..Entry::new(
        "view-error",
        "ViewError, AppCrash (apps/native/src/frame/error.rs)",
        Source::App,
        "Shown in place of a view that failed to render, with the shell around it; or in place of the whole window when the shell itself failed, with the window controls.",
    )
};

pub const SEARCH: Entry = Entry {
    parts: &[
        (
            "control",
            "The box around the input; carries `data-focus-within`.",
        ),
        ("icon", "The magnifier."),
        ("input", "The `<input>`."),
        (
            "shortcut",
            "The shortcut that focuses it: Ctrl K, or in console mode the controller's search button.",
        ),
    ],
    interaction: &["data-open", "data-disabled"],
    contains: &["search-popover"],
    ..Entry::new(
        "search",
        "SearchBox (apps/native/src/frame/search.rs)",
        Source::App,
        "The game search; suggestions open in a `search-popover`.",
    )
};

pub const SEARCH_POPOVER: Entry = Entry {
    parts: &[
        ("suggestion", "A suggested game; carries `data-focused`."),
        (
            "see-all",
            "The item that opens every result in the library.",
        ),
    ],
    interaction: &["data-placement", "data-entering", "data-exiting"],
    contains: &["game-art", "status-badge"],
    ..Entry::new(
        "search-popover",
        "SearchBox (apps/native/src/frame/search.rs)",
        Source::App,
        "The search suggestions, rendered in a portal.",
    )
};

pub const TYPE_SEARCH: Entry = Entry {
    parts: &[
        ("label", "The “Search” label."),
        ("input", "The `<input>` holding the typed text."),
        ("count", "How many games match."),
        (
            "hints",
            "The keys for the results, cancelling and deleting.",
        ),
    ],
    ..Entry::new(
        "type-search",
        "TypeSearch (apps/native/src/frame/type_search.rs)",
        Source::App,
        "Search from a keyboard, RetroArch style: a centred overlay with the text being typed and how many games match, opened by typing anywhere outside a text field or by `/`.",
    )
};

pub const NOTIFICATIONS: Entry = Entry {
    parts: &[
        ("toolbar", "The filters and Mark all read."),
        (
            "filter",
            "All or Unread; the current one has `data-selected`.",
        ),
        ("mark-all", "Marks read every item shown so far."),
        (
            "empty",
            "Shown when the feed (or its unread filter) is empty.",
        ),
        (
            "pages",
            "Newer and Older, when the feed has more than a page.",
        ),
    ],
    contains: &["notification", "button"],
    ..Entry::new(
        "notifications",
        "Feed (apps/native/src/frame/notifications.rs)",
        Source::App,
        "Activity's notification feed (#198): game additions in hourly batches, failed jobs, updates on offer and plugins turned off, newest change first.",
    )
};

pub const NOTIFICATION: Entry = Entry {
    parts: &[
        ("indicator", "The unread dot."),
        ("meta", "Its category and time."),
        (
            "time",
            "When: a batch's first to last addition, else its last change.",
        ),
        ("title", "The heading."),
        ("body", "Details: a batch's sources, a failure's error."),
        (
            "actions",
            "Open (the list, the game or the settings it is about) and Mark read or unread.",
        ),
        (
            "members",
            "What it is about, once opened: a batch's games, the updates on offer.",
        ),
        (
            "member",
            "One of them; `data-status` is `present`, `merged` (it links to the game it joined), `removed` or `other`.",
        ),
    ],
    states: &[
        ("data-status", &["unread", "read"]),
        ("data-severity", &["info", "warning", "error"]),
        (
            "data-category",
            &[
                "gamesAdded",
                "jobFailed",
                "updatesAvailable",
                "pluginDisabled",
                "achievementsUnlocked",
            ],
        ),
        ("data-resolved", &["true"]),
    ],
    contains: &["button"],
    ..Entry::new(
        "notification",
        "Item (apps/native/src/frame/notifications.rs)",
        Source::App,
        "One item of the notification feed. `data-category` is what it is about; `data-resolved` is set once its problem has cleared.",
    )
};
