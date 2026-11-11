//! Settings → Backup (#193, #243, #105), as the webview's `backup/`: Back
//! up now, automatic backups, restoring a backup (one chosen here, or one a
//! paired device keeps), and what backups hold. Admins only: a backup holds
//! every account's data, and a restore replaces it.

use caskade_core::error::ErrorReport;
use caskade_ui_data::backups::{
    BackupJobKind, BackupPlan, BackupStatus, Backups, Component, DeviceBackups, Exclusion,
    Inspection, Phase,
};
use caskade_ui_i18n::{format, t, tn, tp};
use dioxus_native::prelude::*;
use serde_json::{Value, json};

use super::state::{SectionState, SettingsStatus, use_settings_section};
use crate::data::change;
use crate::flows::paths::{PathRequest, PickKind, use_path_picker};
use crate::ui::icons::{
    ARCHIVE_RESTORE, CALENDAR_CLOCK, DATABASE_BACKUP, FOLDER_OPEN, LIFE_BUOY, MONITOR_SMARTPHONE,
};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Checkbox, FailureNotice, NoticeVariant, Segment,
    SegmentedControl, Select, SelectOption, SettingRow, SettingsSection, Svg, TextField,
};

const NAMESPACE: &str = "core.backups";
/// Every component, the library first; the library is always in.
const COMPONENTS: [Component; 6] = [
    Component::Library,
    Component::Artwork,
    Component::DownloadedArtwork,
    Component::Plugins,
    Component::PluginData,
    Component::Themes,
];

/// The app's backups: a test's, else the running app's.
pub fn use_backups() -> Option<Backups> {
    try_use_context::<Backups>().or_else(|| crate::shell::services().map(Backups::of))
}

fn component_id(component: Component) -> &'static str {
    match component {
        Component::Library => "library",
        Component::Artwork => "artwork",
        Component::DownloadedArtwork => "downloadedArtwork",
        Component::Plugins => "plugins",
        Component::PluginData => "pluginData",
        Component::Themes => "themes",
    }
}

fn component_label(component: Component) -> String {
    match component {
        Component::Library => t!("Library and settings"),
        Component::Artwork => t!("Your artwork"),
        Component::DownloadedArtwork => t!("Downloaded artwork"),
        Component::Plugins => t!("Plugins"),
        Component::PluginData => t!("Plugins' files"),
        Component::Themes => t!("Themes"),
    }
}

fn component_description(component: Component) -> String {
    match component {
        Component::Library => t!(
            "Games, play time, collections, edits, launch actions, accounts and settings. Always included."
        ),
        Component::Artwork => {
            t!("Covers and images you chose or imported, and web games' pictures.")
        }
        Component::DownloadedArtwork => t!(
            "Images from IGDB. Caskade downloads them again after a restore, so most people leave them out."
        ),
        Component::Plugins => t!("Installed plugins."),
        Component::PluginData => t!("Files your plugins keep."),
        Component::Themes => t!("Installed themes."),
    }
}

fn exclusion_label(exclusion: Exclusion) -> String {
    match exclusion {
        Exclusion::Credentials => t!("passwords and sign-ins"),
        Exclusion::BrowserData => t!("web games' browser data"),
        Exclusion::GameFiles => t!("games and their saves"),
        Exclusion::ManagedEmulators => t!("emulators Caskade installed"),
        Exclusion::DownloadedRoms => t!("ROMs downloaded from RomM"),
        Exclusion::Caches => t!("caches"),
        Exclusion::PendingJobs => t!("installs under way"),
        Exclusion::Logs => t!("logs"),
    }
}

fn phase_label(phase: Phase) -> String {
    match phase {
        Phase::Snapshot => t!("Copying the library…"),
        Phase::Writing => t!("Writing the backup…"),
        Phase::Verifying => t!("Checking the backup…"),
        Phase::Extracting => t!("Unpacking the backup…"),
        Phase::Migrating => t!("Updating the library…"),
    }
}

/// When, for people.
fn at(ms: i64) -> String {
    format::date_time(crate::library::local_date_time(ms))
}

/// The status, read again on every change, and Unloaded's reason.
#[derive(Clone, Copy, PartialEq)]
struct Shown {
    status: Signal<Option<Result<BackupStatus, ErrorReport>>>,
}

fn use_shown() -> Shown {
    use_context::<Shown>()
}

/// Settings → Backup.
#[component]
pub fn BackupSettings() -> Element {
    let Some(backups) = use_backups() else {
        return rsx! {};
    };
    rsx! {
        BackupPanel { backups }
    }
}

#[component]
fn BackupPanel(backups: Backups) -> Element {
    let shown = Shown {
        status: use_signal(|| None),
    };
    use_context_provider(|| shown);
    {
        let backups = backups.clone();
        use_hook(move || {
            let mut changes = backups.subscribe();
            spawn(async move {
                loop {
                    let read = backups.clone();
                    let status = change(async move { read.status().await }).await;
                    let mut current = shown.status;
                    current.set(Some(status));
                    if changes.changed().await.is_err() {
                        break;
                    }
                }
            });
        });
    }
    rsx! {
        div { class: "flex flex-col gap-6",
            BackUpSection { backups: backups.clone() }
            ScheduleSection { backups: backups.clone() }
            RestoreSection { backups }
            HelpSection {}
        }
    }
}

/// Reads the status again now (after an action).
fn refresh(backups: &Backups, shown: Shown) {
    let backups = backups.clone();
    spawn(async move {
        let status = change(async move { backups.status().await }).await;
        let mut current = shown.status;
        current.set(Some(status));
    });
}

/// A section whose settings or status didn't load (#42): why, with Try
/// again. Nothing while they load.
#[component]
fn Unloaded(
    icon: crate::ui::Icon,
    title: String,
    description: String,
    preferences: Option<SectionState>,
    backups: Backups,
) -> Element {
    let shown = use_shown();
    let status_error = match &*shown.status.read() {
        Some(Err(report)) => Some(report.clone()),
        _ => None,
    };
    let load_error = preferences.and_then(|preferences| preferences.load_error());
    if load_error.is_none() && status_error.is_none() {
        return rsx! {};
    }
    rsx! {
        SettingsSection { icon, title, description,
            if let Some(preferences) = preferences {
                SettingsStatus { section: preferences, what: t!("Backup settings"), class: "" }
            }
            if let Some(report) = status_error {
                FailureNotice {
                    error: report,
                    variant: NoticeVariant::Inline,
                    class: if load_error.is_some() { "mt-3" } else { "" },
                    title: t!("Backups couldn't be loaded"),
                    on_retry: move |_| refresh(&backups, shown),
                }
            }
        }
    }
}

/// A job's bar and what it is doing (`ProgressLine`).
#[component]
fn JobProgress(status: BackupStatus) -> Element {
    let progress = status.running.and_then(|running| running.progress);
    let text = match &progress {
        Some(progress) => phase_label(progress.phase),
        None => t!("Starting…"),
    };
    let value = progress
        .as_ref()
        .filter(|progress| progress.total_bytes > 0)
        .map(|progress| (progress.done_bytes as f64 / progress.total_bytes as f64).min(1.0));
    rsx! {
        crate::ui::Progress {
            value,
            "aria-label": t!("Backup progress"),
            value_text: text.clone(),
            class: "mt-3",
        }
        p { class: "mt-1 text-xs text-fg-muted", "{text}" }
    }
}

fn selected(settings: &Value) -> Vec<Component> {
    COMPONENTS
        .into_iter()
        .filter(|component| {
            settings
                .get("components")
                .and_then(Value::as_array)
                .is_some_and(|list| {
                    list.iter()
                        .any(|id| id.as_str() == Some(component_id(*component)))
                })
        })
        .collect()
}

#[component]
fn BackUpSection(backups: Backups) -> Element {
    let shown = use_shown();
    let preferences = use_settings_section(NAMESPACE);
    let picker = use_path_picker();
    let mut plan = use_signal(|| None::<BackupPlan>);
    let mut checking = use_signal(|| 0u32);
    let starting = use_signal(|| false);
    let start_failure = use_signal(|| None::<ErrorReport>);
    let reveal_failure = use_signal(|| None::<ErrorReport>);
    let mut draft = use_signal(|| None::<String>);
    let title = t!("Back up");
    let description = t!(
        "A backup holds Caskade's library and settings in one file you can keep anywhere. Your games and their saves aren't in it."
    );
    let settings = preferences.value();
    let status = match &*shown.status.read() {
        Some(Ok(status)) => Some(status.clone()),
        _ => None,
    };
    // What a backup would hold, read again as the folder or parts change.
    {
        let backups = backups.clone();
        let destination = settings
            .as_ref()
            .and_then(|settings| settings.get("destination"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|destination| !destination.is_empty());
        let components = settings.as_ref().map(selected).unwrap_or_default();
        let _plan = use_resource(use_reactive!(|(destination, components)| {
            let backups = backups.clone();
            let _ = checking();
            async move {
                let read = change(async move { backups.plan(destination, components).await }).await;
                if let Ok(read) = read {
                    plan.set(Some(read));
                }
            }
        }));
    }
    let (Some(settings), Some(status)) = (settings, status) else {
        return rsx! {
            Unloaded { icon: DATABASE_BACKUP, title, description, preferences: Some(preferences), backups }
        };
    };
    let destination = settings
        .get("destination")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let default_destination = status.default_destination.to_string_lossy().into_owned();
    let chosen = selected(&settings);
    let running = status.running.clone();
    let backing_up = running.as_ref().is_some_and(|running| {
        matches!(
            running.kind,
            BackupJobKind::Backup | BackupJobKind::AutomaticBackup
        )
    });
    let planned = plan();
    let size_of = |component: Component| {
        planned
            .as_ref()
            .and_then(|plan| plan.components.iter().find(|item| item.id == component))
            .map(|item| item.bytes)
    };
    // The default folder is made when needed; a chosen one has to be there.
    let own_folder = !destination.is_empty() && destination != default_destination;
    let missing = own_folder
        && planned
            .as_ref()
            .is_some_and(|plan| !plan.destination_exists);
    let short = planned.as_ref().is_some_and(|plan| {
        plan.available_bytes.is_some_and(|free| {
            free < plan.estimated_bytes + size_of(Component::Library).unwrap_or(0)
        })
    });
    let blocked = missing || short;
    let set_destination =
        move |path: String| preferences.set_field("destination", Value::String(path));
    let last = status.last.clone();
    rsx! {
        SettingsSection { icon: DATABASE_BACKUP, title, description,
            // Saved on Enter or Save: each save checks the folder on disk.
            TextField {
                id: "backup-folder",
                label: t!("Backup folder"),
                value: draft().unwrap_or_else(|| destination.clone()),
                placeholder: default_destination.clone(),
                mono: true,
                on_input: move |text| draft.set(Some(text)),
                on_submit: move |text: String| {
                    draft.set(None);
                    set_destination(text);
                },
                action: rsx! {
                    Button {
                        id: "backup-browse",
                        icon: FOLDER_OPEN,
                        "data-focus-default": "true",
                        on_press: {
                            let start_at = if destination.is_empty() { default_destination.clone() } else { destination.clone() };
                            move |_| {
                                let mut request = PathRequest::new(PickKind::Folder, t!("Choose a backup folder"));
                                request.start_at = Some(start_at.clone());
                                spawn(async move {
                                    if let Some(path) = picker.pick(request).await {
                                        draft.set(None);
                                        set_destination(path);
                                    }
                                });
                            }
                        },
                        {t!("Browse…")}
                    }
                },
            }
            if missing {
                p { role: "alert", class: "mt-2 text-sm text-warning",
                    {t!("This folder isn't there. Connect its drive, or choose another folder. Automatic backups try again later.")}
                }
            }
            if short {
                p { role: "alert", class: "mt-2 text-sm text-warning",
                    {t!("There isn't enough free space in this folder for a backup.")}
                }
            }
            fieldset { "data-part": "components", class: "mt-4 flex flex-col gap-2",
                legend { class: "mb-2 text-sm font-medium", {t!("What to back up")} }
                for component in COMPONENTS {
                    div { key: "{component_id(component)}", class: "flex items-start gap-2",
                        Checkbox {
                            "data-component": component_id(component),
                            checked: component == Component::Library || chosen.contains(&component),
                            disabled: component == Component::Library,
                            label: component_label(component),
                            on_change: move |on: bool| {
                                let id = component_id(component);
                                preferences.set(move |mut current| {
                                    let mut list: Vec<Value> = current
                                        .get("components")
                                        .and_then(Value::as_array)
                                        .cloned()
                                        .unwrap_or_default();
                                    list.retain(|item| item.as_str() != Some(id));
                                    if on {
                                        list.push(json!(id));
                                    }
                                    current["components"] = Value::Array(list);
                                    current
                                })
                            },
                        }
                        span { class: "flex flex-col",
                            if let Some(bytes) = size_of(component) {
                                span { class: "font-mono text-xs text-fg-muted", {format::size(Some(bytes))} }
                            }
                            span { class: "text-xs text-fg-muted", {component_description(component)} }
                        }
                    }
                }
            }
            SettingsStatus { section: preferences, what: t!("Backup settings") }
            if let Some(plan) = planned.clone() {
                p { class: "mt-3 text-xs text-fg-muted",
                    {t!(
                        "About {0} before compression. Never included: {1}.",
                        0 = format::size(Some(plan.estimated_bytes)),
                        1 = plan.excluded.iter().map(|exclusion| exclusion_label(*exclusion)).collect::<Vec<_>>().join(", ")
                    )}
                }
            }
            div { class: "mt-4 flex flex-wrap items-center gap-3",
                if backing_up {
                    Button {
                        id: "backup-cancel",
                        on_press: {
                            let backups = backups.clone();
                            move |_| backups.cancel()
                        },
                        {t!("Cancel")}
                    }
                } else {
                    Button {
                        id: "backup-now",
                        variant: ButtonVariant::Primary,
                        icon: DATABASE_BACKUP,
                        disabled: running.is_some() || starting() || blocked,
                        on_press: {
                            let backups = backups.clone();
                            move |_| {
                                let backups = backups.clone();
                                let (mut starting, mut start_failure) = (starting, start_failure);
                                starting.set(true);
                                start_failure.set(None);
                                spawn(async move {
                                    let result = change({
                                        let backups = backups.clone();
                                        async move { backups.start().await }
                                    })
                                    .await;
                                    starting.set(false);
                                    if let Err(report) = result {
                                        start_failure.set(Some(report));
                                    }
                                    refresh(&backups, shown);
                                });
                            }
                        },
                        {t!("Back up now")}
                    }
                }
                if blocked && !backing_up {
                    span { class: "flex items-center gap-2 text-sm text-fg-muted",
                        span {
                            if missing {
                                {t!("Can't back up: the backup folder isn't there.")}
                            } else {
                                {t!("Can't back up: the backup folder doesn't have enough free space.")}
                            }
                        }
                        // A drive plugged in or space freed tells the app nothing.
                        Button {
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Ghost,
                            on_press: move |_| checking += 1,
                            {t!("Check again")}
                        }
                    }
                }
                if let Some(last) = last.clone().filter(|last| last.error.is_none() && last.path.is_some() && !backing_up) {
                    span { class: "flex items-center gap-2 text-sm text-fg-muted",
                        span { role: "status", "data-part": "last",
                            if last.automatic {
                                {t!("Last backed up automatically {time} ({size}), checked.", time = at(last.at_ms), size = format::size(Some(last.bytes)))}
                            } else {
                                {t!("Backed up {time} ({size}), checked.", time = at(last.at_ms), size = format::size(Some(last.bytes)))}
                            }
                        }
                        Button {
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Ghost,
                            icon: FOLDER_OPEN,
                            on_press: {
                                let backups = backups.clone();
                                let path = last.path.clone().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default();
                                move |_| {
                                    let (backups, path) = (backups.clone(), path.clone());
                                    let mut failure = reveal_failure;
                                    spawn(async move {
                                        if let Err(report) = change(async move { backups.reveal(path).await }).await {
                                            failure.set(Some(report));
                                        }
                                    });
                                }
                            },
                            {t!("Show in folder")}
                        }
                    }
                }
            }
            if backing_up {
                JobProgress { status: status.clone() }
            }
            if let Some(report) = last.as_ref().filter(|_| !backing_up).and_then(|last| last.error.clone()) {
                FailureNotice {
                    error: report,
                    variant: NoticeVariant::Inline,
                    class: "mt-3",
                    title: if last.as_ref().is_some_and(|last| last.automatic) { t!("The automatic backup failed") } else { t!("The backup failed") },
                }
            }
            if let Some(report) = start_failure().or(reveal_failure()) {
                FailureNotice { error: report, variant: NoticeVariant::Inline, class: "mt-3" }
            }
        }
    }
}

#[component]
fn ScheduleSection(backups: Backups) -> Element {
    let shown = use_shown();
    let preferences = use_settings_section(NAMESPACE);
    let title = t!("Automatic backups");
    let description = t!(
        "Made in the background to the backup folder, with the same contents. If Caskade isn't running when one is due, it's made soon after the next start."
    );
    let settings = preferences.value();
    let status = match &*shown.status.read() {
        Some(Ok(status)) => Some(status.clone()),
        _ => None,
    };
    let (Some(settings), Some(status)) = (settings, status) else {
        return rsx! {
            Unloaded { icon: CALENDAR_CLOCK, title, description, preferences: Some(preferences), backups }
        };
    };
    let schedule = settings
        .get("schedule")
        .and_then(Value::as_str)
        .unwrap_or("off")
        .to_owned();
    let keep = settings.get("keep").and_then(Value::as_u64).unwrap_or(7);
    let mut keep_options: Vec<SelectOption> = [3u64, 5, 7, 14, 30]
        .into_iter()
        .map(|n| SelectOption::new(n.to_string(), n.to_string()))
        .collect();
    if !keep_options
        .iter()
        .any(|option| option.value == keep.to_string())
    {
        keep_options.push(SelectOption::new(keep.to_string(), keep.to_string()));
    }
    let next = match status.next_automatic_ms {
        Some(next) if schedule != "off" => Some(if next <= caskade_core::now_ms() {
            t!("Due now: it starts within a few minutes.")
        } else {
            t!("Next one {0}.", 0 = at(next))
        }),
        _ => None,
    };
    rsx! {
        SettingsSection { icon: CALENDAR_CLOCK, title: title.clone(), description,
            SettingRow { label: t!("Back up automatically"), description: next,
                SegmentedControl {
                    "data-part": "schedule",
                    options: vec![
                        Segment::new("off", tp!("automatic backups", "Off")),
                        Segment::new("daily", t!("Daily")),
                        Segment::new("weekly", t!("Weekly")),
                    ],
                    value: schedule,
                    on_change: move |schedule: String| preferences.set_field("schedule", Value::String(schedule)),
                }
            }
            SettingRow {
                label: t!("Automatic backups to keep"),
                description: t!("Older ones are removed only after a new one has been checked. Backups you make yourself, and other files, are never removed."),
                Select {
                    id: "backup-keep",
                    options: keep_options,
                    value: Some(keep.to_string()),
                    on_change: move |keep: String| {
                        if let Ok(keep) = keep.parse::<u64>() {
                            preferences.set_field("keep", json!(keep));
                        }
                    },
                }
            }
            SettingsStatus { section: preferences, what: title }
            KeepOnDevice { backups, preferences }
        }
    }
}

/// Also keep backups on: a paired device, or only here (#243). Shown once
/// this device has paired devices.
#[component]
fn KeepOnDevice(backups: Backups, preferences: SectionState) -> Element {
    let paired = use_resource(move || {
        let backups = backups.clone();
        async move {
            caskade_app::runtime::spawn(async move { backups.paired().await })
                .await
                .unwrap_or_default()
        }
    });
    let paired = paired.read().clone().unwrap_or_default();
    let Some(settings) = preferences.value() else {
        return rsx! {};
    };
    if paired.is_empty() {
        return rsx! {};
    }
    let chosen = settings
        .get("keepOnDevice")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut options = vec![SelectOption::new(
        "",
        tp!("keep backups on", "Only this device"),
    )];
    options.extend(
        paired
            .iter()
            .map(|device| SelectOption::new(device.id.clone(), device.name.clone())),
    );
    // A device forgotten since stays listed until changed.
    if !chosen.is_empty() && !options.iter().any(|option| option.value == chosen) {
        options.push(SelectOption::new(
            chosen.clone(),
            t!("A device no longer paired"),
        ));
    }
    let label = t!("Also keep backups on");
    rsx! {
        SettingRow {
            label: label.clone(),
            description: t!("Each backup also goes to that device the next time they sync, even when it was away. It keeps them apart from its own, as many as are kept here."),
            Select {
                options,
                value: Some(chosen),
                on_change: move |device: String| {
                    preferences.set_field("keepOnDevice", if device.is_empty() { Value::Null } else { Value::String(device) })
                },
            }
        }
        SettingsStatus { section: preferences, what: label }
    }
}

#[component]
fn RestoreSection(backups: Backups) -> Element {
    let shown = use_shown();
    let picker = use_path_picker();
    let mut chosen = use_signal(|| None::<(String, Inspection)>);
    let mut components = use_signal(Vec::<Component>::new);
    let busy_action = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let title = t!("Restore");
    let description = t!(
        "Restoring replaces this library and its settings with the backup's, then restarts Caskade. Nothing is merged, and your games' files aren't touched."
    );
    let status = match &*shown.status.read() {
        Some(Ok(status)) => status.clone(),
        _ => {
            return rsx! {
                Unloaded { icon: ARCHIVE_RESTORE, title, description, preferences: None, backups }
            };
        }
    };
    let open = {
        let backups = backups.clone();
        use_callback(move |path: String| {
            let backups = backups.clone();
            let mut busy = busy_action;
            chosen.set(None);
            failure.set(None);
            busy.set(true);
            spawn(async move {
                let read = change({
                    let path = path.clone();
                    async move { backups.inspect(path).await }
                })
                .await;
                busy.set(false);
                match read {
                    Ok(inspection) => {
                        components.set(
                            inspection
                                .components
                                .iter()
                                .map(|summary| summary.id)
                                .filter(|id| *id != Component::Library)
                                .collect(),
                        );
                        chosen.set(Some((path, inspection)));
                    }
                    Err(report) => failure.set(Some(report)),
                }
            });
        })
    };
    let restart = {
        let backups = backups.clone();
        use_callback(move |()| {
            let backups = backups.clone();
            spawn(async move {
                match change(async move { backups.restart_ready().await }).await {
                    Ok(()) => {
                        if let Err(error) = crate::updater::restart() {
                            failure.set(Some(ErrorReport::from(error)));
                        }
                    }
                    Err(report) => failure.set(Some(report)),
                }
            });
        })
    };
    let staged = status.staged_restore.clone();
    let outcome = status.last_restore.clone();
    let busy = status.running.is_some() || busy_action();
    let default_destination = status.default_destination.to_string_lossy().into_owned();
    rsx! {
        SettingsSection { icon: ARCHIVE_RESTORE, title, description,
            if let Some(outcome) = outcome {
                p { role: "status", class: if outcome.error.is_some() { "mb-3 text-sm text-danger" } else { "mb-3 text-sm text-fg" },
                    match outcome.error.clone() {
                        Some(error) => t!("The restore of {0} was undone, and the library from before is back: {1}", 0 = outcome.journal.archive.clone(), 1 = error),
                        None => t!("Restored from {0}.", 0 = outcome.journal.archive.clone()),
                    }
                }
            }
            if let Some(staged) = staged.clone() {
                div { "data-part": "staged", class: "flex flex-col gap-3",
                    p { class: "text-sm",
                        {t!("{0} is ready to restore. Caskade applies it when it restarts.", 0 = staged.archive.clone())}
                    }
                    div { class: "flex gap-3",
                        // The same words as the restore it finishes (#57).
                        Button { variant: ButtonVariant::Primary, disabled: busy, on_press: move |_| restart.call(()), {t!("Restore and restart")} }
                        Button {
                            disabled: busy,
                            on_press: {
                                let backups = backups.clone();
                                move |_| {
                                    let backups = backups.clone();
                                    spawn(async move {
                                        let read = backups.clone();
                                        if let Err(report) = change(async move { read.discard_restore().await }).await {
                                            failure.set(Some(report));
                                        }
                                        refresh(&backups, shown);
                                    });
                                }
                            },
                            {t!("Don't restore")}
                        }
                    }
                }
            } else {
                div {
                    Button {
                        id: "backup-choose",
                        icon: FOLDER_OPEN,
                        disabled: busy,
                        on_press: move |_| {
                            let mut request = PathRequest::new(PickKind::File, t!("Choose a Caskade backup"));
                            request.start_at = Some(default_destination.clone());
                            request.extensions = vec!["zip".to_owned()];
                            spawn(async move {
                                if let Some(path) = picker.pick(request).await {
                                    open.call(path);
                                }
                            });
                        },
                        {t!("Choose a backup…")}
                    }
                    DeviceBackupList { backups: backups.clone(), busy, on_open: move |path: String| open.call(path) }
                }
            }
            if status.running.as_ref().is_some_and(|running| matches!(running.kind, BackupJobKind::Inspect | BackupJobKind::Restore)) {
                JobProgress { status: status.clone() }
            }
            if let (Some((path, inspection)), None) = (chosen(), staged) {
                RestorePreview {
                    inspection,
                    components: components(),
                    busy,
                    on_components: move |list| components.set(list),
                    on_cancel: move |_| chosen.set(None),
                    on_restore: {
                        let backups = backups.clone();
                        move |_| {
                            let (backups, path, list) = (backups.clone(), path.clone(), components());
                            let mut busy = busy_action;
                            busy.set(true);
                            spawn(async move {
                                let read = backups.clone();
                                let result = change(async move { read.restore(path, list).await }).await;
                                busy.set(false);
                                match result {
                                    Ok(_) => restart.call(()),
                                    Err(report) => failure.set(Some(report)),
                                }
                                refresh(&backups, shown);
                            });
                        }
                    },
                }
            }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline, class: "mt-3" }
            }
        }
    }
}

#[component]
fn RestorePreview(
    inspection: Inspection,
    components: Vec<Component>,
    busy: bool,
    on_components: EventHandler<Vec<Component>>,
    on_restore: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    let manifest = &inspection.manifest;
    let games = manifest
        .tables
        .iter()
        .find(|table| table.name == "games")
        .map_or(0, |table| table.rows);
    let made = at(manifest.created_at_ms);
    let version = manifest.app_version.clone();
    let game_count = tn!("# game", "# games", games);
    let missing_roots = inspection
        .missing_roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(", ");
    rsx! {
        section { "data-part": "preview", "aria-label": t!("Backup to restore"), class: "mt-4 flex flex-col gap-3 rounded-control border border-border bg-surface-base/30 p-4",
            p { class: "text-sm",
                {t!("Made {made} by Caskade {version}, with {gameCount}.", made = made, version = version, gameCount = game_count)}
            }
            fieldset { class: "flex flex-col gap-2",
                legend { class: "mb-1 text-sm font-medium", {t!("Restore")} }
                for summary in inspection.components.clone() {
                    div { key: "{component_id(summary.id)}", class: "flex items-center gap-2",
                        Checkbox {
                            checked: summary.id == Component::Library || components.contains(&summary.id),
                            disabled: summary.id == Component::Library,
                            label: component_label(summary.id),
                            on_change: {
                                let components = components.clone();
                                move |on: bool| {
                                    let mut next: Vec<Component> = components.iter().copied().filter(|id| *id != summary.id).collect();
                                    if on {
                                        next.push(summary.id);
                                    }
                                    on_components.call(next);
                                }
                            },
                        }
                        span { class: "font-mono text-xs text-fg-muted", {format::size(Some(summary.bytes))} }
                    }
                }
            }
            ul { class: "flex list-disc flex-col gap-1 pl-5 text-xs text-fg-muted",
                li { {t!("Parts you leave unticked stay as they are now. Plugins restored without their files ask for their permissions again.")} }
                li { {t!("Passwords and sign-ins aren't in backups: sign in to RomM and your stores again afterwards.")} }
                if inspection.needs_migration {
                    li { {t!("It was made by an older Caskade: its library is updated as it's restored.")} }
                }
                if inspection.other_os {
                    li { class: "text-warning",
                        {t!("It was made on another operating system: games and storage locations whose folders aren't on this computer show as missing until you find them again.")}
                    }
                }
                if !inspection.missing_roots.is_empty() {
                    li { class: "text-warning",
                        {t!("Storage locations not found here: {0}. They show as offline until their drives are connected.", 0 = missing_roots)}
                    }
                }
                if inspection.missing_games > 0 {
                    li { class: "text-warning",
                        {tn!(
                            "# game's folder isn't on this computer; the next scan finds it if it's elsewhere.",
                            "# games' folders aren't on this computer; the next scan finds those that are elsewhere.",
                            u64::from(inspection.missing_games)
                        )}
                    }
                }
            }
            div { class: "flex gap-3",
                Button { id: "restore-now", variant: ButtonVariant::Danger, icon: ARCHIVE_RESTORE, disabled: busy, on_press: move |_| on_restore.call(()), {t!("Restore and restart")} }
                Button { disabled: busy, on_press: move |_| on_cancel.call(()), {t!("Cancel")} }
            }
        }
    }
}

/// Backups paired devices keep, each restorable here.
#[component]
fn DeviceBackupList(backups: Backups, busy: bool, on_open: EventHandler<String>) -> Element {
    let held = {
        let backups = backups.clone();
        use_resource(move || {
            let backups = backups.clone();
            async move {
                change(async move { backups.on_devices().await })
                    .await
                    .unwrap_or_default()
            }
        })
    };
    let mut fetching = use_signal(|| None::<String>);
    let mut failure = use_signal(|| None::<String>);
    let devices: Vec<DeviceBackups> = held.read().clone().unwrap_or_default();
    if devices.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { "data-part": "device-backups", class: "mt-4 flex flex-col gap-3",
            h3 { class: "flex items-center gap-2 text-sm font-semibold",
                Svg { icon: MONITOR_SMARTPHONE, class: "size-4 text-fg-muted" }
                {t!("On your other devices")}
            }
            for device in devices {
                section { key: "{device.device_id}", "aria-label": t!("Kept on {holder}", holder = device.device_name.clone()),
                    p { class: "text-xs text-fg-muted", {t!("Kept on {holder}", holder = device.device_name.clone())} }
                    ul { class: "mt-1 flex flex-col gap-1",
                        for backup in device.backups.clone() {
                            {
                                let from = if backup.origin_name.is_empty() { tp!("backup of", "Another device") } else { backup.origin_name.clone() };
                                let when = at(backup.created_ms);
                                let hash = backup.hash.clone();
                                let holder = device.device_name.clone();
                                rsx! {
                                    li { key: "{backup.hash}", class: "flex flex-wrap items-center justify-between gap-3 text-sm",
                                        span {
                                            {t!("{from}, {when}", from = from.clone(), when = when.clone())}
                                            span { class: "ml-2 text-xs text-fg-muted", {format::bytes(Some(backup.size))} }
                                        }
                                        Button {
                                            "data-spatial": "focusable",
                                            size: ButtonSize::Sm,
                                            disabled: busy || fetching().is_some(),
                                            "aria-label": t!("Restore the backup of {from} from {when}", from = from.clone(), when = when.clone()),
                                            on_press: {
                                                let (backups, hash) = (backups.clone(), hash.clone());
                                                move |_| {
                                                    let (backups, hash) = (backups.clone(), hash.clone());
                                                    fetching.set(Some(hash.clone()));
                                                    failure.set(None);
                                                    spawn(async move {
                                                        let got = change(async move { backups.fetch_from_device(&hash).await }).await;
                                                        fetching.set(None);
                                                        match got {
                                                            Ok(path) => on_open.call(path),
                                                            Err(report) => failure.set(Some(report.summary)),
                                                        }
                                                    });
                                                }
                                            },
                                            if fetching().as_deref() == Some(hash.as_str()) {
                                                {t!("Getting it from {holder}…", holder = holder)}
                                            } else {
                                                {t!("Restore…")}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(message) = failure() {
                p { role: "alert", class: "text-sm text-danger", "{message}" }
            }
        }
    }
}

#[component]
fn HelpSection() -> Element {
    rsx! {
        SettingsSection { icon: LIFE_BUOY, title: t!("About backups"),
            ul { class: "flex list-disc flex-col gap-2 pl-5 text-sm text-fg-muted",
                li { {t!("Backups never hold passwords or sign-ins, web games' browser data, logs, caches, or your games and their saves.")} }
                li { {t!("Caskade keeps versions of each game's saves and sends the newest to your paired devices. Find them in Settings → Devices → Saves, and restore one from the game's page.")} }
                li { {t!("To move to another computer, copy a backup there, install Caskade, and restore it from here. Games keep their place in your library; Caskade finds their folders again when you scan, or you can point it at them.")} }
                li { {t!("If Caskade can't open its library, its error message offers Restore from backup…. You can also start Caskade with --restore and a backup file.")} }
                li { {t!("Every backup is read back and checked before it's reported done, and a restore keeps the library it replaces until the restored one opens.")} }
            }
        }
    }
}
