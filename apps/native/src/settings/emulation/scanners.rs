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
use caskade_ui_i18n::{format, t, tn};
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

/// The key of a form that adds rather than edits.
const NEW_KEY: &str = "new";

/// An exclusion pattern, as the placeholder shows it.
const EXAMPLE_PATTERN: &str = ">*.sav";

/// Saves an editor's draft; whether it saved.
type Save = Rc<dyn Fn() -> Pin<Box<dyn Future<Output = bool>>>>;

/// Extensions of playlists, which absorb the files they list.
const PLAYLIST_TYPES: [&str; 3] = ["m3u", "cue", "gdi"];

/// The exclusion syntax, with what each example skips.
fn exclusion_syntax() -> [(&'static str, String); 4] {
    [
        ("bios/", t!("This folder or file in the scan folder")),
        (
            ">*.txt",
            t!("This name in any folder; * stands for any text"),
        ),
        (
            "?^beta/.*",
            t!("A regular expression, from the start of the path"),
        ),
        (
            ">?\\(Beta\\)",
            t!("A regular expression, anywhere in the path"),
        ),
    ]
}

/// A scanner's switches.
#[derive(Clone, Copy, PartialEq)]
enum Toggle {
    Recursive,
    ScanArchives,
    SkipCloudPlaceholders,
    InLibraryUpdate,
}

const TOGGLES: [Toggle; 4] = [
    Toggle::Recursive,
    Toggle::ScanArchives,
    Toggle::SkipCloudPlaceholders,
    Toggle::InLibraryUpdate,
];

impl Toggle {
    fn id(self) -> &'static str {
        match self {
            Self::Recursive => "scanner-recursive",
            Self::ScanArchives => "scanner-archives",
            Self::SkipCloudPlaceholders => "scanner-skip-cloud",
            Self::InLibraryUpdate => "scanner-library-update",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Recursive => FOLDER_TREE,
            Self::ScanArchives => ARCHIVE,
            Self::SkipCloudPlaceholders => CLOUD_OFF,
            Self::InLibraryUpdate => REFRESH_CW,
        }
    }

    fn label(self) -> String {
        match self {
            Self::Recursive => t!("Scan subfolders"),
            Self::ScanArchives => t!("Look inside archives"),
            Self::SkipCloudPlaceholders => t!("Skip cloud-only files"),
            Self::InLibraryUpdate => t!("Include in library update"),
        }
    }

    fn description(self) -> String {
        match self {
            Self::Recursive => t!("Look in every folder below the scan folder."),
            Self::ScanArchives => {
                t!("Find ROMs inside zip and 7z files the profile can't open directly.")
            }
            Self::SkipCloudPlaceholders => {
                t!("Don't download OneDrive, Dropbox or iCloud placeholders to scan them.")
            }
            Self::InLibraryUpdate => {
                t!("Import new ROMs whenever the library updates, without a review.")
            }
        }
    }

    fn get(self, scanner: &RomScanner) -> bool {
        match self {
            Self::Recursive => scanner.recursive,
            Self::ScanArchives => scanner.scan_archives,
            Self::SkipCloudPlaceholders => scanner.skip_cloud_placeholders,
            Self::InLibraryUpdate => scanner.in_library_update,
        }
    }

    fn set(self, scanner: &mut RomScanner, on: bool) {
        match self {
            Self::Recursive => scanner.recursive = on,
            Self::ScanArchives => scanner.scan_archives = on,
            Self::SkipCloudPlaceholders => scanner.skip_cloud_placeholders = on,
            Self::InLibraryUpdate => scanner.in_library_update = on,
        }
    }
}

/// What a scanner's games launch with, by its `type`.
const LAUNCH_MODES: [&str; 4] = ["platform_default", "profile", "ask_profile", "ask_emulator"];

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

fn platform_short(snapshot: &Snapshot, id: &str) -> String {
    snapshot
        .platforms
        .iter()
        .find(|platform| platform.platform.id == id)
        .map_or_else(|| id.to_owned(), |platform| platform.platform.short.clone())
}

fn platform_name(snapshot: &Snapshot, id: &str) -> String {
    snapshot
        .platforms
        .iter()
        .find(|platform| platform.platform.id == id)
        .map_or_else(|| id.to_owned(), |platform| platform.platform.name.clone())
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

fn at(ms: i64) -> String {
    format::date_time(crate::library::local_date_time(ms))
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
        .map(|id| platform_short(&snapshot, id))
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
                    {at(ms)}
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

/// One scanner's settings, saved with its Save button.
#[component]
fn ScannerEditor(
    initial: RomScanner,
    /// The saved scanner's row; none for a new one.
    #[props(default)]
    row: Option<ScannerView>,
    snapshot: Snapshot,
    on_saved: EventHandler<RomScanner>,
    on_removed: EventHandler<()>,
) -> Element {
    let emulation = use_emulation();
    let mut draft = use_signal(|| initial.clone());
    let mut base = use_signal(|| initial.clone());
    // A review or the include switch changes the saved scanner; follow it
    // unless the draft has edits of its own.
    use_effect(use_reactive!(|initial| {
        if *base.peek() != initial {
            if *draft.peek() == *base.peek() {
                draft.set(initial.clone());
            }
            base.set(initial);
        }
    }));
    let mut saving = use_signal(|| false);
    let mut save_error = use_signal(|| None::<ErrorReport>);
    let mut confirming = use_signal(|| false);
    let mut removing = use_signal(|| false);
    let mut remove_error = use_signal(|| None::<ErrorReport>);
    let created = initial.id.is_empty();
    let now = draft();
    let options = profile_options(&snapshot);
    let profile = options
        .iter()
        .find(|option| option.id == profile_ref(&now.profile.emulator_id, &now.profile.profile_id))
        .cloned();
    let title = if created {
        t!("New ROM scanner")
    } else if now.name.is_empty() {
        t!("Unnamed scanner")
    } else {
        now.name.clone()
    };
    let dirty = now != base();
    let save: Save = {
        let emulation = emulation.clone();
        Rc::new(move || {
            let emulation = emulation.clone();
            let next = draft.peek().clone();
            Box::pin(async move {
                saving.set(true);
                save_error.set(None);
                let saved = change(async move { emulation.save_scanner(next).await }).await;
                saving.set(false);
                match saved {
                    Ok(scanner) => {
                        on_saved.call(scanner);
                        true
                    }
                    Err(report) => {
                        save_error.set(Some(report));
                        false
                    }
                }
            }) as Pin<Box<dyn Future<Output = bool>>>
        })
    };
    // The scanner waits for its Save button (#43): leaving the section asks
    // first.
    let has_profile = profile.is_some();
    use_unsaved_changes(
        dirty,
        UnsavedForm {
            label: title.clone(),
            save: {
                let save = save.clone();
                Rc::new(move || {
                    let save = save.clone();
                    Box::pin(async move { has_profile && save().await })
                        as Pin<Box<dyn Future<Output = bool>>>
                })
            },
            discard: Rc::new(move || {
                let mut draft = draft;
                draft.set(base.peek().clone());
            }),
        },
    );
    let remove = {
        let emulation = emulation.clone();
        let id = initial.id.clone();
        move |_| {
            removing.set(true);
            remove_error.set(None);
            let emulation = emulation.clone();
            let id = id.clone();
            spawn(async move {
                let removed = change(async move { emulation.delete_scanner(&id).await }).await;
                removing.set(false);
                match removed {
                    Ok(()) => {
                        confirming.set(false);
                        on_removed.call(());
                    }
                    Err(report) => remove_error.set(Some(report)),
                }
            });
        }
    };
    let profile_choices: Vec<SelectOption> = options
        .iter()
        .map(|option| SelectOption::new(option.id.clone(), option.label.clone()))
        .collect();
    let from_profile = match &profile {
        Some(profile) if !profile.platforms.is_empty() => t!(
            "From profile ({platforms})",
            platforms = profile
                .platforms
                .iter()
                .map(|id| platform_short(&snapshot, id))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => t!("From profile"),
    };
    let platform_choices: Vec<SelectOption> =
        std::iter::once(SelectOption::new("profile", from_profile))
            .chain(
                profile
                    .iter()
                    .flat_map(|profile| profile.platforms.iter())
                    .map(|id| SelectOption::new(id.clone(), platform_name(&snapshot, id))),
            )
            .collect();
    let launch_choices: Vec<SelectOption> = LAUNCH_MODES
        .iter()
        .map(|mode| SelectOption::new(*mode, launch_mode_label(mode)))
        .collect();
    let playlists = profile.as_ref().is_some_and(|profile| {
        profile
            .extensions
            .iter()
            .any(|extension| PLAYLIST_TYPES.contains(&extension.as_str()))
    });
    let flatpak_hint = row.as_ref().and_then(|row| row.flatpak_hint.clone());
    let problem = row.as_ref().and_then(|row| row.problem.clone());
    let submit = {
        let save = save.clone();
        move |_| {
            let save = save.clone();
            spawn(async move {
                save().await;
            });
        }
    };
    rsx! {
        SettingsSection {
            icon: FOLDER_SEARCH,
            title: title.clone(),
            description: t!("The profile decides which files match. Imported games launch with the choice below."),
            aside: (!created).then(|| rsx! {
                Button {
                    id: "scanner-remove",
                    variant: ButtonVariant::Danger,
                    size: ButtonSize::Sm,
                    icon: TRASH,
                    disabled: removing(),
                    on_press: move |_| {
                        remove_error.set(None);
                        confirming.set(true);
                    },
                    {t!(note = "Remove the ROM scanner", "Remove…")}
                }
            }),
            div { class: "mt-3 grid gap-4",
                TextField {
                    id: "scanner-name",
                    label: t!("Scanner name"),
                    value: now.name.clone(),
                    on_input: move |name: String| draft.write().name = name,
                }
                PathField {
                    id: "scanner-folder",
                    label: t!("ROM folder"),
                    value: now.folder.to_string_lossy().into_owned(),
                    on_change: move |folder: String| draft.write().folder = PathBuf::from(folder),
                }
                Select {
                    id: "scanner-profile",
                    label: t!("Emulator profile"),
                    placeholder: t!("Choose a profile"),
                    options: profile_choices,
                    value: profile.as_ref().map(|profile| profile.id.clone()),
                    disabled: options.is_empty(),
                    on_change: {
                        let options = options.clone();
                        move |id: String| {
                            let Some(next) = options.iter().find(|option| option.id == id) else {
                                return;
                            };
                            let mut scanner = draft.peek().clone();
                            let mode = launch_mode(&scanner.launch_with);
                            scanner.platform = None;
                            scanner.profile = ProfileRef {
                                emulator_id: next.emulator_id.clone(),
                                profile_id: next.profile_id.clone(),
                            };
                            scanner.launch_with = launch_with(mode, &scanner);
                            draft.set(scanner);
                        }
                    },
                }
                Select {
                    id: "scanner-platform",
                    label: t!(note = "A ROM scanner's platform", "Platform"),
                    options: platform_choices,
                    value: Some(now.platform.clone().unwrap_or_else(|| "profile".to_owned())),
                    on_change: move |platform: String| {
                        draft.write().platform = (platform != "profile").then_some(platform);
                    },
                }
                Select {
                    id: "scanner-launch",
                    label: t!("Games launch with"),
                    options: launch_choices,
                    value: Some(launch_mode(&now.launch_with).to_owned()),
                    on_change: move |mode: String| {
                        let next = launch_with(&mode, &draft.peek());
                        draft.write().launch_with = next;
                    },
                }
            }
            if options.is_empty() {
                p { class: "mt-3 text-sm text-fg-muted",
                    {t!("Add an emulator with a profile first; its file types decide what a scanner finds.")}
                }
            }
            if let Some(profile) = profile.clone() {
                div { class: "mt-3 flex flex-wrap items-center gap-1.5 text-xs text-fg-muted",
                    {t!(note = "Followed by the file extensions the scanner's profile matches", "Matches")}
                    for extension in profile.extensions.iter() {
                        span {
                            key: "{extension}",
                            "data-part": "extension",
                            class: "rounded-control border border-border px-1.5 py-0.5 font-mono",
                            {format!(".{extension}")}
                        }
                    }
                    if playlists {
                        span {
                            {format!("· {}", t!("Playlists absorb the files they list, so disc tracks aren't separate games."))}
                        }
                    }
                }
            }
            if let Some(hint) = flatpak_hint {
                p {
                    "data-part": "flatpak-hint",
                    class: "mt-3 flex gap-2 rounded-control border border-accent-alt/30 bg-accent-alt/5 p-3 text-sm text-fg-muted",
                    Svg { icon: INFO, class: "mt-0.5 size-4 shrink-0 text-accent-alt" }
                    "{hint}"
                }
            }
            if let Some(problem) = problem {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{problem}" }
            }
            div { class: "mt-4",
                for (id, toggle) in TOGGLES.map(|toggle| (toggle.id(), toggle)) {
                    SettingRow {
                        key: "{id}",
                        icon: toggle.icon(),
                        label: toggle.label(),
                        description: toggle.description(),
                        Switch {
                            id: "{id}",
                            checked: toggle.get(&now),
                            on_change: move |on: bool| toggle.set(&mut draft.write(), on),
                        }
                    }
                }
            }
            div { class: "mt-4 grid gap-4",
                div {
                    h3 { class: "text-sm font-medium", {t!("Exclusions")} }
                    p { class: "text-xs text-fg-muted",
                        {t!("Files and folders this scanner always skips. Unticking a game in a review adds it here.")}
                    }
                    ExclusionList {
                        patterns: now.exclusions.clone(),
                        on_change: move |patterns: Vec<String>| draft.write().exclusions = patterns,
                    }
                }
                ExclusionSyntax {}
            }
            if let Some(report) = save_error() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
            }
            if confirming() {
                ConfirmDialog {
                    title: t!("Remove {scanner}?", scanner = title.clone()),
                    confirm_label: t!(note = "Confirms removing a ROM scanner", "Remove"),
                    pending_label: t!("Removing…"),
                    pending: removing(),
                    on_confirm: remove,
                    on_close: move |_| confirming.set(false),
                    p {
                        {t!("Its games stay in your library as not installed, because Caskade no longer looks for their ROMs. Your ROM files aren't touched.")}
                    }
                    if let Some(report) = remove_error() {
                        FailureNotice { error: report, variant: NoticeVariant::Inline }
                    }
                }
            }
            div { class: "mt-4 flex items-center justify-end gap-2",
                div { class: "mr-auto",
                    UnsavedHint { dirty }
                }
                Button {
                    id: "scanner-save",
                    variant: ButtonVariant::Primary,
                    disabled: saving() || profile.is_none(),
                    on_press: submit,
                    if saving() {
                        {t!("Saving…")}
                    } else if created {
                        {t!("Save and scan")}
                    } else {
                        {t!("Save scanner")}
                    }
                }
            }
        }
    }
}

/// Exclusion patterns with remove buttons and a field to add one.
#[component]
fn ExclusionList(
    /// Every scanner's list rather than one scanner's.
    #[props(default)]
    global: bool,
    patterns: Vec<String>,
    on_change: EventHandler<Vec<String>>,
    #[props(default)] disabled: bool,
) -> Element {
    let mut pattern = use_signal(String::new);
    let prefix = if global {
        "global-exclusion"
    } else {
        "exclusion"
    };
    let mut add = {
        let patterns = patterns.clone();
        move || {
            let next = pattern.peek().trim().to_owned();
            if !next.is_empty() && !patterns.contains(&next) {
                let mut all = patterns.clone();
                all.push(next);
                on_change.call(all);
            }
            pattern.set(String::new());
        }
    };
    let mut add_on_enter = add.clone();
    let can_add = !disabled && !pattern().trim().is_empty();
    let (list_label, new_label) = if global {
        (t!("Global exclusions"), t!("New global exclusion pattern"))
    } else {
        (t!("Exclusions"), t!("New exclusion pattern"))
    };
    // Each pattern's place, the pattern and its remove button's label.
    let entries: Vec<(usize, String, String)> = patterns
        .iter()
        .enumerate()
        .map(|(index, exclusion)| {
            let label = if global {
                t!(
                    "Remove global exclusion {pattern}",
                    pattern = exclusion.clone()
                )
            } else {
                t!("Remove exclusion {pattern}", pattern = exclusion.clone())
            };
            (index, exclusion.clone(), label)
        })
        .collect();
    rsx! {
        ul {
            "aria-label": list_label,
            class: "mt-2 flex flex-col gap-1",
            for (index, exclusion, label) in entries {
                li {
                    key: "{exclusion}",
                    "data-part": "exclusion",
                    class: "flex items-center gap-2 rounded-control border border-border bg-surface-base/60 py-1 pr-1 pl-3",
                    code { class: "flex-1 truncate font-mono text-xs", "{exclusion}" }
                    RemoveExclusion {
                        id: format!("{prefix}-remove-{index}"),
                        label,
                        disabled,
                        on_press: {
                            let (patterns, exclusion) = (patterns.clone(), exclusion.clone());
                            move |_| {
                                on_change.call(
                                    patterns.iter().filter(|other| **other != exclusion).cloned().collect(),
                                )
                            }
                        },
                    }
                }
            }
            if patterns.is_empty() {
                li { class: "text-xs text-fg-muted italic", {t!("No exclusions.")} }
            }
        }
        div { class: "mt-2",
            TextField {
                id: "{prefix}-new",
                "aria-label": new_label,
                mono: true,
                placeholder: EXAMPLE_PATTERN.to_owned(),
                value: pattern(),
                on_input: move |next: String| pattern.set(next),
                on_submit: move |_| {
                    if can_add {
                        add_on_enter();
                    }
                },
                action: rsx! {
                    Button {
                        id: "{prefix}-add",
                        icon: PLUS,
                        "aria-label": global.then(|| t!("Add global exclusion")),
                        disabled: !can_add,
                        on_press: move |_| add(),
                        {t!(note = "Add the exclusion pattern", "Add")}
                    }
                },
            }
        }
    }
}

/// An exclusion's remove button.
#[component]
fn RemoveExclusion(
    id: String,
    label: String,
    disabled: bool,
    on_press: EventHandler<()>,
) -> Element {
    let interaction = use_interaction();
    let press = move || {
        if !disabled {
            on_press.call(());
        }
    };
    rsx! {
        button {
            r#type: "button",
            id: "{id}",
            "aria-label": "{label}",
            "aria-disabled": flag(disabled),
            "data-disabled": flag(disabled),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-pressed": interaction.pressed(),
            class: "grid size-7 place-items-center rounded-[6px] text-fg-subtle outline-none data-focus-visible:focus-ring data-hovered:bg-surface-hover data-hovered:text-danger",
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onclick: move |_| press(),
            onkeydown: move |event| {
                interaction.key_down(&event, Keys::EnterAndSpace, press);
            },
            onkeyup: move |event| interaction.key_up(&event, press),
            Svg { icon: X, class: "size-4" }
        }
    }
}

/// How to write an exclusion pattern.
#[component]
fn ExclusionSyntax() -> Element {
    rsx! {
        aside { class: "rounded-control border border-accent-alt/25 bg-accent-alt/5 p-3 text-xs text-fg-muted",
            div { class: "mb-2 font-mono tracking-wider text-accent-alt uppercase",
                {t!(note = "How to write an exclusion pattern", "Patterns")}
            }
            p { class: "mb-2",
                {t!("Type a path to skip that folder or file. Start with > to skip a name wherever it turns up: >*.txt skips every text file.")}
            }
            dl { class: "grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5",
                for (pattern, meaning) in exclusion_syntax() {
                    div { key: "{pattern}", class: "contents",
                        dt {
                            code { class: "font-mono text-fg", "{pattern}" }
                        }
                        dd { "{meaning}" }
                    }
                }
            }
        }
    }
}

/// Exclusions every scanner applies (§6.3); saved as soon as they change.
#[component]
fn GlobalExclusions(reload: Signal<u64>) -> Element {
    let emulation = use_emulation();
    let live = use_live();
    let loaded = {
        let emulation = emulation.clone();
        use_resource(move || {
            let _ = (live.changed)();
            let _ = reload();
            let emulation = emulation.clone();
            change(async move { emulation.exclusions().await })
        })
    };
    // What the last save returned, until the list is read again.
    let mut saved = use_signal(|| None::<Vec<String>>);
    use_effect(move || {
        let _ = loaded.read();
        if saved.peek().is_some() {
            saved.set(None);
        }
    });
    // The patterns being saved, shown while they are.
    let mut pending = use_signal(|| None::<Vec<String>>);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let save = move |patterns: Vec<String>| {
        pending.set(Some(patterns.clone()));
        failure.set(None);
        let emulation = emulation.clone();
        spawn(async move {
            let result = change(async move { emulation.set_exclusions(patterns).await }).await;
            match result {
                Ok(next) => saved.set(Some(next)),
                Err(report) => failure.set(Some(report)),
            }
            pending.set(None);
        });
    };
    let state = match &*loaded.read() {
        None => None,
        Some(Ok(patterns)) => Some(Ok(patterns.clone())),
        Some(Err(report)) => Some(Err(report.clone())),
    };
    rsx! {
        SettingsSection {
            id: "global-exclusions",
            icon: FILTER_X,
            title: t!("Global exclusions"),
            description: t!("Files and folders every scanner skips, from its next scan."),
            match state {
                Some(Err(report)) => rsx! {
                    p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
                },
                None => rsx! {
                    p { role: "status", class: "mt-3 text-sm text-fg-muted", {t!("Loading global exclusions…")} }
                },
                Some(Ok(patterns)) => rsx! {
                    div { class: "mt-4 grid gap-4",
                        div {
                            ExclusionList {
                                global: true,
                                patterns: pending().or(saved()).unwrap_or(patterns),
                                on_change: save,
                                disabled: pending().is_some(),
                            }
                        }
                        ExclusionSyntax {}
                    }
                },
            }
            if let Some(report) = failure() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
            }
        }
    }
}

/// One game of the review list as the user is shaping it; `key` stays
/// stable across edits.
#[derive(Clone, PartialEq)]
struct Item {
    key: String,
    roms: Vec<String>,
    names: BTreeMap<String, Option<String>>,
    include: bool,
    title: String,
    platform: String,
    regions: Vec<String>,
}

fn items(job: &ScanJob) -> Vec<Item> {
    job.review
        .iter()
        .map(|game| Item {
            key: game
                .roms
                .first()
                .map_or_else(|| game.title.clone(), |rom| rom.path.clone()),
            roms: game.roms.iter().map(|rom| rom.path.clone()).collect(),
            names: game
                .roms
                .iter()
                .map(|rom| (rom.path.clone(), rom.name.clone()))
                .collect(),
            include: true,
            title: game.title.clone(),
            platform: game.platform.clone(),
            regions: game.regions.clone(),
        })
        .collect()
}

/// The manual-scan review list: untick, fix titles, platforms and regions
/// in bulk, merge or split multi-disc games. The choices are stored, so
/// later automatic scans agree.
#[component]
fn ScanReview(job: ScanJob, platforms: Vec<PlatformView>, on_done: EventHandler<()>) -> Element {
    let emulation = use_emulation();
    let live = use_live();
    let mut rows = use_signal(|| items(&job));
    let mut selected = use_signal(BTreeSet::<String>::new);
    let mut bulk_region = use_signal(String::new);
    let mut submitting = use_signal(|| false);
    let mut cancelling = use_signal(|| false);
    let mut submit_error = use_signal(|| None::<ErrorReport>);
    let mut cancel_error = use_signal(|| None::<ErrorReport>);
    let done = move |next: ScanJob| {
        let mut scans = live.scans;
        merge_scan(&mut scans.write(), next);
        on_done.call(());
    };
    let submit = {
        let emulation = emulation.clone();
        let (id, revision) = (job.id.clone(), job.revision);
        move |_| {
            submitting.set(true);
            submit_error.set(None);
            let choices: Vec<ReviewChoice> = rows
                .peek()
                .iter()
                .map(|row| ReviewChoice {
                    roms: row.roms.clone(),
                    include: row.include,
                    title: row.title.trim().to_owned(),
                    platform: row.platform.clone(),
                    regions: row.regions.clone(),
                })
                .collect();
            let emulation = emulation.clone();
            let id = id.clone();
            spawn(async move {
                let result =
                    change(async move { emulation.submit_review(&id, revision, choices).await })
                        .await;
                submitting.set(false);
                match result {
                    Ok(next) => done(next),
                    Err(report) => submit_error.set(Some(report)),
                }
            });
        }
    };
    let cancel = {
        let emulation = emulation.clone();
        let id = job.id.clone();
        move |_| {
            cancelling.set(true);
            cancel_error.set(None);
            let emulation = emulation.clone();
            let id = id.clone();
            spawn(async move {
                let result = change(async move { emulation.cancel_scan(&id) }).await;
                cancelling.set(false);
                match result {
                    Ok(next) => done(next),
                    Err(report) => cancel_error.set(Some(report)),
                }
            });
        }
    };
    let all = rows();
    let chosen = selected();
    let picked: Vec<Item> = all
        .iter()
        .filter(|row| chosen.contains(&row.key))
        .cloned()
        .collect();
    let merge = {
        let picked = picked.clone();
        move |_| {
            let Some(first) = picked.first().cloned() else {
                return;
            };
            let rest: BTreeSet<String> = picked[1..].iter().map(|item| item.key.clone()).collect();
            let next: Vec<Item> = rows
                .peek()
                .iter()
                .filter(|row| !rest.contains(&row.key))
                .map(|row| {
                    if row.key == first.key {
                        Item {
                            roms: picked.iter().flat_map(|item| item.roms.clone()).collect(),
                            names: picked.iter().flat_map(|item| item.names.clone()).collect(),
                            ..row.clone()
                        }
                    } else {
                        row.clone()
                    }
                })
                .collect();
            rows.set(next);
            selected.set(BTreeSet::new());
        }
    };
    let split = move |item: Item| {
        let next: Vec<Item> = rows
            .peek()
            .iter()
            .flat_map(|row| {
                if row.key == item.key {
                    row.roms
                        .iter()
                        .map(|rom| Item {
                            key: rom.clone(),
                            roms: vec![rom.clone()],
                            ..row.clone()
                        })
                        .collect::<Vec<_>>()
                } else {
                    vec![row.clone()]
                }
            })
            .collect();
        rows.set(next);
    };
    let included = all.iter().filter(|row| row.include).count();
    let busy = submitting() || cancelling();
    let untitled = all
        .iter()
        .any(|row| row.include && row.title.trim().is_empty());
    let platform_options: Vec<SelectOption> = platforms
        .iter()
        .map(|platform| {
            SelectOption::new(platform.platform.id.clone(), platform.platform.name.clone())
        })
        .collect();
    let failure = submit_error().or(cancel_error());
    // Each game's place, key and whether it's selected.
    let games: Vec<(usize, String, bool, Item)> = all
        .iter()
        .enumerate()
        .map(|(index, row)| {
            (
                index,
                row.key.clone(),
                chosen.contains(&row.key),
                row.clone(),
            )
        })
        .collect();
    rsx! {
        SettingsSection {
            id: "scan-review",
            icon: LIST_CHECKS,
            title: t!("Review scan results"),
            description: t!("Untick what isn't a game, fix titles, platforms and regions, and merge or split discs. Your choices are kept for later automatic scans."),
            div { class: "mt-3 space-y-3",
                for warning in job.warnings.iter() {
                    p { key: "{warning}", class: "text-xs text-warning", "{warning}" }
                }
                if all.is_empty() {
                    p { class: "rounded-control border border-dashed border-border p-5 text-sm text-fg-muted",
                        {t!("No ROMs matched this scanner's profile.")}
                    }
                } else {
                    div {
                        "data-part": "bulk",
                        class: "flex flex-wrap items-end gap-2 rounded-control border border-border bg-surface-overlay/40 p-3",
                        Checkbox {
                            id: "review-select-all",
                            label: t!("Select all"),
                            checked: picked.len() == all.len(),
                            indeterminate: !picked.is_empty() && picked.len() < all.len(),
                            on_change: move |on: bool| {
                                let next = if on {
                                    rows.peek().iter().map(|row| row.key.clone()).collect()
                                } else {
                                    BTreeSet::new()
                                };
                                selected.set(next);
                            },
                        }
                        span { class: "text-xs text-fg-muted",
                            {t!(note = "How many games of the scan review are selected", "{0} selected", 0 = picked.len())}
                        }
                        Button {
                            id: "review-merge",
                            size: ButtonSize::Sm,
                            icon: COMBINE,
                            disabled: picked.len() < 2,
                            on_press: merge,
                            {t!("Merge into one game")}
                        }
                        Select {
                            id: "review-platform",
                            "aria-label": t!("Set platform of selected games"),
                            placeholder: t!("Set platform…"),
                            options: platform_options.clone(),
                            value: None,
                            disabled: picked.is_empty(),
                            on_change: move |platform: String| {
                                if platform.is_empty() {
                                    return;
                                }
                                let chosen = selected.peek().clone();
                                for row in rows.write().iter_mut() {
                                    if chosen.contains(&row.key) {
                                        row.platform = platform.clone();
                                    }
                                }
                            },
                        }
                        div { class: "w-44",
                            TextField {
                                id: "review-regions",
                                "aria-label": t!("Regions for selected games"),
                                placeholder: t!(
                                    note = "Regions are comma-separated region codes; keep the example code",
                                    "Regions, e.g. usa"
                                ),
                                value: bulk_region(),
                                on_input: move |next: String| bulk_region.set(next),
                            }
                        }
                        Button {
                            id: "review-set-regions",
                            size: ButtonSize::Sm,
                            disabled: picked.is_empty(),
                            on_press: move |_| {
                                let regions: Vec<String> = bulk_region
                                    .peek()
                                    .split(',')
                                    .map(|region| region.trim().to_owned())
                                    .filter(|region| !region.is_empty())
                                    .collect();
                                let chosen = selected.peek().clone();
                                for row in rows.write().iter_mut() {
                                    if chosen.contains(&row.key) {
                                        row.regions = regions.clone();
                                    }
                                }
                            },
                            {t!("Set regions")}
                        }
                    }
                    ul { class: "space-y-2",
                        for (index, key, picked, row) in games {
                            ReviewGame {
                                key: "{key}",
                                index,
                                selected: picked,
                                options: platform_options.clone(),
                                on_select: {
                                    let key = key.clone();
                                    move |on: bool| {
                                        let mut chosen = selected.write();
                                        if on {
                                            chosen.insert(key.clone());
                                        } else {
                                            chosen.remove(&key);
                                        }
                                    }
                                },
                                on_update: move |next: Item| {
                                    if let Some(slot) = rows.write().iter_mut().find(|row| row.key == next.key) {
                                        *slot = next;
                                    }
                                },
                                on_split: split,
                                item: row,
                            }
                        }
                    }
                }
                if let Some(report) = failure {
                    p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
                }
                div { class: "flex justify-end gap-2",
                    Button { id: "review-discard", disabled: busy, on_press: cancel, {t!("Discard review")} }
                    Button {
                        id: "review-import",
                        variant: ButtonVariant::Primary,
                        disabled: busy || untitled,
                        on_press: submit,
                        if submitting() {
                            {t!("Importing…")}
                        } else {
                            {tn!("Import # game", "Import # games", included)}
                        }
                    }
                }
            }
        }
    }
}

/// One game of the review list.
#[component]
fn ReviewGame(
    item: Item,
    index: usize,
    selected: bool,
    options: Vec<SelectOption>,
    on_select: EventHandler<bool>,
    on_update: EventHandler<Item>,
    on_split: EventHandler<Item>,
) -> Element {
    let first = item.roms.first().cloned().unwrap_or_default();
    let roms: Vec<String> = item
        .roms
        .iter()
        .map(|rom| match item.names.get(rom) {
            Some(Some(name)) if !name.is_empty() => format!("{rom} · {name}"),
            _ => rom.clone(),
        })
        .collect();
    rsx! {
        li {
            "data-part": "review-game",
            "data-included": if item.include { "true" } else { "false" },
            class: "grid gap-3 rounded-control border border-border p-3 data-[included=false]:opacity-60 md:grid-cols-[auto_minmax(0,1fr)_14rem_auto]",
            div { class: "flex flex-col gap-2",
                Checkbox {
                    id: "review-select-{index}",
                    "aria-label": t!("Select {title}", title = item.title.clone()),
                    checked: selected,
                    on_change: move |on: bool| on_select.call(on),
                }
            }
            div { class: "min-w-0 space-y-2",
                TextField {
                    id: "review-title-{index}",
                    "aria-label": t!("Title of {rom}", rom = first),
                    value: item.title.clone(),
                    on_input: {
                        let item = item.clone();
                        move |title: String| on_update.call(Item { title, ..item.clone() })
                    },
                }
                ol {
                    "aria-label": t!("ROMs of {title}", title = item.title.clone()),
                    class: "space-y-0.5",
                    for (rom, line) in item.roms.iter().zip(roms) {
                        li { key: "{rom}", class: "break-all font-mono text-xs text-fg-muted", "{line}" }
                    }
                }
                if !item.regions.is_empty() {
                    p { class: "text-xs text-fg-muted",
                        {t!("Regions: {regions}", regions = item.regions.join(", "))}
                    }
                }
            }
            Select {
                id: "review-platform-{index}",
                "aria-label": t!("Platform of {title}", title = item.title.clone()),
                options,
                value: Some(item.platform.clone()),
                on_change: {
                    let item = item.clone();
                    move |platform: String| on_update.call(Item { platform, ..item.clone() })
                },
            }
            div { class: "flex flex-col items-end gap-2",
                Checkbox {
                    id: "review-include-{index}",
                    label: t!(note = "Import this game from the scan", "Import"),
                    checked: item.include,
                    on_change: {
                        let item = item.clone();
                        move |include: bool| on_update.call(Item { include, ..item.clone() })
                    },
                }
                if item.roms.len() > 1 {
                    Button {
                        id: "review-split-{index}",
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Ghost,
                        icon: SPLIT,
                        on_press: {
                            let item = item.clone();
                            move |_| on_split.call(item.clone())
                        },
                        {t!("Split discs")}
                    }
                }
            }
        }
    }
}
