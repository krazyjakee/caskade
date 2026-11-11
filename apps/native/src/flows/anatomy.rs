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
