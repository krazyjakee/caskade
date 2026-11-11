//! Activity's roots (#101) in the catalogue: the details refresh's card,
//! as the webview's `RefreshCard` entry. A job's card, its question and the
//! install confirmation aren't in the webview's catalogue either
//! (`anatomy.rs`' `NOT_YET`).

use caskade_ui_theme::anatomy::{Entry, Source};

pub const ANATOMY: [Entry; 1] = [Entry {
    parts: &[
        ("title", "What the refresh is doing."),
        (
            "mode",
            "Missing details only, or automatic details refreshed.",
        ),
        ("progress", "How many games are through."),
        (
            "counts",
            "Updated, unchanged, not found on IGDB, skipped and failed games.",
        ),
        ("cancel", "Stops after the current batch (a `button`)."),
        (
            "retry",
            "Refreshes the games that failed again (a `button`).",
        ),
        (
            "continue",
            "Refreshes the games a cancelled refresh left (a `button`).",
        ),
    ],
    states: &[("data-status", &["running", "done", "cancelled"])],
    contains: &["button"],
    ..Entry::new(
        "games-refresh",
        "RefreshCard (apps/native/src/activity/refresh.rs)",
        Source::App,
        "The refresh of chosen games' details in Activity: progress, counts, Cancel, and retrying failed games or continuing a cancelled refresh.",
    )
}];
