//! Settings → Emulation (#115, #116, #105), as the webview's
//! `EmulatorSettings` and `emulators/`: the overview (counts, Detect
//! emulators, Add emulator, Scan all), Playing (`core.emulation`), and one
//! section at a time, kept in the address as `?section=`: the configured
//! emulators and their profiles, Get emulators, platforms and their
//! defaults, ROM scanners, and RomM servers.

use caskade_app::events::Event;
use caskade_core::error::ErrorReport;
use caskade_ui_data::emulation::{
    DetectJob, Emulation, EmulationCatalog, EmulationOverview, EmulatorView, PlatformView, ScanJob,
    ScanSummary, SetupJob,
};
use caskade_ui_data::romm::Romm;
use caskade_ui_i18n::{format, t, tn, tp};
use dioxus_native::prelude::*;
use serde_json::Value;
use tokio::sync::broadcast;

use super::state::{SettingsStatus, use_settings_section};
use crate::data::{DataSource, change};
use crate::router::{Route, Router};
use crate::ui::icons::{
    CLOUD_DOWNLOAD, CPU, FOLDER_SEARCH, GAMEPAD, LAYERS, MAXIMIZE, MONITOR_PLAY, PLUS, REFRESH_CW,
    SCAN_SEARCH, SERVER, SHIELD_CHECK,
};
use crate::ui::{
    Button, ButtonVariant, FailureNotice, Icon, Segment, SegmentedControl, SettingRow,
    SettingsSection, Svg, Switch,
};

mod detection;
mod emulators;
mod fields;
mod platforms;
mod romm;
mod romm_editor;
mod scanners;
mod setup;

pub(crate) use romm::server_id as romm_server_id;

/// The app's emulation: a test's, else the running app's; `None` without
/// an app (fixtures mode).
fn find_emulation() -> Option<Emulation> {
    try_use_context::<Emulation>().or_else(|| crate::shell::services().map(Emulation::of))
}

/// The emulation the panel was opened with.
pub fn use_emulation() -> Emulation {
    use_context::<Emulation>()
}

/// The app's RomM servers: a test's, else the running app's; `None` where
/// RomM didn't start.
pub fn use_romm() -> Option<Romm> {
    try_use_context::<Romm>().or_else(|| crate::shell::services().and_then(Romm::of))
}

/// What the sections show, read together and again on every emulation
/// change.
#[derive(Clone, PartialEq)]
pub struct Snapshot {
    pub catalog: EmulationCatalog,
    pub overview: EmulationOverview,
    pub emulators: Vec<EmulatorView>,
    pub platforms: Vec<PlatformView>,
}

/// What's running, as its events say: the last detection, every scan
/// (newest first) and every install.
#[derive(Clone, Copy, PartialEq)]
pub struct Live {
    pub detection: Signal<Option<DetectJob>>,
    pub scans: Signal<Vec<ScanJob>>,
    pub installs: Signal<Vec<SetupJob>>,
    /// Bumped on every emulation change, for queries beside the snapshot.
    pub changed: Signal<u64>,
}

pub fn use_live() -> Live {
    use_context::<Live>()
}

/// Keeps the newest revision of every scan, newest scan first.
fn merge_scan(jobs: &mut Vec<ScanJob>, next: ScanJob) {
    if jobs
        .iter()
        .any(|job| job.id == next.id && job.revision > next.revision)
    {
        return;
    }
    jobs.retain(|job| job.id != next.id);
    jobs.insert(0, next);
}

/// Keeps the newest revision of every install, oldest first.
fn merge_install(jobs: &mut Vec<SetupJob>, next: SetupJob) {
    match jobs.iter_mut().find(|job| job.id == next.id) {
        Some(job) if job.revision > next.revision => {}
        Some(job) => *job = next,
        None => jobs.push(next),
    }
}

/// The snapshot, and what's running, followed through the app's events.
fn use_emulation_state(emulation: &Emulation) -> (Resource<Result<Snapshot, ErrorReport>>, Live) {
    let emulation = emulation.clone();
    let source = use_context::<DataSource>();
    let snapshot = {
        let emulation = emulation.clone();
        use_resource(move || {
            let emulation = emulation.clone();
            change(async move {
                Ok(Snapshot {
                    catalog: emulation.catalog(),
                    overview: emulation.overview().await?,
                    emulators: emulation.emulators().await?,
                    platforms: emulation.platforms().await?,
                })
            })
        })
    };
    let live = use_context_provider(|| Live {
        detection: Signal::new(emulation.detection()),
        scans: Signal::new(emulation.scan_jobs().into_iter().rev().collect()),
        installs: Signal::new(emulation.setup_jobs()),
        changed: Signal::new(0),
    });
    use_hook(move || {
        let mut events = source.events.subscribe();
        let mut snapshot = snapshot;
        let Live {
            mut detection,
            mut scans,
            mut installs,
            mut changed,
        } = live;
        spawn(async move {
            loop {
                match events.recv().await {
                    Ok(Event::EmulationChanged(_)) => {
                        snapshot.restart();
                        *changed.write() += 1;
                    }
                    Ok(Event::EmulationDetectChanged(job)) => {
                        let newer = detection
                            .peek()
                            .as_ref()
                            .is_none_or(|old| old.id != job.0.id || old.revision <= job.0.revision);
                        if newer {
                            detection.set(Some(job.0));
                        }
                    }
                    Ok(Event::EmulationScanChanged(job)) => {
                        merge_scan(&mut scans.write(), job.0);
                    }
                    Ok(Event::EmulationSetupChanged(job)) => {
                        let done = job.0.status == caskade_core::emulation::SetupStatus::Done;
                        merge_install(&mut installs.write(), job.0);
                        if done {
                            snapshot.restart();
                            *changed.write() += 1;
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        snapshot.restart();
                        *changed.write() += 1;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    });
    (snapshot, live)
}

/// The sections, in order; the id is the address's `?section=`.
const SECTIONS: [&str; 5] = ["emulators", "browse", "platforms", "scanners", "romm"];

fn section_label(id: &str) -> String {
    match id {
        "browse" => t!("Get emulators"),
        "platforms" => tp!("emulation section", "Platforms"),
        "scanners" => t!("ROM scanners"),
        "romm" => t!("RomM server"),
        _ => tp!("emulation section", "Emulators"),
    }
}

fn section_icon(id: &str) -> Icon {
    match id {
        "browse" => CLOUD_DOWNLOAD,
        "platforms" => LAYERS,
        "scanners" => FOLDER_SEARCH,
        "romm" => SERVER,
        _ => CPU,
    }
}

/// "Scan all"'s outcome in one line.
fn scanned(summaries: &[ScanSummary]) -> String {
    let failed: Vec<&ScanSummary> = summaries
        .iter()
        .filter(|summary| summary.error.is_some())
        .collect();
    let added: u32 = summaries.iter().map(|summary| summary.added).sum();
    let missing: u32 = summaries.iter().map(|summary| summary.missing).sum();
    let ok = summaries.len() - failed.len();
    let scanners = tn!("Scanned # scanner", "Scanned # scanners", ok);
    let games = tn!("# new game", "# new games", added);
    let mut parts = vec![t!(
        note = "“Scanned 2 scanners: 5 new games”",
        "{scanners}: {games}",
        scanners = scanners,
        games = games
    )];
    if missing > 0 {
        parts.push(t!(
            note = "ROMs a scan no longer found",
            "{missing} missing",
            missing = missing
        ));
    }
    if !failed.is_empty() {
        let errors = failed
            .iter()
            .filter_map(|summary| summary.error.clone())
            .collect::<Vec<_>>()
            .join("; ");
        parts.push(t!(
            note = "How many ROM scanners failed, and their errors",
            "{failed} failed ({errors})",
            failed = failed.len(),
            errors = errors
        ));
    }
    parts.join(", ")
}

/// `core.emulation`: console mode always plays fullscreen; this makes
/// desktop mode do it too. In a card of its own, not among the overview's
/// counts (#54).
#[component]
fn Playing() -> Element {
    let section = use_settings_section("core.emulation");
    let value = section.value();
    let fullscreen = value
        .as_ref()
        .and_then(|value| value.get("desktopFullscreen"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    rsx! {
        SettingsSection {
            icon: MONITOR_PLAY,
            title: t!("Playing"),
            description: t!("How emulated games open."),
            SettingsStatus { section, what: t!("Playing") }
            if value.is_some() {
                SettingRow {
                    icon: MAXIMIZE,
                    label: t!("Desktop full screen"),
                    description: t!("Play emulated games fullscreen in desktop mode too. Console mode always does."),
                    Switch {
                        checked: fullscreen,
                        on_change: move |next: bool| section.set_field("desktopFullscreen", Value::Bool(next)),
                    }
                }
            }
        }
    }
}

/// Settings → Emulation at sub-section `section`.
#[component]
pub fn EmulationSettings(section: Option<String>) -> Element {
    match find_emulation() {
        Some(emulation) => rsx! {
            Panel { emulation, section }
        },
        None => rsx! {
            div { "data-settings": "emulators",
                FailureNotice {
                    error: ErrorReport::new(
                        caskade_core::error::ErrorCode::Unavailable,
                        t!("This part of Caskade didn't start this session. Restarting Caskade may fix it."),
                    ),
                    title: t!("Emulators could not be loaded."),
                }
            }
        },
    }
}

#[component]
fn Panel(emulation: Emulation, section: Option<String>) -> Element {
    use_context_provider(|| emulation.clone());
    let router = try_use_context::<Router>();
    let (mut snapshot, live) = use_emulation_state(&emulation);
    // The address's section, kept here too so it shows at once.
    let mut chosen = use_signal(|| section.clone());
    use_effect(use_reactive!(|section| chosen.set(section)));
    let shown = chosen()
        .filter(|id| SECTIONS.contains(&id.as_str()))
        .unwrap_or_else(|| "emulators".to_owned());
    let mut go = move |id: String| {
        chosen.set(Some(id.clone()));
        if let Some(router) = router {
            router.replace(
                &Route::Settings {
                    tab: "emulators",
                    section: Some(id),
                }
                .href(),
            );
        }
    };
    let mut adding = use_signal(|| false);
    let mut selected = use_signal(|| None::<String>);
    let mut detecting = use_signal(|| false);
    let mut detect_failure = use_signal(|| None::<ErrorReport>);
    let mut scanning = use_signal(|| false);
    let mut scan_result = use_signal(|| None::<Result<Vec<ScanSummary>, ErrorReport>>);
    // How many scanners take part in library updates, for Scan all.
    let automatic = {
        let emulation = emulation.clone();
        use_resource(move || {
            let _ = (live.changed)();
            let emulation = emulation.clone();
            change(async move { emulation.scanners().await })
        })
    };
    let automatic = match &*automatic.read() {
        Some(Ok(rows)) => rows
            .iter()
            .filter(|row| row.scanner.in_library_update)
            .count(),
        _ => 0,
    };
    let detect = {
        let emulation = emulation.clone();
        use_callback(move |()| {
            detecting.set(true);
            detect_failure.set(None);
            let emulation = emulation.clone();
            let mut detection = live.detection;
            spawn(async move {
                match change(async move { emulation.detect(None).await }).await {
                    Ok(job) => detection.set(Some(job)),
                    Err(report) => {
                        detecting.set(false);
                        detect_failure.set(Some(report));
                    }
                }
            });
        })
    };
    let add = move |_| {
        adding.set(true);
        go("emulators".to_owned());
    };
    let scan_all = {
        let emulation = emulation.clone();
        move |_| {
            scanning.set(true);
            scan_result.set(None);
            let emulation = emulation.clone();
            spawn(async move {
                let result = change(async move { emulation.scan_all().await }).await;
                scanning.set(false);
                scan_result.set(Some(result));
            });
        }
    };
    let loaded = match &*snapshot.read() {
        None => None,
        Some(Ok(snapshot)) => Some(Ok(snapshot.clone())),
        Some(Err(report)) => Some(Err(report.clone())),
    };
    let snapshot_now = match loaded {
        None => {
            return rsx! {
                p { "data-settings": "emulators", role: "status", class: "p-5 text-sm text-fg-muted",
                    {t!("Loading emulators…")}
                }
            };
        }
        Some(Err(report)) => {
            return rsx! {
                div { "data-settings": "emulators",
                    FailureNotice {
                        error: report,
                        title: t!("Emulators could not be loaded."),
                        on_retry: move |_| snapshot.restart(),
                    }
                }
            };
        }
        Some(Ok(snapshot)) => snapshot,
    };
    let stats = &snapshot_now.overview;
    let searching = live
        .detection
        .read()
        .as_ref()
        .is_some_and(|job| job.status == caskade_core::emulation::DetectStatus::Running);
    let bios = match stats.bios_issues {
        Some(issues) => format::count(issues as f64),
        None => t!(note = "BIOS issues", "Not checked"),
    };
    let tiles: [(Icon, String, String); 4] = [
        (
            CPU,
            format::count(stats.emulators as f64),
            tp!("emulation section", "Emulators"),
        ),
        (
            LAYERS,
            format!(
                "{} / {}",
                format::count(stats.platforms_with_default as f64),
                format::count(snapshot_now.platforms.len() as f64)
            ),
            t!("Platforms ready"),
        ),
        (GAMEPAD, format::count(stats.roms as f64), t!("ROMs found")),
        (SHIELD_CHECK, bios, t!("BIOS issues")),
    ];
    let segments: Vec<Segment> = SECTIONS
        .iter()
        .map(|id| Segment {
            icon: Some(section_icon(id)),
            ..Segment::new(*id, section_label(id))
        })
        .collect();
    let body = match shown.as_str() {
        "browse" => rsx! {
            setup::BrowseEmulators { snapshot: snapshot_now.clone() }
        },
        "platforms" => rsx! {
            platforms::ConfiguredPlatforms { snapshot: snapshot_now.clone(), on_detect: move |_| detect.call(()) }
        },
        "scanners" => rsx! {
            scanners::ScannersSection { snapshot: snapshot_now.clone() }
        },
        "romm" => rsx! {
            romm::RommPanel { snapshot: snapshot_now.clone() }
        },
        _ => rsx! {
            setup::EmulatorsFromDevices {}
            setup::MissingEmulators {}
            emulators::ConfiguredEmulators {
                snapshot: snapshot_now.clone(),
                selected: selected(),
                adding: adding(),
                on_select: move |id: String| {
                    selected.set(Some(id));
                    adding.set(false);
                },
                on_add: move |_| adding.set(true),
                on_cancel_add: move |_| adding.set(false),
            }
        },
    };
    rsx! {
        div { "data-settings": "emulators", class: "space-y-4",
            section {
                "aria-label": t!("Emulation overview"),
                class: "space-y-5 rounded-panel border border-border bg-surface-raised p-6",
                div { class: "flex flex-wrap items-center gap-5",
                    div { class: "chamfer grid size-16 place-items-center bg-surface-overlay",
                        Svg { icon: GAMEPAD, class: "size-8 text-accent-alt" }
                    }
                    div { class: "min-w-64 flex-1",
                        h2 { class: "font-display text-xl font-semibold", {t!("Emulation")} }
                        p { class: "mt-1 text-sm text-fg-muted",
                            {t!("Configure emulators, choose platform defaults and find your ROM collection.")}
                        }
                    }
                    div { class: "flex flex-wrap gap-2",
                        Button {
                            id: "emulation-detect",
                            variant: ButtonVariant::Primary,
                            icon: SCAN_SEARCH,
                            disabled: searching,
                            on_press: move |_| detect.call(()),
                            {t!("Detect emulators")}
                        }
                        Button { id: "emulation-add", icon: PLUS, on_press: add, {t!("Add emulator")} }
                        Button {
                            id: "emulation-scan-all",
                            icon: REFRESH_CW,
                            disabled: automatic == 0 || scanning(),
                            on_press: scan_all,
                            if scanning() {
                                {t!("Scanning…")}
                            } else {
                                {t!("Scan all")}
                            }
                        }
                    }
                }
                div { class: "grid gap-3 sm:grid-cols-2 xl:grid-cols-4",
                    for (icon, value, label) in tiles {
                        div {
                            key: "{label}",
                            "data-part": "stat",
                            class: "chamfer flex items-center gap-3 bg-surface-overlay px-4 py-3",
                            Svg { icon, class: "size-6 text-accent-alt" }
                            div {
                                p { class: "font-mono text-lg font-semibold", "{value}" }
                                p { class: "text-xs text-fg-muted", "{label}" }
                            }
                        }
                    }
                }
                if automatic == 0 {
                    p { class: "text-xs text-fg-muted",
                        {t!("Scan all runs the ROM scanners included in library updates. Add one under ROM Scanners.")}
                    }
                }
                match scan_result() {
                    Some(Err(report)) => rsx! {
                        p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
                    },
                    Some(Ok(summaries)) => rsx! {
                        p { role: "status", class: "text-sm text-fg-muted", {scanned(&summaries)} }
                    },
                    None => rsx! {},
                }
                if let Some(report) = detect_failure().filter(|_| !detecting()) {
                    p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
                }
            }
            Playing {}
            if detecting() {
                detection::DetectionReview { on_close: move |_| detecting.set(false) }
            }
            SegmentedControl {
                "aria-label": t!("Emulation section"),
                options: segments,
                value: shown.clone(),
                on_change: go,
                class: "max-w-full flex-wrap self-start",
            }
            {body}
        }
    }
}
