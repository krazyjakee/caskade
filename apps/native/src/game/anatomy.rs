//! The game page's themeable roots (#100), word for word the webview's
//! (`packages/ui/src/anatomy.ts`) apart from the component each names.

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root the game page renders.
pub const ANATOMY: &[Entry] = &[SCREENSHOT_VIEWER, VIDEOS, ACHIEVEMENTS, ACHIEVEMENT_SUMMARY];

pub const SCREENSHOT_VIEWER: Entry = Entry {
    parts: &[
        ("title", "The game's title."),
        ("counter", "Which screenshot of how many."),
        ("close", "Closes the gallery."),
        (
            "stage",
            "The area the screenshot fills; a swipe on it steps.",
        ),
        ("image", "The screenshot, full size."),
        ("prev", "Steps to the previous screenshot."),
        ("next", "Steps to the next screenshot."),
        ("strip", "Every screenshot, small, to jump to one."),
        (
            "thumb",
            "One screenshot in the strip; `data-selected` on the one shown.",
        ),
    ],
    ..Entry::new(
        "screenshot-viewer",
        "ScreenshotViewer (apps/native/src/game/media.rs)",
        Source::App,
        "A game's screenshots one at a time, filling the window, opened from its page; arrows, a swipe, a pad's left/right or LB/RB step through them.",
    )
};

pub const VIDEOS: Entry = Entry {
    parts: &[
        ("items", "The row of videos."),
        ("video", "One video's button: its thumbnail and name."),
        ("thumbnail", "The video's thumbnail."),
        ("play", "The play mark over the thumbnail."),
        ("name", "The video's name."),
    ],
    interaction: &["data-hovered", "data-focus-visible", "data-pressed"],
    ..Entry::new(
        "videos",
        "Videos (apps/native/src/game/media.rs)",
        Source::App,
        "A game's videos (IGDB's trailers) in a row on its page and its Store page; each plays in the video window.",
    )
};

pub const ACHIEVEMENTS: Entry = Entry {
    parts: &[
        (
            "refresh",
            "Refresh achievements: fetches the game's sets now (a `button`).",
        ),
        ("cancel", "Stops a refresh under way (a `button`)."),
        (
            "sets",
            "Which store account's set is shown, when there are several (a `segmented`).",
        ),
        (
            "account",
            "The set's service and account, and when it was last fetched.",
        ),
        (
            "stale",
            "The last refresh failed: what is shown is the last answer, and why.",
        ),
        ("wait", "The service asked Caskade to wait, and until when."),
        (
            "notice",
            "Not fetched yet, no achievements, not offered, private, or signed out, with what to do.",
        ),
        (
            "reconnect",
            "Opens Settings → Store accounts to sign in again (a `button`).",
        ),
        (
            "summary",
            "Unlocked of total, hardcore unlocks and points, with a meter.",
        ),
        ("filter", "All, unlocked or locked (a `segmented`)."),
        ("reveal-all", "Shows every hidden achievement (a `switch`)."),
        ("list", "The achievements, in the service's order."),
        (
            "achievement",
            "One achievement (`data-state`: `unlocked` or `locked`; `data-concealed` while a hidden one is concealed; `data-hardcore` when unlocked in hardcore).",
        ),
        (
            "icon",
            "Its icon from the service, or a drawn trophy, lock or concealed mark.",
        ),
        ("details", "Unlock time, hardcore, rarity and points."),
        (
            "hardcore",
            "Its hardcore unlock, or that it was unlocked in softcore only.",
        ),
        (
            "reveal",
            "Shows a hidden achievement's name and description (a `button`).",
        ),
    ],
    states: &[(
        "data-state",
        &[
            "loading",
            "error",
            "not_fetched",
            "ready",
            "no_achievements",
            "unsupported",
            "private",
            "sign_in_needed",
        ],
    )],
    contains: &["button", "segmented", "switch"],
    ..Entry::new(
        "achievements",
        "GameAchievements (apps/native/src/game/achievements.rs)",
        Source::App,
        "A game page's achievements: one store account's set at a time, its counts and how its last fetch went, and every achievement with its state, progress, unlock time, rarity and points where the service gives them. Hidden achievements stay concealed until unlocked or revealed.",
    )
};

pub const ACHIEVEMENT_SUMMARY: Entry = Entry::new(
    "achievement-summary",
    "AchievementSummary (apps/native/src/game/achievements.rs)",
    Source::App,
    "A game's achievement counts in its hero, across its sets; pressing it moves to the achievements.",
);
