//! What syncs between devices, and saves (the webview's `SyncChoices` and
//! `SavesSection`).

use super::*;

/// What syncs between paired devices (#227, `core.sync.categories`).
pub(super) const SYNCED: [&str; 8] = [
    "library",
    "playSessions",
    "accounts",
    "artwork",
    "saves",
    "achievementLinks",
    "rommServers",
    "appearance",
];

/// Kept on each device until the user turns them on.
pub(super) const OPTIONAL: [&str; 5] = [
    "launchActions",
    "gameScripts",
    "preferredEmulators",
    "emulatorConfig",
    "addons",
];

pub(super) fn category_label(category: &str) -> (String, String) {
    match category {
        "library" => (
            t!("Library"),
            t!(
                "Game details you changed, favourites, hidden games, collections, matches you set and games you kept apart."
            ),
        ),
        "playSessions" => (
            t!("Play history"),
            t!("Every session from every device, so play time adds up wherever you play."),
        ),
        "accounts" => (
            t!("Accounts and PINs"),
            t!(
                "Who plays here, their age limits and PINs. Wrong PIN attempts are counted on each device."
            ),
        ),
        "artwork" => (
            t!("Your artwork"),
            t!("Covers and images you chose or imported."),
        ),
        "saves" => (t!("Saves"), t!("Game saves, with every version kept.")),
        "achievementLinks" => (
            t!("Achievement links you made"),
            t!("Games you linked to their achievements by hand."),
        ),
        "rommServers" => (
            t!("RomM servers"),
            t!("Servers and what they import. Each device signs in with its own password."),
        ),
        "appearance" => (t!("Appearance"), t!("Theme, accent and layout.")),
        "launchActions" => (
            t!("Launch actions"),
            t!(
                "Play and utility actions you added. Their programs must be at the same place on each device."
            ),
        ),
        "gameScripts" => (
            t!("Game scripts"),
            t!("Scripts that run with one game. They arrive turned off on other devices."),
        ),
        "preferredEmulators" => (
            t!("Preferred emulators"),
            t!(
                "Which emulator each platform uses. One this device doesn't have is offered in Settings → Emulation."
            ),
        ),
        "emulatorConfig" => (
            t!("Emulator configuration"),
            t!(
                "Controller mappings, core options and shader presets, each in this device's own emulator folders."
            ),
        ),
        _ => (
            t!("Plugins and themes"),
            t!(
                "Offers to install the same ones. Plugins still ask for their permissions on each device."
            ),
        ),
    }
}

pub(super) fn local_only() -> [String; 7] {
    [
        t!("Where games are installed"),
        t!("Storage folders"),
        t!("Emulators and their profiles"),
        t!("ROM folders"),
        t!("Store sign-ins (connect each device)"),
        t!("Downloaded metadata and images"),
        t!("Console mode, window and start-up choices"),
    ]
}

/// A category's state when the settings don't name it: synced ones sync.
pub(super) fn state_of(value: &Value, category: &str) -> &'static str {
    match value
        .pointer(&format!("/categories/{category}"))
        .and_then(Value::as_str)
    {
        Some("sync") => "sync",
        Some(_) => "thisDeviceOnly",
        None if SYNCED.contains(&category) => "sync",
        None => "thisDeviceOnly",
    }
}

#[component]
pub(super) fn SyncChoices(collapsed: bool, admin: bool) -> Element {
    let sync = use_settings_section("core.sync");
    let mut expanded = use_signal(|| false);
    let title = t!("What syncs");
    let description = t!(
        "Changes made on any device reach the others when they next connect. Where two devices changed the same thing, the later change wins. This device only keeps a kind out of sync here: this device neither sends its changes nor takes the others'."
    );
    let Some(value) = sync.value() else {
        // Loading says nothing; a failed load says why, with Try again.
        return match sync.load_error() {
            Some(_) => rsx! {
                SettingsSection { icon: REFRESH_CCW, title: title.clone(), description,
                    SettingsStatus { section: sync, what: title, class: "" }
                }
            },
            None => rsx! {},
        };
    };
    let segments = vec![
        Segment::new("sync", tp!("sync choice", "Sync")),
        Segment::new("thisDeviceOnly", t!("This device only")),
    ];
    let row = move |category: &'static str| {
        let (label, description) = category_label(category);
        let segments = segments.clone();
        rsx! {
            SettingRow { key: "{category}", label, description,
                SegmentedControl {
                    "data-category": category,
                    options: segments,
                    value: state_of(&value, category).to_owned(),
                    disabled: !admin || sync.saving(),
                    on_change: move |state: String| {
                        sync.set(move |mut current| {
                            if !current.get("categories").is_some_and(Value::is_object) {
                                current["categories"] = json!({});
                            }
                            current["categories"][category] = Value::String(state.clone());
                            current
                        })
                    },
                }
            }
        }
    };
    let rows = rsx! {
        div { class: "flex flex-col",
            for category in SYNCED { {row(category)} }
        }
        h3 { class: "mt-6 text-sm font-medium", {t!("Usually different on each device")} }
        p { class: "mb-1 text-xs text-fg-muted", {t!("These stay on each device until you choose to sync them.")} }
        div { class: "flex flex-col",
            for category in OPTIONAL { {row(category)} }
        }
    };
    let never_title = t!("Never synced");
    let never_description = t!("These belong to each device's own drives and hardware.");
    let never = rsx! {
        ul { "data-part": "never", class: "grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-x-6 gap-y-1.5 text-sm text-fg-muted",
            for label in local_only() {
                li { key: "{label}", class: "flex items-center gap-2",
                    span { "aria-hidden": "true", class: "size-1 rounded-full bg-fg-subtle" }
                    "{label}"
                }
            }
        }
    };
    if collapsed {
        return rsx! {
            SettingsSection {
                icon: REFRESH_CCW,
                title: title.clone(),
                description: t!("Nothing syncs until a device is paired. Then your library, play history and saves stay in step, unless you choose otherwise."),
                SettingsStatus { section: sync, what: title.clone(), class: "mb-3" }
                Button {
                    id: "sync-choose",
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    icon: if expanded() { CHEVRON_DOWN } else { CHEVRON_RIGHT },
                    "aria-expanded": if expanded() { "true" } else { "false" },
                    on_press: move |_| expanded.toggle(),
                    if admin { {t!("Choose what syncs")} } else { {t!("See what syncs")} }
                }
                if expanded() {
                    p { class: "mt-2 text-sm text-fg-muted", "{description}" }
                    {rows}
                    h3 { class: "mt-6 text-sm font-medium", "{never_title}" }
                    p { class: "mb-2 text-xs text-fg-muted", "{never_description}" }
                    {never}
                }
            }
        };
    }
    rsx! {
        SettingsSection { icon: REFRESH_CCW, title: title.clone(), description,
            SettingsStatus { section: sync, what: title, class: "mb-3" }
            {rows}
        }
        SettingsSection { icon: HARD_DRIVE, title: never_title, description: never_description,
            {never}
        }
    }
}

/// Saves (#241): whether saves sync (set in What syncs) and how many
/// versions each game keeps.
#[component]
pub(super) fn SavesSection(admin: bool) -> Element {
    let sync = use_settings_section("core.sync");
    let saves = use_settings_section("core.saves");
    let title = t!("Saves");
    let description = t!(
        "A version of a game's saves is kept each time you finish playing, and the newest goes to your other devices, so you can carry on where you left off."
    );
    let (Some(sync_value), Some(saves_value)) = (sync.value(), saves.value()) else {
        // Loading says nothing; a failed load says why, with Try again.
        return if sync.load_error().is_some() || saves.load_error().is_some() {
            rsx! {
                SettingsSection { icon: SAVE, title: title.clone(), description,
                    SettingsStatus { section: sync, what: t!("Sync settings"), class: "" }
                    SettingsStatus { section: saves, what: title, class: "" }
                }
            }
        } else {
            rsx! {}
        };
    };
    let syncing = state_of(&sync_value, "saves") == "sync";
    let keep = saves_value
        .get("keep")
        .and_then(Value::as_u64)
        .unwrap_or(20);
    let mut options: Vec<SelectOption> = [5u64, 10, 20, 50, 100]
        .into_iter()
        .map(|count| SelectOption::new(count.to_string(), t!("{count} versions", count = count)))
        .collect();
    if !options
        .iter()
        .any(|option| option.value == keep.to_string())
    {
        options.push(SelectOption::new(
            keep.to_string(),
            t!("{keep} versions", keep = keep),
        ));
    }
    rsx! {
        SettingsSection { icon: SAVE, title, description,
            // Whether saves sync is What syncs' Saves row; one control (#54).
            p { "data-part": "saves-sync", class: "mt-1 text-sm text-fg-muted",
                if syncing {
                    {t!("Saves go to your other devices. Saves in What syncs above changes that.")}
                } else {
                    {t!("Saves stay on this device. Saves in What syncs above changes that.")}
                }
            }
            SettingRow {
                label: t!("Versions to keep"),
                description: t!("For each game. The newest stay, and so does any save that isn't settled between devices yet."),
                Select {
                    id: "saves-keep",
                    options,
                    value: Some(keep.to_string()),
                    disabled: !admin || saves.saving(),
                    on_change: move |count: String| {
                        if let Ok(count) = count.parse::<u64>() {
                            saves.set_field("keep", json!(count));
                        }
                    },
                }
            }
            SettingsStatus { section: sync, what: t!("Sync settings") }
            SettingsStatus { section: saves, what: t!("Versions to keep") }
        }
    }
}
