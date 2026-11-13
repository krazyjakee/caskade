//! The game page's themeable roots (#100).

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root the game page renders.
pub const ANATOMY: &[Entry] = &[
    SCREENSHOT_VIEWER,
    VIDEOS,
    ACHIEVEMENTS,
    ACHIEVEMENT_SUMMARY,
    GAME_EDITOR,
    GAME_EDITOR_DISCARD,
    GAME_ACTIONS,
    GAME_SAVES,
    PLAY_CHOICE,
    EMULATION,
    SAVES_PANEL,
    UPDATE_OFFER,
    PLAYING_ELSEWHERE,
    SAVE_CONFLICT,
    SAVE_ARRIVING,
];

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

pub const GAME_EDITOR: Entry = Entry {
    parts: &[
        ("title", "The editor's heading."),
        ("tabs", "The editor's sections: Details, Actions, Scripts."),
        (
            "tab",
            "One section; `data-selected` on the open one, `data-dirty` while it holds unsaved changes.",
        ),
        ("details", "The Details form."),
        (
            "field",
            "One field, with `data-field` naming it (`title`, `description`, `tags`, …), `data-manual` when its value is the user's and refreshes keep it, and `data-invalid` when the last save was refused for it.",
        ),
        ("label", "A field's name."),
        ("manual", "Marks a value the user set."),
        (
            "reset",
            "Puts a field or an artwork kind back to its automatic value (a `button`).",
        ),
        (
            "error",
            "Why a field wasn't saved, with the conflict choices.",
        ),
        (
            "use-theirs",
            "Takes the value saved meanwhile (a `button`).",
        ),
        (
            "keep-mine",
            "Keeps the user's value over the one saved meanwhile (a `button`).",
        ),
        ("list", "A list field's values and its add field."),
        ("chip", "One value of a list."),
        ("remove", "Removes a value from its list (a `button`)."),
        ("footer", "Status, Cancel and Save."),
        ("status", "Saving, or what went wrong."),
        (
            "choice",
            "Who decides an artwork kind's image: automatic, chosen by the user, or none.",
        ),
        ("artwork", "The Artwork tab."),
        (
            "slot",
            "One kind of artwork (`data-kind`: `cover`, `hero`, `icon`; `data-choice`: `automatic`, `chosen`, `removed`; `data-drop-active` while an image is dragged over it).",
        ),
        ("preview", "The image that kind shows now."),
        (
            "candidate",
            "An image a source supplied, to choose (a `button`; `data-selected` on the one shown).",
        ),
        ("import", "Imports an image file (a `button`)."),
        ("clear", "Shows no image of that kind (a `button`)."),
        (
            "cancel",
            "Closes the editor, asking first when there are unsaved changes (a `button`).",
        ),
        ("save", "Saves the Details (a `button`)."),
    ],
    contains: &["button", "text-field", "checkbox", "slider"],
    ..Entry::new(
        "game-editor",
        "GameEditor (apps/native/src/game/editor/mod.rs)",
        Source::App,
        "A game's editor, from its menu's Edit… or its page's Edit details: its details as one draft saved together, its artwork, and tabs for its launch actions and scripts. A dialog in desktop mode, the whole window in console mode.",
    )
};

pub const GAME_EDITOR_DISCARD: Entry = Entry {
    contains: &["button"],
    ..Entry::new(
        "game-editor-discard",
        "GameEditor (apps/native/src/game/editor/mod.rs)",
        Source::App,
        "Asks before an editor with unsaved changes closes: Keep editing or Discard.",
    )
};

pub const GAME_ACTIONS: Entry = Entry {
    parts: &[
        (
            "default",
            "What Play starts: one of the play actions, or Ask each time.",
        ),
        ("actions", "The release's actions (a listbox)."),
        (
            "action",
            "One action; `data-kind` is `play` or `utility`, `data-selected` on the one in the form.",
        ),
        ("form", "The chosen action's fields."),
        (
            "locked",
            "Says the release's own launch comes from its source and how to change it.",
        ),
        (
            "target-kind",
            "What an emulator, web or plugin action starts, which the form doesn't change.",
        ),
        (
            "path",
            "A path with Browse…; `data-drop-active` while a dropped file is over it.",
        ),
        (
            "list",
            "A list of values, one field each: arguments, program names.",
        ),
        (
            "env",
            "The environment variables, a name and a value per row.",
        ),
        ("remove", "Removes one value or variable (a `button`)."),
        (
            "proton",
            "Which Proton build a Windows program runs with: Automatic (the best installed) or one build.",
        ),
        (
            "tracking",
            "Which programs are the game, for a play action.",
        ),
        (
            "error",
            "Why the last save, order, delete or default change was refused.",
        ),
        (
            "delete",
            "Deletes the action; pressed once more to confirm (a `button`).",
        ),
    ],
    contains: &["button", "select", "segmented", "text-field"],
    ..Entry::new(
        "game-actions",
        "ActionsTab (apps/native/src/game/editor/actions.rs)",
        Source::App,
        "The game editor's Actions tab (#178): a release's launch actions, its own launch first, with the form for the one chosen and what Play starts.",
    )
};

pub const GAME_SAVES: Entry = Entry {
    parts: &[
        (
            "match",
            "What the locations come from; `data-match` is `unknown`, `matched` or `ambiguous`.",
        ),
        (
            "mode",
            "Says whether the automatic locations or the user's own are in use.",
        ),
        ("locations", "The save locations (a list)."),
        (
            "location",
            "One location; `data-kind` is `file`, `folder` or `registry`, with its file count.",
        ),
        ("path", "A location's path or registry key."),
        ("empty", "Shown when the release has no save locations."),
        ("warnings", "What couldn't be resolved on this device."),
        (
            "error",
            "Why the locations couldn't load or the last save was refused.",
        ),
    ],
    states: &[("data-choice", &["automatic", "chosen"])],
    contains: &["button", "select", "text-field"],
    ..Entry::new(
        "game-saves",
        "SavesTab (apps/native/src/game/editor/saves.rs)",
        Source::App,
        "The game editor's Saves tab (#238): one release's save locations on this device, automatic or the user's own, and the manifest game it matches. Files and folders dropped on it are added.",
    )
};

pub const PLAY_CHOICE: Entry = Entry {
    parts: &[
        (
            "play-starts",
            "Play starts: which play action Play starts, or Ask each time, when the release has more than one (a `select`'s trigger).",
        ),
        ("tools", "The release's tools, one button each."),
    ],
    contains: &["button", "select"],
    ..Entry::new(
        "play-choice",
        "PlayChoice (apps/native/src/game/play_choice.rs)",
        Source::App,
        "On a game's page, under Play: which of the release's play actions Play starts, and its tools (#178).",
    )
};

pub const EMULATION: Entry = Entry {
    parts: &[
        (
            "readiness",
            "What to set up before it can start, with Configure (to Settings → Emulators → Platforms).",
        ),
        (
            "launch-with",
            "Launch with: the platform's default, a compatible emulator profile, or Ask when launching (a `select`'s trigger).",
        ),
        (
            "args",
            "Extra arguments, added after the profile's (a `text-field`'s input).",
        ),
        ("save-args", "Saves the extra arguments (a `button`)."),
        (
            "disc",
            "One disc image or ROM file: present or missing, and its path.",
        ),
        (
            "bios",
            "One BIOS file the platform uses: found, wrong dump, missing or unknown, and whether it's optional.",
        ),
    ],
    states: &[(
        "data-status",
        &[
            "ready",
            "ask",
            "no_emulator",
            "profile_gone",
            "not_installed",
            "missing_files",
            "invalid",
            "missing_bios",
            "rom_missing",
            "not_emulated",
        ],
    )],
    contains: &["status-dot", "select", "text-field", "button"],
    ..Entry::new(
        "emulation",
        "EmulatedEntry (apps/native/src/game/emulation.rs)",
        Source::App,
        "An emulated release on a game's page, under Play from your library (#100): its platform and whether it can start, what to set up when it can't, the emulator and profile it launches with, extra arguments, and its discs and BIOS files. `data-status` is its readiness: `ready`, `ask` (it asks for an emulator when launching) or what stops it.",
    )
};

pub const SAVES_PANEL: Entry = Entry {
    parts: &[
        (
            "conflict",
            "Says the game was played on more than one device since they last shared saves.",
        ),
        (
            "choose",
            "Choose a save…: opens the choice between the devices' saves (a `button`).",
        ),
        ("locations", "Where the saves are on this device."),
        ("reveal", "Show in folder, for one location (a `button`)."),
        ("versions", "The versions Caskade kept, newest first."),
        (
            "version",
            "One version: when, the device and account that made it, why it was kept and its size; `data-status` is `current` (in use), `kept` or `arriving` (not on this device yet).",
        ),
        ("share", "Shares the version with someone (a `button`)."),
        (
            "restore",
            "Restores the version; the save in use is kept as a version (a `button`).",
        ),
    ],
    contains: &["button", "save-conflict", "share-game"],
    ..Entry::new(
        "saves-panel",
        "SavesPanel (apps/native/src/game/saves.rs)",
        Source::App,
        "A game's saves on its page (#100): the versions Caskade kept with Restore and Share…, the one in use marked; choosing between devices' saves when both played; and where the saves are on this device, with Show in folder.",
    )
};

pub const UPDATE_OFFER: Entry = Entry {
    parts: &[
        (
            "progress",
            "The update's progress once it started: updating, updated, or why it didn't finish.",
        ),
        ("skip", "Skip this version: stops offering it (a `button`)."),
        ("play-now", "Plays without updating (a `button`)."),
        (
            "update",
            "Update and play: installs the update, then plays once it has finished (a `button`).",
        ),
    ],
    contains: &["button"],
    ..Entry::new(
        "update-offer",
        "UpdateOffer (apps/native/src/game/launch.rs)",
        Source::App,
        "Update before playing? (#100): asked at Play when a newer version of the release is out; the version out and the one installed, What's new, then Skip this version, Play now, or Update and play. Once the update starts, its progress and Close.",
    )
};

pub const PLAYING_ELSEWHERE: Entry = Entry {
    parts: &[(
        "play-anyway",
        "Plays here as well; each device keeps its own save (a `button`).",
    )],
    contains: &["button"],
    ..Entry::new(
        "playing-elsewhere",
        "PlayingElsewhereDialog (apps/native/src/game/launch.rs)",
        Source::App,
        "Asked at Play when the game is running on another device (#100): which devices play it and since when, that each device keeps its own save if it's played here too, then Cancel or Play anyway.",
    )
};

pub const SAVE_CONFLICT: Entry = Entry {
    parts: &[
        (
            "versions",
            "The saves to choose from, newest first, one radio each: the device that made it, when, and its size.",
        ),
        (
            "choose",
            "Plays with the chosen save, or uses it from the Saves panel (a `button`).",
        ),
    ],
    contains: &["button"],
    ..Entry::new(
        "save-conflict",
        "SaveConflictDialog (apps/native/src/game/launch.rs)",
        Source::App,
        "Choose which save to play with (#100): the game was played on more than one device since they last shared saves. Asked at Play, and from the Saves panel's Choose a save…; this device's save is chosen to begin with, and the one not chosen is kept as a version.",
    )
};

pub const SAVE_ARRIVING: Entry = Entry {
    parts: &[(
        "skip",
        "Plays with the save on this device instead of waiting (a `button`).",
    )],
    contains: &["button"],
    ..Entry::new(
        "save-arriving",
        "WaitForSave (apps/native/src/game/launch.rs)",
        Source::App,
        "Shown at Play while another device's newer save is on its way (#100): the device it comes from, then Cancel or Skip.",
    )
};
