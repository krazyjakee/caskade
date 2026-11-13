//! Settings → Emulation → ROM scanners (#115, #116), as the webview's
//! `emulators/ScannersSection` and `ScanReview`: the saved scanners and
//! what each found last, one scanner's editor, its latest scan (progress,
//! the manual scan's review list, or its outcome) and the exclusions every
//! scanner applies.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;

use caskade_core::emulation::{EmulatorTarget, ProfileRef, ScanStatus};
use caskade_core::error::ErrorReport;
use caskade_ui_data::emulation::{
    PlatformView, ReviewChoice, RomScanner, ScanJob, ScanMode, ScannerView,
};
use caskade_ui_i18n::{t, tn};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use super::fields::{PathField, StackedCell, StackedTable};
use super::{Snapshot, merge_scan, use_emulation, use_live};
use crate::data::change;
use crate::settings::unsaved::{UnsavedForm, UnsavedHint, use_unsaved_changes};
use crate::ui::icons::{
    ARCHIVE, CLOUD_OFF, COMBINE, FILTER_X, FOLDER_SEARCH, FOLDER_TREE, INFO, LIST_CHECKS, PENCIL,
    PLAY, PLUS, REFRESH_CW, SPLIT, TRASH, X,
};
use crate::ui::interact::{Keys, flag, use_interaction};
use crate::ui::{
    Anchor, Button, ButtonSize, ButtonVariant, Checkbox, ConfirmDialog, DotTone, FailureNotice,
    Icon, Menu, MenuEntry, MenuItem, NoticeVariant, Select, SelectOption, SettingRow,
    SettingsSection, StatusDot, Svg, Switch, SwitchSize, TextField,
};

mod exclusions;
mod scan_review;
mod scanner_editor;
use exclusions::{ExclusionList, ExclusionSyntax, GlobalExclusions};
use scan_review::ScanReview;
use scanner_editor::ScannerEditor;

/// The key of a form that adds rather than edits.
const NEW_KEY: &str = "new";

fn launch_mode_label(mode: &str) -> String {
    match mode {
        "profile" => t!("This scanner's profile"),
        "ask_profile" => t!("Ask for a profile of this emulator"),
        "ask_emulator" => t!("Ask for any emulator"),
        _ => t!(
            note = "What a scanner's games launch with",
            "Platform default"
        ),
    }
}

fn launch_mode(target: &EmulatorTarget) -> &'static str {
    match target {
        EmulatorTarget::PlatformDefault => "platform_default",
        EmulatorTarget::Profile { .. } => "profile",
        EmulatorTarget::AskProfile { .. } => "ask_profile",
        EmulatorTarget::AskEmulator => "ask_emulator",
    }
}

/// The launch choice `mode` for `scanner`'s profile.
fn launch_with(mode: &str, scanner: &RomScanner) -> EmulatorTarget {
    match mode {
        "profile" => EmulatorTarget::Profile {
            emulator_id: scanner.profile.emulator_id.clone(),
            profile_id: scanner.profile.profile_id.clone(),
        },
        "ask_profile" => EmulatorTarget::AskProfile {
            emulator_id: scanner.profile.emulator_id.clone(),
        },
        "ask_emulator" => EmulatorTarget::AskEmulator,
        _ => EmulatorTarget::PlatformDefault,
    }
}

fn profile_ref(emulator_id: &str, profile_id: &str) -> String {
    format!("{emulator_id}/{profile_id}")
}

/// A configured emulator's profile, as a scanner chooses it.
#[derive(Clone, PartialEq)]
struct ProfileOption {
    id: String,
    label: String,
    emulator_id: String,
    profile_id: String,
    platforms: Vec<String>,
    extensions: Vec<String>,
}

fn profile_options(snapshot: &Snapshot) -> Vec<ProfileOption> {
    snapshot
        .emulators
        .iter()
        .flat_map(|view| {
            view.emulator.profiles.iter().filter_map(move |config| {
                let state = view
                    .profiles
                    .iter()
                    .find(|profile| profile.id == config.id())?;
                Some(ProfileOption {
                    id: profile_ref(&view.emulator.id, config.id()),
                    label: format!("{} · {}", view.emulator.name, state.name),
                    emulator_id: view.emulator.id.clone(),
                    profile_id: config.id().to_owned(),
                    platforms: state.platforms.clone(),
                    extensions: state.extensions.clone(),
                })
            })
        })
        .collect()
}

fn empty_scanner(snapshot: &Snapshot, name: String) -> RomScanner {
    let first = profile_options(snapshot).into_iter().next();
    RomScanner {
        id: String::new(),
        name,
        folder: PathBuf::new(),
        platform: None,
        profile: ProfileRef {
            emulator_id: first
                .as_ref()
                .map(|option| option.emulator_id.clone())
                .unwrap_or_default(),
            profile_id: first.map(|option| option.profile_id).unwrap_or_default(),
        },
        launch_with: EmulatorTarget::PlatformDefault,
        recursive: true,
        scan_archives: false,
        skip_cloud_placeholders: true,
        exclusions: Vec::new(),
        in_library_update: true,
    }
}

/// The ROM scanners section.
#[component]
pub fn ScannersSection(snapshot: Snapshot) -> Element {
    let emulation = use_emulation();
    let live = use_live();
    // Bumped where the webview invalidates its emulation queries.
    let mut reload = use_signal(|| 0u64);
    // A finished scan changes its scanner's games and last scan.
    let finished = use_memo(move || {
        live.scans
            .read()
            .iter()
            .filter(|job| job.status == ScanStatus::Done)
            .count()
    });
    let mut scanners = {
        let emulation = emulation.clone();
        use_resource(move || {
            let _ = (live.changed)();
            let _ = reload();
            let _ = finished();
            let emulation = emulation.clone();
            change(async move { emulation.scanners().await })
        })
    };
    let mut selected_id = use_signal(|| None::<String>);
    let mut adding = use_signal(|| false);
    let mut scan_pending = use_signal(|| false);
    let mut scan_error = use_signal(|| None::<ErrorReport>);
    let mut toggle_pending = use_signal(|| false);
    let mut toggle_error = use_signal(|| None::<ErrorReport>);
    let open_scanner = use_callback(move |id: String| {
        adding.set(false);
        selected_id.set(Some(id));
    });
    let start_scan = {
        let emulation = emulation.clone();
        use_callback(move |id: String| {
            open_scanner.call(id.clone());
            scan_pending.set(true);
            scan_error.set(None);
            let emulation = emulation.clone();
            let mut scans = live.scans;
            spawn(async move {
                let started =
                    change(async move { emulation.start_scan(&id, ScanMode::Review).await }).await;
                scan_pending.set(false);
                match started {
                    Ok(job) => merge_scan(&mut scans.write(), job),
                    Err(report) => scan_error.set(Some(report)),
                }
            });
        })
    };
    let toggle = {
        let emulation = emulation.clone();
        use_callback(move |scanner: RomScanner| {
            toggle_pending.set(true);
            toggle_error.set(None);
            let emulation = emulation.clone();
            spawn(async move {
                let saved = change(async move { emulation.save_scanner(scanner).await }).await;
                toggle_pending.set(false);
                match saved {
                    Ok(_) => *reload.write() += 1,
                    Err(report) => toggle_error.set(Some(report)),
                }
            });
        })
    };
    let loaded = match &*scanners.read() {
        None => None,
        Some(Ok(rows)) => Some(Ok(rows.clone())),
        Some(Err(report)) => Some(Err(report.clone())),
    };
    let rows = match loaded {
        None => {
            return rsx! {
                p { role: "status", class: "p-5 text-sm text-fg-muted", {t!("Loading ROM scanners…")} }
            };
        }
        Some(Err(report)) => {
            return rsx! {
                div {
                    role: "alert",
                    class: "rounded-panel border border-border p-5 text-sm text-fg-muted",
                    p { {t!("ROM scanners could not be loaded.")} }
                    p { class: "mt-2", "{report.summary}" }
                    Button {
                        id: "scanners-retry",
                        class: "mt-3",
                        on_press: move |_| scanners.restart(),
                        {t!("Try again")}
                    }
                }
            };
        }
        Some(Ok(rows)) => rows,
    };
    let selected = if adding() {
        None
    } else {
        let chosen = selected_id();
        rows.iter()
            .find(|row| Some(&row.scanner.id) == chosen.as_ref())
            .or(rows.first())
            .cloned()
    };
    let latest = |id: &str| {
        live.scans
            .read()
            .iter()
            .find(|job| job.scanner_id == id)
            .cloned()
    };
    let current = selected
        .as_ref()
        .and_then(|selected| latest(&selected.scanner.id));
    let selected_key = selected.as_ref().map(|row| row.scanner.id.clone());
    // Each scanner's id, whether it's the one open, its row, and whether its
    // latest scan is running.
    let cards: Vec<(String, bool, ScannerView, bool)> = rows
        .iter()
        .map(|row| {
            let id = row.scanner.id.clone();
            let running = latest(&id).is_some_and(|job| job.status == ScanStatus::Running);
            let open = selected_key.as_ref() == Some(&id);
            (id, open, row.clone(), running)
        })
        .collect();
    let failure = scan_error().or(toggle_error());
    rsx! {
        div { id: "rom-scanners", class: "flex flex-col gap-4",
            SettingsSection {
                icon: FOLDER_SEARCH,
                title: t!("ROM scanners"),
                description: t!("Saved scans that turn ROM folders into games. Multi-disc games and playlists import as one game."),
                aside: rsx! {
                    Button {
                        id: "scanner-add",
                        size: ButtonSize::Sm,
                        icon: PLUS,
                        on_press: move |_| {
                            adding.set(true);
                            selected_id.set(None);
                        },
                        {t!("Add scanner")}
                    }
                },
                if cards.is_empty() {
                    p { class: "mt-3 rounded-control border border-dashed border-border p-5 text-sm text-fg-muted",
                        {t!("No ROM scanners yet. Add one for each ROM folder.")}
                    }
                } else {
                    StackedTable { class: "mt-3",
                        for (id, open, row, running) in cards {
                            ScannerCard {
                                key: "{id}",
                                selected: open,
                                row,
                                running,
                                snapshot: snapshot.clone(),
                                scan_pending: scan_pending(),
                                toggle_pending: toggle_pending(),
                                on_open: open_scanner,
                                on_scan: start_scan,
                                on_toggle: toggle,
                            }
                        }
                    }
                }
                if let Some(report) = failure {
                    p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
                }
            }
            if let Some(job) = current {
                LatestScan {
                    job,
                    snapshot: snapshot.clone(),
                    on_done: move |_| *reload.write() += 1,
                }
            }
            if adding() {
                ScannerEditor {
                    key: "{NEW_KEY}",
                    initial: empty_scanner(
                        &snapshot,
                        t!(note = "A new ROM scanner's name until it's renamed", "New scanner"),
                    ),
                    snapshot: snapshot.clone(),
                    on_saved: move |scanner: RomScanner| {
                        *reload.write() += 1;
                        // A new scanner's first run is a review (§6.1).
                        start_scan.call(scanner.id);
                    },
                    on_removed: move |_| adding.set(false),
                }
            } else if let Some(row) = selected {
                ScannerEditor {
                    key: "{row.scanner.id}",
                    initial: row.scanner.clone(),
                    row: row.clone(),
                    snapshot: snapshot.clone(),
                    on_saved: move |_| *reload.write() += 1,
                    on_removed: move |_| {
                        selected_id.set(None);
                        *reload.write() += 1;
                    },
                }
            }
            GlobalExclusions { reload }
        }
    }
}

/// One scanner of the list: a right click offers its row's actions.
#[component]
fn ScannerCard(
    row: ScannerView,
    snapshot: Snapshot,
    selected: bool,
    /// Its latest scan is running.
    running: bool,
    scan_pending: bool,
    toggle_pending: bool,
    on_open: EventHandler<String>,
    on_scan: EventHandler<String>,
    on_toggle: EventHandler<RomScanner>,
) -> Element {
    let interaction = use_interaction();
    let mut menu = use_signal(|| None::<Anchor>);
    let mut card = use_signal(|| None::<NodeHandle>);
    let scanner = row.scanner.clone();
    let id = scanner.id.clone();
    let scan_disabled = running || row.problem.is_some() || scan_pending;
    let platforms = row
        .platforms
        .iter()
        .map(|id| snapshot.platform_short(id))
        .collect::<Vec<_>>()
        .join(", ");
    let platforms = if platforms.is_empty() {
        "—".to_owned()
    } else {
        platforms
    };
    let profile_name = profile_options(&snapshot)
        .into_iter()
        .find(|option| {
            option.id == profile_ref(&scanner.profile.emulator_id, &scanner.profile.profile_id)
        })
        .map(|option| option.label);
    let launch = launch_mode_label(launch_mode(&scanner.launch_with));
    let scan_item = MenuItem {
        icon: Some(PLAY),
        disabled: scan_disabled,
        ..MenuItem::new("scan", t!(note = "Run the ROM scanner", "Scan"))
    };
    let edit_item = MenuItem {
        icon: Some(PENCIL),
        ..MenuItem::new("edit", t!("Edit scanner"))
    };
    let auto_item = MenuItem {
        icon: Some(REFRESH_CW),
        disabled: toggle_pending,
        ..MenuItem::new(
            "auto",
            if scanner.in_library_update {
                t!("Leave out of library updates")
            } else {
                t!("Include in library updates")
            },
        )
    };
    let items = vec![
        MenuEntry::Item(scan_item),
        MenuEntry::Item(edit_item),
        MenuEntry::Item(auto_item),
    ];
    let press = {
        let id = id.clone();
        move || on_open.call(id.clone())
    };
    let key_press = press.clone();
    let up_press = press.clone();
    rsx! {
        div {
            role: "listitem",
            "data-part": "scanner-row",
            "data-selected": flag(selected),
            class: "grid grid-cols-2 gap-x-4 gap-y-3 border-t border-border p-4 first:border-t-0 transition-colors duration-(--cs-motion-duration-fast) data-selected:bg-accent/10",
            onmounted: move |event: Event<MountedData>| card.set(event.data().downcast::<NodeHandle>().cloned()),
            oncontextmenu: move |event: MouseEvent| {
                menu.set(Some(crate::frame::pointer_anchor(card.peek().as_ref(), &event)));
            },
            if let Some(anchor) = menu() {
                Menu {
                    anchor,
                    items,
                    label: scanner.name.clone(),
                    on_close: move |_| menu.set(None),
                    on_action: {
                        let scanner = scanner.clone();
                        move |action: String| {
                            menu.set(None);
                            match action.as_str() {
                                "scan" => on_scan.call(scanner.id.clone()),
                                "edit" => on_open.call(scanner.id.clone()),
                                _ => on_toggle.call(RomScanner {
                                    in_library_update: !scanner.in_library_update,
                                    ..scanner.clone()
                                }),
                            }
                        }
                    },
                }
            }
            StackedCell { full: true,
                button {
                    r#type: "button",
                    id: "scanner-open-{id}",
                    "aria-pressed": if selected { "true" } else { "false" },
                    "data-hovered": interaction.hovered(),
                    "data-focus-visible": interaction.ring(),
                    "data-pressed": interaction.pressed(),
                    class: "rounded-[4px] text-left font-medium outline-none data-focus-visible:focus-ring data-hovered:text-accent-alt",
                    onfocus: move |_| interaction.focus(),
                    onblur: move |_| interaction.blur(),
                    onmouseenter: move |_| interaction.enter(),
                    onmouseleave: move |_| interaction.leave(),
                    onmousedown: move |_| interaction.down(),
                    onmouseup: move |_| interaction.up(),
                    onclick: move |_| press(),
                    onkeydown: move |event| {
                        interaction.key_down(&event, Keys::EnterAndSpace, key_press.clone());
                    },
                    onkeyup: move |event| interaction.key_up(&event, up_press.clone()),
                    "{scanner.name}"
                }
            }
            StackedCell {
                label: t!(note = "ROM scanners table column", "Folder"),
                full: true,
                span { class: "font-mono text-xs text-fg-muted",
                    {crate::flows::paths::format_path(&scanner.folder.to_string_lossy())}
                }
            }
            StackedCell {
                label: t!(note = "ROM scanners table column", "Platform"),
                class: "text-xs",
                "{platforms}"
            }
            StackedCell { label: t!(note = "ROM scanners table column", "Emulator · Profile"),
                if let Some(problem) = row.problem.clone() {
                    StatusDot { tone: DotTone::Danger, "{problem}" }
                } else if let Some(name) = profile_name {
                    "{name}"
                }
            }
            StackedCell {
                label: t!(note = "ROM scanners table column", "Launch with"),
                class: "text-xs text-fg-muted",
                "{launch}"
            }
            StackedCell { label: t!(note = "ROM scanners table column", "Games"),
                span { class: "font-mono", "{row.games}" }
            }
            StackedCell {
                label: t!(note = "ROM scanners table column", "Last scan"),
                class: "text-xs text-fg-muted",
                if running {
                    StatusDot { tone: DotTone::Warning, {t!("Scanning…")} }
                } else if let Some(ms) = row.last_scan_ms.filter(|ms| *ms != 0) {
                    {crate::library::date_time(ms)}
                } else {
                    {t!(note = "A scanner's last scan", "Never")}
                }
            }
            StackedCell {
                label: t!(
                    note = "ROM scanners table column: whether library updates run the scanner",
                    "Auto"
                ),
                Switch {
                    id: "scanner-auto-{id}",
                    size: SwitchSize::Sm,
                    "aria-label": t!("Include {name} in library update", name = scanner.name.clone()),
                    checked: scanner.in_library_update,
                    disabled: toggle_pending,
                    on_change: {
                        let scanner = scanner.clone();
                        move |on: bool| on_toggle.call(RomScanner { in_library_update: on, ..scanner.clone() })
                    },
                }
            }
            StackedCell { full: true, class: "text-right",
                Button {
                    id: "scanner-scan-{id}",
                    size: ButtonSize::Sm,
                    icon: PLAY,
                    disabled: scan_disabled,
                    "aria-label": t!("Scan {name}", name = scanner.name.clone()),
                    on_press: {
                        let id = id.clone();
                        move |_| on_scan.call(id.clone())
                    },
                    {t!(note = "Run the ROM scanner", "Scan")}
                }
            }
        }
    }
}

/// The latest scan of a scanner: progress, its review list or its outcome.
#[component]
fn LatestScan(job: ScanJob, snapshot: Snapshot, on_done: EventHandler<()>) -> Element {
    if job.status == ScanStatus::Review {
        let id = job.id.clone();
        return rsx! {
            ScanReview {
                key: "{id}",
                job,
                platforms: snapshot.platforms.clone(),
                on_done: move |_| on_done.call(()),
            }
        };
    }
    let text = match (&job.status, &job.summary) {
        (ScanStatus::Running, _) => match &job.progress {
            Some(progress) => tn!("Scanning… # file", "Scanning… # files", progress.files),
            None => t!("Scanning…"),
        },
        (ScanStatus::Done, Some(summary)) => t!(
            "Last scan: {added} added, {updated} updated, {missing} missing.",
            added = summary.added,
            updated = summary.updated,
            missing = summary.missing
        ),
        (ScanStatus::Cancelled, _) => t!("Scan cancelled."),
        _ => match &job.error {
            Some(error) => t!("Scan failed: {error}", error = error.clone()),
            None => t!("Scan failed: unknown error"),
        },
    };
    rsx! {
        p { role: "status", "data-part": "scan-status", class: "text-sm text-fg-muted", "{text}" }
    }
}
