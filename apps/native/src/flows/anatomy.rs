//! The flows' catalogued roots (#108): each matches the webview's entry
//! (`packages/ui/src/anatomy.ts`) word for word, apart from the
//! component it names.

use caskade_ui_theme::anatomy::{Entry, Source};

pub const SHARE_GAME: Entry = Entry {
    parts: &[
        ("link", "The share link, selectable."),
        ("qr", "The link as a QR code, in console mode."),
        ("state", "What the share is doing."),
        ("recipients", "Who opened the link."),
    ],
    contains: &["share-recipient"],
    ..Entry::new(
        "share-game",
        "ShareGameDialog (apps/native/src/flows/share_game.rs)",
        Source::App,
        "Share with someone… (#247): how long the link works, then the link with Copy (and, in console mode, a QR code), what the share is doing, who received it, and Stop sharing.",
    )
};

pub const RECEIVE_SHARE: Entry = Entry {
    contains: &["share-info"],
    ..Entry::new(
        "receive-share",
        "ReceiveShareDialog (apps/native/src/flows/receive_share.rs)",
        Source::App,
        "Add game → Receive a shared game… (#247): the link, what its sharer says it shares, Help others receive it, and Receive.",
    )
};

pub const SHARE_INFO: Entry = Entry {
    parts: &[
        (
            "where",
            "Where a save, web game or ROM goes on this device.",
        ),
        (
            "games",
            "A shared collection's games, each with whether it's here (`data-status`).",
        ),
    ],
    states: &[("data-status", &["direct", "relayOnly"])],
    ..Entry::new(
        "share-info",
        "ShareInfoDetails (apps/native/src/flows/shares_common.rs)",
        Source::App,
        "What a share link shares, as its sharer says (#247, #248): who, what, its size and build, until when the link works; for a save, web game or ROM, where it goes here; for a collection, which of its games this device has. `data-status` is `direct`, or `relayOnly` when only a relay reaches the sharer and files wait for a direct connection.",
    )
};

pub const SHARE_RECIPIENT: Entry = Entry {
    states: &[("data-status", &["receiving", "away", "done"])],
    ..Entry::new(
        "share-recipient",
        "ShareRecipients (apps/native/src/flows/shares_common.rs)",
        Source::App,
        "Someone who opened a share's link (#247): their device's name, whether they help others receive it, and a bar of how far they got. `data-status` is `receiving`, `away` (not connected now) or `done`.",
    )
};

pub const PAIR_DEVICE: Entry = Entry {
    parts: &[
        (
            "state",
            "A code pairing; `data-status` is `waiting`, `paired`, `cancelled` or `failed`.",
        ),
        (
            "code",
            "The six-digit code, split in two; larger in console mode.",
        ),
        (
            "link",
            "An opened pairing link; `data-status` is `valid` or `expired`.",
        ),
    ],
    contains: &["button"],
    ..Entry::new(
        "pair-device",
        "PairingHost (apps/native/src/flows/pairing.rs)",
        Source::App,
        "Pairing over any view (#226, #231): the code both devices show, for a pairing this device or the other one started; or a `caskade://pair` link opened from outside, waiting for an admin.",
    )
};

pub const SHARE: Entry = Entry {
    parts: &[
        (
            "state",
            "What the share is doing: reading its files, until when the link works, or that it stopped.",
        ),
        (
            "recipients",
            "Who opened the link, each with how far they got.",
        ),
    ],
    states: &[(
        "data-status",
        &["preparing", "ready", "failed", "stopped", "expired"],
    )],
    contains: &["share-recipient"],
    ..Entry::new(
        "share",
        "SharesList (apps/native/src/flows/shares_list.rs)",
        Source::App,
        "A game this device shared once with someone else (#246, #247): its title and size, Copy link and Stop sharing while it runs (Forget after), what it is doing and who received it. `data-status` is `preparing` (its files are being read; the link works already), `ready`, `failed`, `stopped` or `expired`.",
    )
};

pub const RECEIVED_SHARE: Entry = Entry {
    states: &[("data-status", &["receiving", "serving", "done"])],
    ..Entry::new(
        "received-share",
        "SharesList (apps/native/src/flows/shares_list.rs)",
        Source::App,
        "A game someone shared with this device (#249): its title and sharer, and the Help others receive it switch. `data-status` is `receiving`, `serving` (it helps the share's other recipients) or `done`.",
    )
};

pub const TOOL_FAILED: Entry = Entry {
    contains: &["button"],
    ..Entry::new(
        "tool-failed",
        "ToolRequest (apps/native/src/flows/tool_request.rs)",
        Source::App,
        "Says why a game's tool didn't start.",
    )
};

pub const SCRIPTS: Entry = Entry {
    parts: &[
        ("items", "The scope's scripts (a listbox)."),
        (
            "script",
            "One script; `data-enabled` is `false` for one that's off, `data-selected` on the one in the form.",
        ),
        ("form", "The chosen script's fields."),
        (
            "path",
            "A path with Browse… (the program, the working folder); `data-drop-active` while a dropped file is over it.",
        ),
        ("list", "A program's arguments, one field each."),
        ("remove", "Removes one argument (a `button`)."),
        ("error", "Why the last save, order or delete was refused."),
        (
            "delete",
            "Deletes the script; pressed once more to confirm (a `button`).",
        ),
        ("test", "Test and Cancel test, and the last test's result."),
        (
            "result",
            "A test's outcome; `data-status` is how it ended (`succeeded`, `failed`, `timed_out`, `cancelled`, `not_started`).",
        ),
        ("output", "A test's standard output or errors."),
        ("context", "The values the test saw."),
        (
            "order",
            "The order a launch runs scripts in, every scope's.",
        ),
        (
            "step",
            "One step; `data-inherited` when it comes from another scope, `data-enabled` `false` when it's off.",
        ),
    ],
    contains: &["button", "select", "segmented", "switch", "text-field"],
    ..Entry::new(
        "scripts",
        "ScriptsPanel (apps/native/src/flows/scripts.rs)",
        Source::App,
        "One scope's lifecycle scripts (#180), in Settings → Scripts, an emulator profile and the game editor: the scripts, the chosen one's form, its test and the order a launch runs every scope's scripts in.",
    )
};

pub const ABOUT: Entry = Entry {
    parts: &[
        ("title", "The page heading."),
        (
            "credits",
            "The list of data credits, directly under the heading.",
        ),
        (
            "credit",
            "One credit (IGDB, Hasheous): a link to the service, each with the same weight.",
        ),
        ("details", "The app's name, logo and short description."),
        (
            "version",
            "The version and build, from the app's build metadata.",
        ),
        (
            "links",
            "Links to the project page, issue tracker and releases.",
        ),
        ("legal-links", "Links to each section of the Legal view."),
    ],
    ..Entry::new(
        "about",
        "AboutPage (apps/native/src/settings/about.rs)",
        Source::App,
        "Settings → About: the IGDB and Hasheous credits first, side by side, then the app, its version and build, project links, and the links into `legal`.",
    )
};

pub const LEGAL: Entry = Entry {
    parts: &[
        ("back", "The link back to About."),
        ("title", "The view's heading."),
        ("nav", "The section links."),
        (
            "section-link",
            "A section; the open one carries `aria-current`.",
        ),
        (
            "document",
            "A long text (a policy, a licence group's packages and texts): focusable, so keys and a pad scroll it.",
        ),
    ],
    ..Entry::new(
        "legal",
        "LegalView (apps/native/src/settings/about.rs)",
        Source::App,
        "Settings → About → Legal: licences, open-source packages, terms and privacy policy, bundled for offline reading. One section shows at a time, at `/settings?tab=about&section=<id>`.",
    )
};

pub const SYSTEM_STATS: Entry = Entry {
    parts: &[
        ("title", "The section heading."),
        (
            "status",
            "The line under it: live, waiting, not updating or unavailable, in words.",
        ),
        (
            "card",
            "One metric (`data-metric`: `cpu`, `memory`, `gpu`, `network`, `storage`); focusable, so keys and a pad reach every card.",
        ),
        (
            "gauge",
            "A utilization gauge (`role=meter`) with its percentage written inside.",
        ),
        (
            "graph",
            "A recent-history graph; a missed sample is a gap in its line.",
        ),
        (
            "unavailable",
            "Why a metric has no value: unsupported, no device or a failed sample.",
        ),
        (
            "interface",
            "A network interface and its rates; `data-kind` is `physical`, `virtual` or `loopback`.",
        ),
        (
            "volume",
            "A storage volume: its name, mount points and capacity.",
        ),
        ("capacity", "A volume's used-space bar (`role=meter`)."),
    ],
    states: &[("data-status", &["loading", "live", "stale", "error"])],
    ..Entry::new(
        "system-stats",
        "StatisticsDashboard (apps/native/src/settings/statistics.rs)",
        Source::App,
        "Settings → Statistics: this machine's CPU, GPUs, memory, network and storage capacity, live, as the same cards in desktop and console modes. `data-status` says whether readings are coming in.",
    )
};

pub const PEER_LINK: Entry = Entry {
    states: &[("data-status", &["lan", "direct", "relay", "offline"])],
    ..Entry::new(
        "peer-link",
        "LinkBadge (apps/native/src/settings/devices.rs)",
        Source::App,
        "How another device is reached (#225, #231): an icon and a word. `data-status` is `lan` (on this network), `direct` (over the internet), `relay` (through a relay) or `offline`.",
    )
};

pub const PAIRED_DEVICE: Entry = Entry {
    parts: &[
        (
            "sync",
            "Its OS and the last sync, or that it is syncing now.",
        ),
        ("sync-error", "Shown when the last sync didn't reach it."),
    ],
    states: &[("data-status", &["lan", "direct", "relay", "offline"])],
    contains: &["peer-link", "button"],
    ..Entry::new(
        "paired-device",
        "DeviceRow in Settings → Devices (apps/native/src/settings/devices.rs)",
        Source::App,
        "A device paired with this one: its icon, name, how it is reached, its last sync, Sync now and (for an admin) Forget. `data-status` is how it is reached.",
    )
};

pub const TRANSFER: Entry = Entry {
    states: &[(
        "data-status",
        &["running", "waitingDirect", "done", "failed"],
    )],
    contains: &["peer-link"],
    ..Entry::new(
        "transfer",
        "CopiesAndShares in Settings → Devices (apps/native/src/settings/devices.rs)",
        Source::App,
        "A game copied between two of the user's devices (#233): its title, the other device, how it is reached and its progress. `data-status` is `running`, `waitingDirect` (game files wait for a direct path), `done` or `failed`.",
    )
};

pub const PIN_PAD: Entry = Entry {
    parts: &[
        ("title", "What the PIN is for."),
        ("field", "The PIN, as dots."),
        ("keys", "The number pad."),
        ("key", "A digit."),
        ("delete", "Deletes the last digit."),
        ("submit", "Confirms the PIN."),
        ("message", "Why the PIN was refused, or how long to wait."),
    ],
    states: &[("data-status", &["ready", "wrong", "locked"])],
    ..Entry::new(
        "pin-pad",
        "PinPad (apps/native/src/flows/pin_pad.rs)",
        Source::App,
        "PIN entry: a field for keys and a number pad for a controller (A types a digit, B deletes, Start confirms). A wrong PIN shakes the field and clears it; after too many, `locked` counts the wait down.",
    )
};

pub const ACCOUNT_LIST: Entry = Entry {
    parts: &[
        ("account", "One account's row."),
        ("avatar", "Its picture (an `avatar`)."),
        ("name", "Its name; the signed-in account's says so."),
        (
            "details",
            "Its role, whether a PIN locks it, and what it may do.",
        ),
        ("edit", "Opens the `account-editor` (a `button`)."),
        (
            "delete",
            "Asks before deleting the account and its collections (a `button`).",
        ),
    ],
    contains: &["avatar", "button"],
    ..Entry::new(
        "account-list",
        "AccountsSettings (apps/native/src/settings/accounts.rs)",
        Source::App,
        "Settings → Accounts, for admins: every account, with Edit and Delete.",
    )
};

pub const ACCOUNT_EDITOR: Entry = Entry {
    parts: &[
        (
            "avatars",
            "The avatar choice (an `avatar-picker`) and its label.",
        ),
        (
            "access",
            "A standard account's permissions: Install games, one switch per Settings section, the maximum age rating and unrated games.",
        ),
    ],
    contains: &[
        "text-field",
        "avatar-picker",
        "setting-row",
        "segmented",
        "switch",
        "select",
        "button",
    ],
    ..Entry::new(
        "account-editor",
        "AccountEditor (apps/native/src/settings/accounts.rs)",
        Source::App,
        "The dialog that adds or changes an account: its name, avatar, role and PIN, and for a standard account what it may change and its age limit.",
    )
};

pub const ACCOUNT_PROFILE: Entry = Entry {
    parts: &[
        (
            "avatars",
            "The avatar choice (an `avatar-picker`) and its label.",
        ),
        (
            "pin",
            "The PIN's row: set, change or remove it, after the current PIN.",
        ),
    ],
    contains: &["text-field", "avatar-picker", "setting-row", "button"],
    ..Entry::new(
        "account-profile",
        "ProfileSettings (apps/native/src/settings/accounts.rs)",
        Source::App,
        "Settings → My profile: the signed-in account's own name, avatar and PIN.",
    )
};

pub const ROMM: Entry = Entry {
    contains: &["settings-section", "romm-servers"],
    container: true,
    ..Entry::new(
        "romm",
        "RommPanel (apps/native/src/settings/emulation/romm.rs)",
        Source::App,
        "Settings → RomM servers: the server list, the selected server's editor, downloads and the cache.",
    )
};

pub const ROMM_SERVERS: Entry = Entry {
    parts: &[
        (
            "romm-server",
            "A server's row (`data-status`: `untested`, `connected`, `offline`, `error` or `sign_in_needed`; `data-selected` while its editor is open).",
        ),
        (
            "sign-in",
            "Sign in or Enter API token on a server this device can't sign in to: opens its editor at the password or token (a `button`).",
        ),
        (
            "sign-in-reason",
            "Why a server needs signing in on this device: sign-ins stay on each device, or the server refused the saved one.",
        ),
    ],
    contains: &["button", "status-dot"],
    ..Entry::new(
        "romm-servers",
        "RommPanel (apps/native/src/settings/emulation/romm.rs)",
        Source::App,
        "The RomM server list: each server's state, games, last sync and its action (Sync now, or Sign in when this device can't sign in).",
    )
};
