//! The game page's themeable roots (#100), word for word the webview's
//! (`packages/ui/src/anatomy.ts`) apart from the component each names.

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root the game page renders.
pub const ANATOMY: &[Entry] = &[SCREENSHOT_VIEWER, VIDEOS];

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
