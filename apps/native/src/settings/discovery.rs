//! Settings → Library Discovery (#104), as the webview's
//! `LibraryDiscoverySettings`, `ReviewList` and `HiddenGames`: turning
//! discovery on, Scan now and its progress; results waiting for a decision
//! (accept or ignore, several at once or one by a right click); when scans
//! run; the system's own records; deep scans and their folders; what each
//! source last reported; hidden games shown again; ignored results allowed
//! again. Launchers are their own section (`launchers.rs`), linked from
//! here.

use std::collections::BTreeSet;

use caskade_app::discovery::{DiscoverySnapshot, ReviewItem};
use caskade_core::model::GameSummary;
use caskade_ui_data::discovery::scanning;
use caskade_ui_i18n::{t, tn, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;
use serde_json::{Map, Value};

use super::launchers::source_name;
use super::reset::{ResetDefaults, ResetField, ResetPart};
use super::state::{SettingsFeedback, SettingsLoading, use_settings_section};
use crate::data::{change, try_use_discovery, use_discovery_query, use_library, use_query};
use crate::flows::paths::{PathRequest, PickKind, use_path_picker};
use crate::router::{Route, RouteLink};
use crate::ui::icons::{BAN, CHECK, EYE, FOLDER, FOLDER_OPEN, REFRESH_CW, SCAN_SEARCH};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Checkbox, Menu, MenuEntry, MenuItem, Select, SelectOption,
    SettingRow, SettingsSection, Switch, TextField,
};

/// How many folders deep scans may search, as the webview's.
const MAX_FOLDERS: usize = 64;

fn intervals() -> Vec<SelectOption> {
    [
        ("1h", t!(note = "How often discovery refreshes", "1 hour")),
        ("6h", t!(note = "How often discovery refreshes", "6 hours")),
        (
            "12h",
            t!(note = "How often discovery refreshes", "12 hours"),
        ),
        (
            "24h",
            t!(note = "How often discovery refreshes", "24 hours"),
        ),
        ("7d", t!(note = "How often discovery refreshes", "Weekly")),
    ]
    .into_iter()
    .map(|(id, label)| SelectOption::new(id, label))
    .collect()
}

fn deep_options() -> [(&'static str, String, String); 5] {
    [
        (
            "customFolders",
            t!("Custom folders"),
            t!("Search only the custom paths listed below."),
        ),
        (
            "externalDrives",
            t!("External drives"),
            t!("Search removable volumes reported by this device."),
        ),
        (
            "portableExecutables",
            t!("Portable executables"),
            t!("Search Applications and Games inside your home directory."),
        ),
        (
            "winePrefixes",
            t!("Wine and Proton prefixes"),
            t!("Look for compatibility prefixes inside your home directory."),
        ),
        (
            "fullDisk",
            t!("Full disk scan"),
            t!("Search all reported volumes. This can be slow."),
        ),
    ]
}

/// The controller's scan phase in words; another shows as sent.
fn phase(name: &str) -> String {
    match name {
        "idle" => tp!("discovery phase", "Idle"),
        "scanning" => tp!("discovery phase", "Scanning"),
        "importing" => tp!("discovery phase", "Importing"),
        "cancelling" => tp!("discovery phase", "Cancelling"),
        "complete" => tp!("discovery phase", "Complete"),
        "cancelled" => tp!("discovery phase", "Cancelled"),
        "failed" => tp!("discovery phase", "Failed"),
        other => other.to_owned(),
    }
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// What a discovery action does, for the one runner.
#[derive(Clone)]
enum Action {
    Scan,
    Cancel,
    Review {
        ids: Vec<String>,
        accept: bool,
        revision: u32,
    },
}

#[component]
pub fn LibraryDiscoverySettings() -> Element {
    let preferences = use_settings_section("core.discovery");
    let history = use_settings_section("discovery.import");
    let discovery = try_use_discovery();
    let mut snapshot = use_discovery_query(|discovery| async move { discovery.status().await });
    let mut pending = use_signal(|| false);
    let mut action_error = use_signal(|| None::<String>);
    let mut path = use_signal(String::new);
    let picker = use_path_picker();
    let act = use_callback(move |action: Action| {
        let Some(discovery) = discovery.clone() else {
            return;
        };
        pending.set(true);
        action_error.set(None);
        spawn(async move {
            let outcome = change(async move {
                match action {
                    Action::Scan => discovery.scan().await,
                    Action::Cancel => discovery.cancel().await,
                    Action::Review {
                        ids,
                        accept,
                        revision,
                    } => discovery.review(ids, accept, revision).await,
                }
            })
            .await;
            pending.set(false);
            if let Err(error) = outcome {
                action_error.set(Some(error.summary));
            }
            snapshot.restart();
        });
    });
    let Some(settings) = preferences.value() else {
        return rsx! {
            SettingsLoading {
                error: preferences.load_error(),
                on_reload: move |_| preferences.reload(),
            }
        };
    };
    let current: Option<DiscoverySnapshot> = match &*snapshot.read() {
        Some(Ok(current)) => Some(current.clone()),
        _ => None,
    };
    let load_error = match &*snapshot.read() {
        Some(Err(error)) => Some(error.summary.clone()),
        _ => None,
    };
    let busy = scanning(current.as_ref()) || pending();
    let enabled = flag(&settings, "enabled");
    let toggle =
        move |key: &'static str| move |on: bool| preferences.set_field(key, Value::Bool(on));
    let status = match &current {
        Some(current) => {
            let mut parts = vec![
                phase(&current.phase),
                t!(
                    note = "Games discovery found, counting each game's copies once",
                    "{found} games found",
                    found = current.found
                ),
                t!("{imported} imported", imported = current.imported),
            ];
            if current.watching {
                parts.push(t!("Watching folders"));
            }
            parts.join(" · ")
        }
        None => t!("Loading discovery status…"),
    };
    let field = |key: &str, label: String| ResetField {
        key: key.to_owned(),
        label,
    };
    let reset_parts = vec![ResetPart {
        namespace: "core.discovery".into(),
        whole: true,
        fields: vec![
            field("enabled", t!("Enable library discovery")),
            field("scanOnStartup", t!("Full scan on startup")),
            field("backgroundRefresh", t!("Background refresh")),
            field("refreshInterval", t!("Refresh interval")),
            field("watchFolders", t!("Watch library folders")),
            field("osChecks", t!("Operating-system checks")),
            field("deepScans", t!("Optional deep scans")),
            field("scanPaths", t!("Custom folders")),
            field("ignoredEntries", t!("Ignored results")),
        ],
    }];
    let deep = settings
        .get("deepScans")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let folders: Vec<String> = settings
        .get("scanPaths")
        .and_then(Value::as_array)
        .map(|folders| {
            folders
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let ignored: Vec<Value> = settings
        .get("ignoredEntries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let os_checks = settings
        .get("osChecks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let error = action_error().or(load_error);
    let browse = move |_| {
        let start = path.peek().trim().to_owned();
        spawn(async move {
            let mut request = PathRequest::new(PickKind::Folder, t!("Choose a folder to scan"));
            if !start.is_empty() {
                request.start_at = Some(start);
            }
            // Cancel changes nothing.
            if let Some(chosen) = picker.pick(request).await {
                path.set(chosen);
            }
        });
    };
    let add_folder = {
        let folders = folders.clone();
        move |_| {
            let value = path.peek().trim().to_owned();
            let mut next = folders.clone();
            if !next.contains(&value) {
                next.push(value);
            }
            preferences.set_field(
                "scanPaths",
                Value::Array(next.into_iter().map(Value::String).collect()),
            );
            path.set(String::new());
        }
    };
    rsx! {
        div {
            "data-settings": "library-discovery",
            "data-cs": "discovery-settings",
            class: "flex flex-col gap-4",
            div { class: "flex items-center justify-between gap-3",
                SettingsFeedback {
                    error: preferences.error().or(history.error()),
                    saving: preferences.saving() || history.saving(),
                }
                // Names what goes back and asks first (#44, #49).
                ResetDefaults {
                    group: tp!("settings tab", "Library Discovery"),
                    label: t!("Reset discovery settings"),
                    parts: reset_parts,
                    keeps: t!("Launchers, play history import and settings on other pages stay as they are."),
                    disabled: preferences.saving(),
                }
            }
            SettingsSection {
                icon: Some(SCAN_SEARCH),
                title: t!("Automatic Library Discovery"),
                description: t!("Finds the games your launchers and your system already know about. It reads their records and never starts a launcher."),
                SettingRow {
                    label: t!("Enable library discovery"),
                    description: t!("Off stops every scan, Scan now included, and watching folders."),
                    Switch { checked: enabled, on_change: toggle("enabled") }
                }
                div { class: "mt-3 flex flex-wrap items-center gap-3",
                    Button {
                        variant: ButtonVariant::Primary,
                        disabled: !enabled || busy,
                        on_press: move |_| act(Action::Scan),
                        {t!("Scan now")}
                    }
                    Button {
                        disabled: !scanning(current.as_ref()) || pending(),
                        on_press: move |_| act(Action::Cancel),
                        {t!("Cancel scan")}
                    }
                    p { role: "status", class: "text-sm text-fg-muted", "{status}" }
                }
                if let Some(error) = error {
                    p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
                }
                if let Some(error) = current.as_ref().and_then(|current| current.error.clone()) {
                    p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
                }
            }
            // The decision first; the settings and reports below don't need
            // one (#54).
            if let Some(current) = current.clone() {
                ReviewList {
                    review: current.review.clone(),
                    busy,
                    on_decide: move |(ids, accept): (Vec<String>, bool)| {
                        act(Action::Review {
                            ids,
                            accept,
                            revision: current.revision,
                        })
                    },
                }
            }
            if !enabled {
                // Why everything below is dimmed (#55).
                p { role: "note", class: "text-sm text-fg-muted",
                    {t!("Library discovery is off, so the settings below don't apply until it's on.")}
                }
            }
            div {
                "data-part": "discovery-options",
                "data-disabled": crate::ui::interact::flag(!enabled),
                "aria-disabled": crate::ui::interact::flag(!enabled),
                class: "flex flex-col gap-4 data-disabled:opacity-50",
                SettingsSection {
                    icon: Some(REFRESH_CW),
                    title: t!("Scan behaviour"),
                    description: t!("A full scan looks everywhere again and notices games that were removed. Watching picks up changes as they happen."),
                    SettingRow {
                        label: t!("Full scan on startup"),
                        description: t!("Look everywhere again each time Caskade starts."),
                        Switch {
                            checked: flag(&settings, "scanOnStartup"),
                            disabled: !enabled,
                            on_change: toggle("scanOnStartup"),
                        }
                    }
                    SettingRow {
                        label: t!("Background refresh"),
                        description: t!("Run a full scan on the selected schedule."),
                        div { class: "flex items-center gap-3",
                            Select {
                                "aria-label": t!("Refresh interval"),
                                options: intervals(),
                                value: settings.get("refreshInterval").and_then(Value::as_str).map(str::to_owned),
                                disabled: !enabled || !flag(&settings, "backgroundRefresh"),
                                on_change: move |next: String| preferences.set_field("refreshInterval", Value::String(next)),
                                class: "w-36",
                            }
                            Switch {
                                checked: flag(&settings, "backgroundRefresh"),
                                disabled: !enabled,
                                on_change: toggle("backgroundRefresh"),
                            }
                        }
                    }
                    SettingRow {
                        label: t!("Watch library folders"),
                        description: t!("Notice games installed or removed while Caskade runs, in launchers installed later too."),
                        Switch {
                            checked: flag(&settings, "watchFolders"),
                            disabled: !enabled,
                            on_change: toggle("watchFolders"),
                        }
                    }
                    if let Some(imports) = history.value() {
                        SettingRow {
                            label: t!("Import existing play history"),
                            description: t!("When a game joins your library, bring in the play time its launcher recorded. Time is never counted twice."),
                            Switch {
                                checked: flag(&imports, "seedPlaytime"),
                                disabled: !enabled,
                                on_change: move |on: bool| history.set_field("seedPlaytime", Value::Bool(on)),
                            }
                        }
                    }
                }
                SettingsSection {
                    title: t!("Operating-system checks"),
                    description: t!("Games the system itself keeps a record of, besides launchers' own."),
                    LaunchersLink {}
                    div { class: "flex flex-col gap-3",
                        if let Some(current) = current.clone() {
                            for check in current.checks.clone() {
                                Checkbox {
                                    key: "{check.id}",
                                    label: check.label.clone(),
                                    checked: os_checks
                                        .get(&current.os)
                                        .and_then(|checks| checks.get(&check.id))
                                        != Some(&Value::Bool(false)),
                                    disabled: !enabled,
                                    on_change: {
                                        let os = current.os.clone();
                                        let id = check.id.clone();
                                        let os_checks = os_checks.clone();
                                        move |on: bool| {
                                            let mut all = os_checks.clone();
                                            let mut mine: Map<String, Value> = all
                                                .get(&os)
                                                .and_then(Value::as_object)
                                                .cloned()
                                                .unwrap_or_default();
                                            mine.insert(id.clone(), Value::Bool(on));
                                            all.insert(os.clone(), Value::Object(mine));
                                            preferences.set_field("osChecks", Value::Object(all));
                                        }
                                    },
                                }
                            }
                        }
                    }
                }
                SettingsSection {
                    icon: Some(FOLDER),
                    title: t!("Optional deep scans"),
                    description: t!("Slower searches for games that no launcher knows about. Each is off until you turn it on."),
                    for (key, label, description) in deep_options() {
                        SettingRow { key: "{key}", label, description,
                            Switch {
                                checked: deep.get(key).and_then(Value::as_bool).unwrap_or(false),
                                disabled: !enabled,
                                on_change: {
                                    let deep = deep.clone();
                                    move |on: bool| {
                                        let mut next = deep.clone();
                                        next.insert(key.to_owned(), Value::Bool(on));
                                        preferences.set_field("deepScans", Value::Object(next));
                                    }
                                },
                            }
                        }
                    }
                    div { class: "mt-4",
                        TextField {
                            label: t!("Folder to search"),
                            value: path(),
                            mono: true,
                            disabled: !enabled,
                            description: if deep.get("customFolders").and_then(Value::as_bool).unwrap_or(false) {
                                t!("A full path on this device. Caskade only reads it.")
                            } else {
                                t!("A full path on this device. Folders are searched while Custom folders is on.")
                            },
                            on_input: move |next: String| path.set(next),
                            action: rsx! {
                                span { class: "flex gap-2",
                                    Button {
                                        icon: Some(FOLDER_OPEN),
                                        disabled: !enabled,
                                        on_press: browse,
                                        {t!("Browse…")}
                                    }
                                    Button {
                                        disabled: !enabled || path().trim().is_empty() || folders.len() >= MAX_FOLDERS,
                                        on_press: add_folder,
                                        {t!("Add folder")}
                                    }
                                }
                            },
                        }
                        ul { "data-part": "scan-paths", class: "mt-3 flex flex-col gap-2",
                            for folder in folders.clone() {
                                li {
                                    key: "{folder}",
                                    class: "flex items-center justify-between gap-3 rounded-control border border-border p-2",
                                    span { class: "break-all font-mono text-xs", "{folder}" }
                                    Button {
                                        size: ButtonSize::Sm,
                                        "aria-label": t!("Remove {folder}", folder = folder.clone()),
                                        disabled: !enabled,
                                        on_press: {
                                            let folders = folders.clone();
                                            let folder = folder.clone();
                                            move |_| {
                                                let next: Vec<Value> = folders
                                                    .iter()
                                                    .filter(|known| **known != folder)
                                                    .cloned()
                                                    .map(Value::String)
                                                    .collect();
                                                preferences.set_field("scanPaths", Value::Array(next));
                                            }
                                        },
                                        {t!("Remove")}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(current) = current.clone() {
                SettingsSection {
                    title: t!("Last source reports"),
                    description: t!("What each source found in the last scan, and any problems."),
                    "data-part": "source-reports",
                    ul { class: "mt-3 flex flex-col gap-3",
                        for report in current.sources.clone() {
                            li {
                                key: "{report.source}",
                                class: "rounded-control border border-border p-3",
                                p { class: "text-sm font-medium",
                                    {t!(
                                        note = "A discovery source's last report: its name and the games it found",
                                        "{source}: {games}",
                                        source = source_name(&report.source.to_string()),
                                        games = tn!("# game", "# games", report.count as usize)
                                    )}
                                }
                                if let Some(error) = &report.error {
                                    p { class: "mt-1 text-sm text-danger", "{error}" }
                                }
                                ul { class: "mt-1 text-xs text-fg-muted",
                                    for warning in report.warnings.clone() {
                                        li { key: "{warning}", "{warning}" }
                                    }
                                }
                            }
                        }
                    }
                    for warning in current.warnings.clone() {
                        p { key: "{warning}", class: "mt-2 text-sm text-fg-muted", "{warning}" }
                    }
                }
            }
            HiddenGames {}
            SettingsSection {
                title: t!("Ignored results"),
                description: t!("Games you chose to ignore. Scans leave them out of your library."),
                "data-part": "ignored-results",
                if ignored.is_empty() {
                    p { class: "mt-3 text-sm text-fg-muted", {t!("Nothing ignored.")} }
                }
                ul { class: "mt-3 flex flex-col gap-2",
                    for item in ignored.clone() {
                        IgnoredRow {
                            key: "{item}",
                            item: item.clone(),
                            on_allow: {
                                let ignored = ignored.clone();
                                move |_| {
                                    let next: Vec<Value> = ignored
                                        .iter()
                                        .filter(|entry| {
                                            entry.get("source") != item.get("source")
                                                || entry.get("key") != item.get("key")
                                        })
                                        .cloned()
                                        .collect();
                                    preferences.set_field("ignoredEntries", Value::Array(next));
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn IgnoredRow(item: Value, on_allow: EventHandler<()>) -> Element {
    let text = |key: &str| {
        item.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    // Decisions from before titles were kept show the record's key.
    let title = Some(text("title"))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| text("key"));
    let source = source_name(&text("source"));
    rsx! {
        li { class: "flex items-center justify-between gap-3",
            span { class: "min-w-0",
                span { class: "block text-sm", "{title}" }
                span { class: "block text-xs text-fg-muted", "{source}" }
            }
            Button { size: ButtonSize::Sm, on_press: move |_| on_allow.call(()), {t!("Allow again")} }
        }
    }
}

/// Launchers are their own section (#48); this links there.
#[component]
fn LaunchersLink() -> Element {
    let href = Route::Settings {
        tab: "launchers",
        section: None,
    }
    .href();
    rsx! {
        p { class: "mb-3 text-sm text-fg-muted",
            {crate::trans!("Which launchers are scanned: <0>Launchers</0>";
                0 => |inner| rsx! {
                    RouteLink { to: href.clone(), current: false, class: "text-accent-alt", {inner} }
                })}
        }
    }
}

/// Results that need a decision: accept adds them to the library, ignore
/// keeps future scans from bringing them back.
#[component]
pub(super) fn ReviewList(
    review: Vec<ReviewItem>,
    busy: bool,
    on_decide: EventHandler<(Vec<String>, bool)>,
) -> Element {
    let mut selection = use_signal(BTreeSet::<String>::new);
    let selected: Vec<String> = review
        .iter()
        .filter(|item| selection.read().contains(&item.id))
        .map(|item| item.id.clone())
        .collect();
    let all: Vec<String> = review.iter().map(|item| item.id.clone()).collect();
    let accept = selected.clone();
    let ignore = selected.clone();
    rsx! {
        section {
            "data-cs": "discovery-review",
            "aria-labelledby": "discovery-review-heading",
            class: "rounded-panel border border-border bg-surface-raised p-5",
            h2 { id: "discovery-review-heading", class: "font-display text-lg font-semibold",
                {t!("Review possible games")}
            }
            p { class: "mt-1 text-sm text-fg-muted",
                {t!("These results need your decision. Accept adds them to the library. Ignore saves a source exclusion for future scans.")}
            }
            if review.is_empty() {
                p { class: "mt-4 text-sm text-fg-muted", {t!("No games awaiting review.")} }
            } else {
                div { class: "my-4 flex flex-wrap items-center gap-3",
                    Checkbox {
                        label: t!("Select all"),
                        checked: selected.len() == review.len(),
                        disabled: busy,
                        on_change: move |on: bool| {
                            selection.set(if on { all.iter().cloned().collect() } else { BTreeSet::new() })
                        },
                    }
                    Button {
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Primary,
                        disabled: busy || selected.is_empty(),
                        on_press: move |_| on_decide.call((accept.clone(), true)),
                        {t!(
                            note = "Accepts the checked discovery results; {count} is how many are checked",
                            "Accept selected ({count})",
                            count = selected.len()
                        )}
                    }
                    Button {
                        size: ButtonSize::Sm,
                        disabled: busy || selected.is_empty(),
                        on_press: move |_| on_decide.call((ignore.clone(), false)),
                        {t!("Ignore selected")}
                    }
                }
                ul { class: "flex flex-col gap-3",
                    for item in review.clone() {
                        ReviewRow {
                            key: "{item.id}",
                            checked: selection.read().contains(&item.id),
                            busy,
                            on_check: {
                                let id = item.id.clone();
                                move |on: bool| {
                                    let mut next = selection.peek().clone();
                                    if on {
                                        next.insert(id.clone());
                                    } else {
                                        next.remove(&id);
                                    }
                                    selection.set(next);
                                }
                            },
                            on_decide: {
                                let id = item.id.clone();
                                move |accept: bool| on_decide.call((vec![id.clone()], accept))
                            },
                            item,
                        }
                    }
                }
            }
        }
    }
}

/// One result; a right click accepts or ignores it.
#[component]
fn ReviewRow(
    item: ReviewItem,
    checked: bool,
    busy: bool,
    on_check: EventHandler<bool>,
    on_decide: EventHandler<bool>,
) -> Element {
    let mut row = use_signal(|| None::<NodeHandle>);
    let mut menu = use_signal(|| None);
    let mut accept = MenuItem::new("accept", tp!("discovery review", "Accept"));
    accept.icon = Some(CHECK);
    accept.description = Some(t!("Adds it to the library"));
    accept.disabled = busy;
    let mut ignore = MenuItem::new("ignore", tp!("discovery review", "Ignore"));
    ignore.icon = Some(BAN);
    ignore.description = Some(t!("Future scans skip it"));
    ignore.disabled = busy;
    rsx! {
        li {
            "data-part": "review-item",
            class: "rounded-control border border-border p-4",
            onmounted: move |event: Event<MountedData>| row.set(event.data().downcast::<NodeHandle>().cloned()),
            oncontextmenu: move |event: MouseEvent| {
                menu.set(Some(crate::frame::pointer_anchor(row.peek().as_ref(), &event)));
            },
            if let Some(anchor) = menu() {
                Menu {
                    anchor,
                    items: vec![MenuEntry::Item(accept), MenuEntry::Item(ignore)],
                    label: item.title.clone(),
                    on_close: move |_| menu.set(None),
                    on_action: move |action: String| {
                        menu.set(None);
                        on_decide.call(action == "accept");
                    },
                }
            }
            Checkbox {
                label: item.title.clone(),
                checked,
                disabled: busy,
                on_change: move |on: bool| on_check.call(on),
            }
            p { class: "mt-2 text-xs text-fg-muted",
                {t!(
                    note = "The discovery sources that found a game",
                    "Sources: {0}",
                    0 = item.sources.join(", ")
                )}
            }
            if let Some(location) = &item.location {
                p { class: "mt-1 break-all font-mono text-xs text-fg-muted", "{location}" }
            }
            ul {
                "aria-label": t!(
                    note = "The reasons discovery thinks a result is a game",
                    "Evidence for {title}",
                    title = item.title.clone()
                ),
                class: "mt-2 flex flex-col gap-1 text-xs text-fg-muted",
                for evidence in item.evidence.clone() {
                    li { key: "{evidence}", "{evidence}" }
                }
            }
        }
    }
}

/// The games hidden from a card's actions; here they can be shown again.
#[component]
fn HiddenGames() -> Element {
    let library = use_library();
    let mut games = use_query(caskade_ui_data::Key::Games, |library| async move {
        library.hidden(0, 500).await
    });
    let mut failed = use_signal(|| None::<String>);
    let mut pending = use_signal(|| false);
    let show = use_callback(move |id: String| {
        let library = library.clone();
        pending.set(true);
        failed.set(None);
        spawn(async move {
            let outcome = change(async move { library.set_hidden(&id, false).await }).await;
            pending.set(false);
            match outcome {
                Ok(()) => games.restart(),
                Err(error) => failed.set(Some(error.summary)),
            }
        });
    });
    let read = games.read().clone();
    rsx! {
        SettingsSection {
            title: t!("Hidden games"),
            description: t!("Hidden from the library. Scans that find them again leave them hidden."),
            "data-part": "hidden-games",
            match read {
                Some(Err(error)) => rsx! {
                    p { role: "alert", class: "mt-3 text-sm text-danger",
                        {t!("Couldn't load the hidden games: {0}", 0 = error.summary.clone())}
                    }
                },
                Some(Ok(page)) if !page.items.is_empty() => rsx! {
                    ul { "aria-label": t!("Hidden games"), class: "mt-3 flex flex-col gap-2",
                        for game in page.items.clone() {
                            HiddenRow { key: "{game.id}", game, pending: pending(), on_show: move |id: String| show(id) }
                        }
                    }
                },
                Some(Ok(_)) => rsx! {
                    p { class: "mt-3 text-sm text-fg-muted", {t!("No hidden games. Hide one from its card's actions.")} }
                },
                None => rsx! {
                    p { class: "mt-3 text-sm text-fg-muted", {t!("Loading hidden games…")} }
                },
            }
            if let Some(error) = failed() {
                p { role: "alert", class: "mt-2 text-sm text-danger", "{error}" }
            }
        }
    }
}

#[component]
fn HiddenRow(game: GameSummary, pending: bool, on_show: EventHandler<String>) -> Element {
    let mut row = use_signal(|| None::<NodeHandle>);
    let mut menu = use_signal(|| None);
    let mut again = MenuItem::new("show", t!("Show again"));
    again.icon = Some(EYE);
    again.disabled = pending;
    let id = game.id.clone();
    let menu_id = game.id.clone();
    rsx! {
        li {
            "data-part": "hidden-game",
            class: "flex items-center justify-between gap-3",
            onmounted: move |event: Event<MountedData>| row.set(event.data().downcast::<NodeHandle>().cloned()),
            oncontextmenu: move |event: MouseEvent| {
                menu.set(Some(crate::frame::pointer_anchor(row.peek().as_ref(), &event)));
            },
            if let Some(anchor) = menu() {
                Menu {
                    anchor,
                    items: vec![MenuEntry::Item(again)],
                    label: game.title.clone(),
                    on_close: move |_| menu.set(None),
                    on_action: move |_| {
                        menu.set(None);
                        on_show.call(menu_id.clone());
                    },
                }
            }
            span { class: "min-w-0 truncate text-sm",
                "{game.title}"
                if let Some(platform) = crate::ui::platform_line(&game.platforms) {
                    span { class: "ml-2 text-xs text-fg-muted", "{platform}" }
                }
            }
            Button {
                size: ButtonSize::Sm,
                icon: Some(EYE),
                "aria-label": t!("Show {title} again", title = game.title.clone()),
                disabled: pending,
                on_press: move |_| on_show.call(id.clone()),
                {t!("Show again")}
            }
        }
    }
}
