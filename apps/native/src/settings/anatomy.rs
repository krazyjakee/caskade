//! Settings panels' themeable roots (#103, #104, #105): the panels drawn
//! from a JSON Schema, Library Discovery, Storage, a plugin's review and
//! Streaming. The settings window itself is `settings::ANATOMY`.

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root the settings panels render of their own.
pub const PANELS: &[Entry] = &[
    SCHEMA_SETTINGS,
    DISCOVERY_SETTINGS,
    DISCOVERY_REVIEW,
    STORAGE_SETTINGS,
    PLUGIN_REVIEW,
    STREAMING_SETTINGS,
    STREAMING_EXISTING,
    STREAMING_CLIENT,
    STREAM_HOST,
    PAIR_DIALOG,
    STREAMING_PROGRAM,
];

pub const SCHEMA_SETTINGS: Entry = Entry {
    contains: &[
        "error-notice",
        "settings-section",
        "button",
        "setting-row",
        "select",
        "switch",
        "slider",
        "text-field",
    ],
    ..Entry::new(
        "schema-settings",
        "SchemaSettings (apps/native/src/settings/schema.rs)",
        Source::App,
        "A settings section drawn from its JSON Schema (#59, #128): Settings → General's Game updates, Settings → Installer and a plugin's settings in Settings → Plugins. Its reset button in the heading, then a row per field with a select, switch, slider or text field, as the field's type has it.",
    )
};

pub const DISCOVERY_SETTINGS: Entry = Entry {
    parts: &[
        (
            "discovery-options",
            "The scan, operating-system check and deep scan settings; `data-disabled` while discovery is off.",
        ),
        (
            "scan-paths",
            "The folders deep scans search, each with Remove.",
        ),
        (
            "source-reports",
            "What each source found in the last scan, and its problems.",
        ),
        ("hidden-games", "The games hidden from the library."),
        (
            "hidden-game",
            "One hidden game, with Show again; a right click offers it too.",
        ),
        (
            "ignored-results",
            "The results ignored before, each with Allow again.",
        ),
    ],
    contains: &[
        "error-notice",
        "button",
        "settings-section",
        "setting-row",
        "switch",
        "discovery-review",
        "checkbox",
        "menu",
        "select",
        "text-field",
    ],
    ..Entry::new(
        "discovery-settings",
        "LibraryDiscoverySettings (apps/native/src/settings/discovery.rs)",
        Source::App,
        "Settings → Library Discovery (#104): turning discovery on, Scan now and its progress; the results waiting for a decision; when scans run; the system's own records; deep scans and their folders; what each source last reported; hidden games shown again; and ignored results allowed again.",
    )
};

pub const DISCOVERY_REVIEW: Entry = Entry {
    parts: &[(
        "review-item",
        "One result: its checkbox, sources, location and evidence; a right click accepts or ignores it.",
    )],
    contains: &["checkbox", "button", "menu"],
    ..Entry::new(
        "discovery-review",
        "ReviewList (apps/native/src/settings/discovery.rs)",
        Source::App,
        "Review possible games, in Settings → Library Discovery: the results that need a decision, with Select all, Accept selected and Ignore selected. Accept adds a result to the library; Ignore keeps future scans from bringing it back.",
    )
};

pub const STORAGE_SETTINGS: Entry = Entry {
    parts: &[
        (
            "root",
            "One location's card, which selects it: its label, status, path, space and games; a right click offers Make default and Remove location…. `data-status` is `online`, `offline`, `read_only` or `identity_mismatch`; `data-selected` on the selected one; `data-hovered` and `data-focus-visible`.",
        ),
        ("capacity", "How the location's space is used, as a bar."),
        ("capacity-games", "The bar's share taken by games."),
        ("capacity-other", "The bar's share taken by other files."),
        (
            "add-root",
            "Adding a location: its folder, with Browse…, and label.",
        ),
        (
            "selected-root",
            "The selected location: its label and folder-name template, Make default and Remove location….",
        ),
        (
            "remove-root",
            "Removing the selected location: keeping its files, or moving its games to another location first.",
        ),
    ],
    contains: &[
        "error-notice",
        "settings-section",
        "button",
        "menu",
        "text-field",
        "select",
        "setting-row",
        "switch",
    ],
    ..Entry::new(
        "storage-settings",
        "StoragePanel (apps/native/src/settings/storage.rs)",
        Source::App,
        "Settings → Storage (#104): where Caskade installs games. Each location as a card with its status, space and games; adding one; the selected one's label and folder template, made the default, or removed, keeping its files or moving its games to another location first (one job in Activity, #64); and Always use the default location.",
    )
};

pub const PLUGIN_REVIEW: Entry = Entry {
    contains: &["button"],
    ..Entry::new(
        "plugin-review",
        "ReviewDialog (apps/native/src/settings/plugins.rs)",
        Source::App,
        "A plugin's name, version, id and description, what it asks to do and what it adds, before it's allowed or installed (Settings → Plugins, and an add-on's details), with Cancel and Allow or Install and allow; or an installed plugin's details, with Close. An invalid plugin lists its problems instead.",
    )
};

pub const STREAMING_SETTINGS: Entry = Entry {
    parts: &[
        (
            "host-state",
            "What hosting is doing, with a dot; `data-status` is `ready`, `settingUp`, `starting`, `needsAttention`, `unsupported` or `off`, and empty while it isn't known yet.",
        ),
        ("host-warning", "A warning about a ready host, with a dot."),
        (
            "requests",
            "The devices waiting to pair, each with the code to enter (admins).",
        ),
        (
            "paired",
            "The devices that can play from this one, and Forget all paired devices… (admins).",
        ),
    ],
    contains: &[
        "settings-section",
        "setting-row",
        "switch",
        "status-dot",
        "button",
        "error-notice",
        "streaming-existing",
        "text-field",
        "pair-request",
        "streaming-client",
        "stream-host",
        "pair-dialog",
        "select",
        "streaming-program",
    ],
    ..Entry::new(
        "streaming-settings",
        "StreamingPanel (apps/native/src/settings/streaming.rs)",
        Source::App,
        "Settings → Streaming (#265): letting other devices stream games from this one (admins), with Repair, the devices waiting to pair and the ones paired; the devices this one can stream from, pairing with them and adding one by its address; the stream quality; and the streaming programs' updates. Other accounts see a note on what only an admin can do.",
    )
};

pub const STREAMING_EXISTING: Entry = Entry {
    contains: &["text-field", "error-notice", "button"],
    ..Entry::new(
        "streaming-existing",
        "ExistingDialog (apps/native/src/settings/streaming/hosting.rs)",
        Source::App,
        "Streaming is already set up: turning on Let other devices stream games from this one found game streaming set up on this device before. The user name and password it was set up with, Leave it off and Use it.",
    )
};

pub const STREAMING_CLIENT: Entry = Entry {
    contains: &["button"],
    ..Entry::new(
        "streaming-client",
        "PairedDevices (apps/native/src/settings/streaming/hosting.rs)",
        Source::App,
        "A device that can play from this one, in Settings → Streaming → Let other devices play (admins, #49): its name and Forget…, which asks in its row (Forget it or Keep).",
    )
};

pub const STREAM_HOST: Entry = Entry {
    parts: &[(
        "state",
        "Whether it can be reached, and whether it's paired, with how many games.",
    )],
    states: &[("data-status", &["ready", "busy", "asleep", "unreachable"])],
    contains: &["button", "error-notice"],
    ..Entry::new(
        "stream-host",
        "HostRow (apps/native/src/settings/streaming/stream_from.rs)",
        Source::App,
        "A device this one can stream from, in Settings → Streaming → Play from other devices: its name and state, then Pair… (not paired) or Forget…, which asks in its row. `data-status` is `ready`, `busy` (playing another game), `asleep` (it wakes when you stream) or `unreachable`.",
    )
};

pub const PAIR_DIALOG: Entry = Entry {
    parts: &[
        (
            "code",
            "The code to enter in Caskade on the other device; not shown when one of your own devices pairs without a code.",
        ),
        ("actions", "Done, or Try again and Cancel or Close."),
    ],
    states: &[(
        "data-status",
        &[
            "asking",
            "code",
            "paired",
            "wrongCode",
            "declined",
            "expired",
            "failed",
        ],
    )],
    contains: &["button"],
    ..Entry::new(
        "pair-dialog",
        "PairDialog (apps/native/src/settings/streaming/stream_from.rs)",
        Source::App,
        "Pair with a device to stream from (#265), from Pair… in Settings → Streaming: the code to enter on that device, until it answers. `data-status` is `asking`, `code` (waiting for the other device), then `paired`, `wrongCode`, `declined`, `expired` or `failed`; a wrong code, an expired request or a failure can be tried again.",
    )
};

pub const STREAMING_PROGRAM: Entry = Entry {
    parts: &[("version", "Up to date, or the newer version that's ready.")],
    states: &[("data-status", &["update", "current"])],
    contains: &["button"],
    ..Entry::new(
        "streaming-program",
        "UpdatesSection (apps/native/src/settings/streaming.rs)",
        Source::App,
        "One streaming program in Settings → Streaming → Streaming updates (#266): its name, its version and, for an admin when a newer one is ready, Update and Skip this version. `data-status` is `update` when a newer version is ready, else `current`.",
    )
};
