//! Activity's roots (#101) in the catalogue: the details refresh's card, a
//! job's card, its question and the install confirmation.

use caskade_ui_theme::anatomy::{Entry, Source};

pub const ANATOMY: [Entry; 4] = [
    Entry {
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
    },
    ACTIVITY_JOB,
    JOB_QUESTION,
    INSTALL_CONFIRMATION,
];

pub const ACTIVITY_JOB: Entry = Entry {
    parts: &[
        (
            "progress",
            "The progress bar's fill, as wide as the job is through; a quarter while it can't tell.",
        ),
        (
            "failure",
            "Why a failed install or update stopped (#158): the stage and file, what happens next, the recovery buttons and the technical details. `data-kind` is the cause (`missingSource`, `missingStaged`, `missingOutput`, `accessDenied`, `locked`, `integrity`, `storageDisconnected`, `diskFull`, `download`, `unsupported`, `other`); `data-status` is `retrying` while Caskade will try again on its own, else `waiting`.",
        ),
        (
            "strategy",
            "An install way the installer weighed, and why; `data-selected` on the one it chose. After a failure it has Retry with it (a `button`).",
        ),
    ],
    states: &[(
        "data-status",
        &[
            "queued",
            "running",
            "needs_user",
            "paused",
            "completed",
            "failed",
            "cancelled",
        ],
    )],
    contains: &["menu", "button", "install-root-picker"],
    ..Entry::new(
        "activity-job",
        "JobCard (apps/native/src/activity/card.rs)",
        Source::App,
        "A job in Activity (#101): its kind, title and status; Review choices, Pause and Resume a download (#131), Cancel job, Retry and Remove; its progress; why a failed install stopped and what can be done; verification results, warnings, dependencies and the install ways under Advanced strategies. A right click offers its buttons as a menu. `data-status` is `queued`, `running` (downloading or installing), `needs_user` (a question waits), `paused`, `completed`, `failed` or `cancelled`.",
    )
};

pub const JOB_QUESTION: Entry = Entry {
    contains: &["button"],
    ..Entry::new(
        "job-question",
        "QuestionDialog (apps/native/src/activity/question.rs)",
        Source::App,
        "A job's question (#101), over whatever view is shown when the user follows the job (Review choices, a retry, a confirmed install): what it asks, its choices as radio buttons, Not now (the job waits in Activity) and Continue.",
    )
};

pub const INSTALL_CONFIRMATION: Entry = Entry {
    parts: &[
        (
            "share",
            "A share link's details (a `share-info`) and where the game comes from.",
        ),
        (
            "peer",
            "The game another of the user's devices sends, and its size.",
        ),
        (
            "plan",
            "What a dropped file or folder installs and launches.",
        ),
        (
            "source",
            "The URL or path an external link asked to install from.",
        ),
        ("dismiss", "Dismisses the request (a `button`)."),
        (
            "confirm",
            "Install, Receive, Copy here or Add to my collections (a `button`).",
        ),
    ],
    states: &[("data-kind", &["path", "url", "peer", "share"])],
    contains: &["share-info", "button"],
    ..Entry::new(
        "install-confirmation",
        "InstallConfirmation (apps/native/src/activity/ingress.rs)",
        Source::App,
        "An install request waiting over any view (#101): a deep link, a file dropped outside the library, a pasted link or path (#132), a share link or a game another of the user's devices sends (#235). Nothing downloads or is written until it's confirmed; closing dismisses it. `data-kind` is what it installs from: `path`, `url`, `peer` or `share`.",
    )
};
