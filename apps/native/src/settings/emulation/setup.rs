//! One-click emulator installs (#115), as the webview's
//! `emulators/EmulatorSetup.tsx`, `emulators/FromDevices.tsx` and
//! `api/emulatorSetup.ts`: platforms with games and no emulator, every
//! emulator Caskade can install here, the emulators other devices use
//! (#275), a Caskade-installed emulator's updates, and BIOS files from
//! RomM. Installs run in the core; their progress comes from
//! `super::use_live().installs`.

use caskade_core::emulation::SetupStatus;
use caskade_core::error::ErrorReport;
use caskade_ui_data::emulation::{
    BiosFetch, DeviceEmulator, Emulation, Installable, MissingPlatform, SetupJob,
};
use caskade_ui_i18n::{format, t, tn, tp};
use dioxus_native::prelude::*;

use crate::data::{change, run};
use crate::ui::icons::{
    CLOUD_DOWNLOAD, DOWNLOAD, MONITOR_SMARTPHONE, REFRESH_CW, ROTATE_CCW, TRIANGLE_ALERT, X,
};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, DotTone, Icon, Pill, PillTone, SettingsSection, StatusDot,
    Svg,
};

/// Whether a job is still under way.
pub fn setup_running(job: &SetupJob) -> bool {
    !job.status.finished()
}

/// What a job is doing, in a few words.
pub fn setup_progress(job: &SetupJob) -> String {
    match job.status {
        SetupStatus::Queued => t!("Waiting…"),
        SetupStatus::Downloading => match job.total {
            Some(total) if total > 0 => t!(
                "Downloading {done} of {total}",
                done = format::size(Some(job.downloaded)),
                total = format::size(Some(total))
            ),
            _ if job.downloaded > 0 => t!(
                "Downloading {done}",
                done = format::size(Some(job.downloaded))
            ),
            _ => t!("Downloading…"),
        },
        SetupStatus::Unpacking => t!("Unpacking…"),
        SetupStatus::Configuring => t!("Setting up…"),
        SetupStatus::Done => match &job.version {
            Some(version) => t!(
                "Installed {version}",
                version = version.split('@').next().unwrap_or(version).to_owned()
            ),
            None => t!("Installed"),
        },
        SetupStatus::Failed => job
            .error
            .clone()
            .unwrap_or_else(|| t!("The install failed")),
        SetupStatus::Cancelled => t!("Cancelled"),
    }
}

/// A status as `data-status` names it.
fn status_name(status: SetupStatus) -> &'static str {
    match status {
        SetupStatus::Queued => "queued",
        SetupStatus::Downloading => "downloading",
        SetupStatus::Unpacking => "unpacking",
        SetupStatus::Configuring => "configuring",
        SetupStatus::Done => "done",
        SetupStatus::Failed => "failed",
        SetupStatus::Cancelled => "cancelled",
    }
}

/// The newest install of an emulator, or for a platform.
fn latest_job(jobs: &[SetupJob], matches: impl Fn(&SetupJob) -> bool) -> Option<SetupJob> {
    jobs.iter().filter(|job| matches(job)).last().cloned()
}

/// What an install starts from.
enum Target {
    /// An emulator definition.
    Definition(String),
    /// A platform's recommended emulator.
    Platform(String),
}

/// Starts an install; its job joins `installs`, the live installs.
fn install(
    emulation: &Emulation,
    target: Target,
    installs: Signal<Vec<SetupJob>>,
    pending: Signal<bool>,
    failure: Signal<Option<ErrorReport>>,
) {
    let emulation = emulation.clone();
    run(
        pending,
        failure,
        async move {
            match target {
                Target::Definition(id) => emulation.install(&id).await,
                Target::Platform(id) => emulation.install_for(&id).await,
            }
        },
        move |job| {
            let mut installs = installs;
            if let Some(job) = job {
                super::merge_install(&mut installs.write(), job);
            }
        },
    );
}

/// A job's progress, its warnings (a BIOS to add) and its error.
#[component]
fn JobStatus(job: SetupJob) -> Element {
    let emulation = super::use_emulation();
    let live = super::use_live();
    let running = setup_running(&job);
    let progress = setup_progress(&job);
    let done = job.status == SetupStatus::Done;
    let failed = job.status == SetupStatus::Failed;
    let tone = if done { DotTone::Ok } else { DotTone::Off };
    let cancel = {
        let id = job.id.clone();
        move |_| {
            let emulation = emulation.clone();
            let id = id.clone();
            let mut installs = live.installs;
            spawn(async move {
                match change(async move { emulation.cancel_install(&id) }).await {
                    Ok(job) => super::merge_install(&mut installs.write(), job),
                    Err(report) => tracing::warn!("Cancel failed: {}", report.summary),
                }
            });
        }
    };
    rsx! {
        div { "data-part": "setup-status", "data-status": status_name(job.status), class: "space-y-1 text-xs",
            div { class: "flex items-center gap-2",
                if running {
                    span { role: "status", class: "text-fg-muted", "{progress}" }
                } else if failed {
                    span { role: "alert", class: "text-danger", "{progress}" }
                } else {
                    StatusDot { tone, "{progress}" }
                }
                if running {
                    Button {
                        id: "setup-cancel-{job.id}",
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Ghost,
                        icon: X,
                        "aria-label": t!("Cancel installing {name}", name = job.name.clone()),
                        on_press: cancel,
                        {tp!("emulator install", "Cancel")}
                    }
                }
            }
            if done && !job.fetched_bios.is_empty() {
                p { class: "text-fg-muted",
                    {t!(note = "BIOS files downloaded from the user's RomM server", "Got {files} from RomM.", files = job.fetched_bios.join(", "))}
                }
            }
            if done {
                for warning in job.warnings.iter().cloned() {
                    p { key: "{warning}", class: "text-warning", "{warning}" }
                }
            }
        }
    }
}

/// Platforms the user has games for that no emulator runs.
#[component]
pub fn MissingEmulators() -> Element {
    let emulation = super::use_emulation();
    let live = super::use_live();
    let pending = use_signal(|| false);
    let failure = use_signal(|| None::<ErrorReport>);
    let missing = {
        let emulation = emulation.clone();
        use_resource(move || {
            let _ = (live.changed)();
            let emulation = emulation.clone();
            change(async move { emulation.missing().await })
        })
    };
    let rows: Vec<MissingPlatform> = match (*missing.read()).clone() {
        None => return rsx! {},
        Some(Err(report)) => {
            return rsx! {
                p { "data-part": "missing-emulators", role: "alert", class: "text-sm text-danger",
                    {t!("Couldn't check which platforms need an emulator: {error}", error = report.summary.clone())}
                }
            };
        }
        Some(Ok(rows)) => rows,
    };
    if rows.is_empty() {
        return rsx! {};
    }
    let jobs = (live.installs)();
    rsx! {
        section {
            "data-part": "missing-emulators",
            "aria-labelledby": "missing-emulators-title",
            class: "space-y-4 rounded-panel border border-warning/40 bg-warning/5 p-5",
            div { class: "flex items-start gap-3",
                Svg { icon: TRIANGLE_ALERT, class: "mt-0.5 size-5 shrink-0 text-warning" }
                div {
                    h3 { id: "missing-emulators-title", class: "font-display font-semibold",
                        {tn!("# platform has no emulator", "# platforms have no emulator", rows.len())}
                    }
                    p { class: "mt-1 text-sm text-fg-muted",
                        {t!("You have games for these, but nothing to play them with. Caskade can download, install and set up an emulator for each in one click.")}
                    }
                }
            }
            ul { class: "divide-y divide-border rounded-control border border-border bg-surface-raised",
                for platform in rows {
                    {
                        let job = latest_job(&jobs, |row| row.platform_id.as_deref() == Some(platform.platform_id.as_str()));
                        let running = job.as_ref().is_some_and(setup_running);
                        let failed = job.as_ref().is_some_and(|job| job.status == SetupStatus::Failed);
                        let icon = if failed { ROTATE_CCW } else { DOWNLOAD };
                        let games = tn!("# game", "# games", platform.games);
                        let details = match &platform.recommended {
                            Some(recommended) => format!(
                                "{games} · {}",
                                t!(note = "The emulator Caskade recommends for a platform", "Recommended: {name}", name = recommended.name.clone())
                            ),
                            None => games,
                        };
                        let platform_id = platform.platform_id.clone();
                        let emulation = emulation.clone();
                        rsx! {
                            li {
                                key: "{platform.platform_id}",
                                "data-part": "missing-platform",
                                class: "flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3",
                                div { class: "min-w-48 flex-1",
                                    p { class: "font-medium", "{platform.name}" }
                                    p { class: "text-xs text-fg-muted", "{details}" }
                                    if let Some(job) = job {
                                        JobStatus { job }
                                    }
                                }
                                if let Some(recommended) = &platform.recommended {
                                    Button {
                                        id: "missing-install-{platform.platform_id}",
                                        variant: ButtonVariant::Primary,
                                        icon,
                                        disabled: running || pending(),
                                        on_press: move |_| install(&emulation, Target::Platform(platform_id.clone()), live.installs, pending, failure),
                                        if failed {
                                            {t!("Try again")}
                                        } else {
                                            {t!("Install {name}", name = recommended.name.clone())}
                                        }
                                    }
                                } else {
                                    p { class: "max-w-80 text-xs text-fg-muted",
                                        {t!("Caskade can't install an emulator for {platform} on this system. Add one you have installed with Add emulator.", platform = platform.short.clone())}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(report) = failure() {
                p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
            }
        }
    }
}

/// How an emulator Caskade installs is updated: its label and tone.
fn source(source: &str) -> (String, PillTone) {
    match source {
        "github" | "dolphin" => (
            t!(
                note = "How an emulator Caskade installs is updated",
                "Updates automatically"
            ),
            PillTone::Accent,
        ),
        "flatpak" => (
            t!(
                note = "How an emulator Caskade installs is updated",
                "Updated by Flatpak"
            ),
            PillTone::Accent,
        ),
        _ => (
            t!(
                note = "How an emulator Caskade installs is updated",
                "Download only"
            ),
            PillTone::Warning,
        ),
    }
}

#[component]
fn EmulatorCard(
    item: Installable,
    /// The short names of the platforms it runs.
    platforms: String,
    /// Where an install's failure shows (the section's).
    failure: Signal<Option<ErrorReport>>,
    /// The installable list changed (an update skipped or started).
    on_settled: EventHandler<()>,
) -> Element {
    let emulation = super::use_emulation();
    let live = super::use_live();
    let pending = use_signal(|| false);
    let job = latest_job(&live.installs.read(), |row| {
        row.definition_id == item.definition_id
    });
    let running = job.as_ref().is_some_and(setup_running);
    let managed = item.managed.is_some();
    let (label, tone) = source(&item.source);
    let pill_icon: Icon = if item.updates {
        REFRESH_CW
    } else {
        CLOUD_DOWNLOAD
    };
    let show_job = job.as_ref().filter(|job| {
        running || !managed || job.status != SetupStatus::Done || !job.warnings.is_empty()
    });
    let installed = match &item.installed_label {
        Some(version) => t!(
            note = "An emulator's installed version",
            "Installed {version}",
            version = version.clone()
        ),
        None => tp!("emulator", "Installed"),
    };
    let definition_id = item.definition_id.clone();
    let (variant, icon, aria_label, action) = if managed {
        (
            ButtonVariant::Secondary,
            ROTATE_CCW,
            t!("Reinstall {name}", name = item.name.clone()),
            tp!("emulator", "Reinstall"),
        )
    } else {
        (
            ButtonVariant::Primary,
            DOWNLOAD,
            t!("Install {name}", name = item.name.clone()),
            tp!("emulator", "Install"),
        )
    };
    rsx! {
        li {
            "data-part": "emulator-card",
            "data-installed": managed.then_some(""),
            class: "flex flex-col gap-3 rounded-control border border-border bg-surface-overlay p-4",
            div { class: "flex items-start justify-between gap-3",
                div {
                    p { class: "font-display font-semibold", "{item.name}" }
                    p { class: "mt-1 text-xs text-fg-muted", "{platforms}" }
                }
                Pill { tone, icon: pill_icon, "{label}" }
            }
            if let Some(job) = show_job.cloned() {
                JobStatus { job }
            } else if managed {
                StatusDot { tone: DotTone::Ok, "{installed}" }
            }
            div { class: "mt-auto flex flex-wrap gap-2",
                if managed && item.update_available && !running {
                    ManagedUpdate { item: item.clone(), on_settled }
                } else {
                    Button {
                        id: "emulator-install-{item.definition_id}",
                        size: ButtonSize::Sm,
                        variant,
                        icon,
                        disabled: running || pending(),
                        "aria-label": aria_label,
                        on_press: move |_| install(&emulation, Target::Definition(definition_id.clone()), live.installs, pending, failure),
                        "{action}"
                    }
                }
            }
        }
    }
}

/// Update and Skip this version for a newer version of a managed emulator.
#[component]
fn ManagedUpdate(item: Installable, on_settled: EventHandler<()>) -> Element {
    let emulation = super::use_emulation();
    let live = super::use_live();
    let pending = use_signal(|| false);
    let failure = use_signal(|| None::<ErrorReport>);
    let Some(emulator_id) = item
        .managed
        .as_ref()
        .map(|managed| managed.emulator_id.clone())
    else {
        return rsx! {};
    };
    let version = item.latest_label.clone().unwrap_or_default();
    let update = {
        let emulation = emulation.clone();
        let emulator_id = emulator_id.clone();
        move |_| {
            let emulation = emulation.clone();
            let emulator_id = emulator_id.clone();
            let mut installs = live.installs;
            run(
                pending,
                failure,
                async move { emulation.update(&emulator_id).await },
                move |job| {
                    if let Some(job) = job {
                        super::merge_install(&mut installs.write(), job);
                    }
                    on_settled.call(());
                },
            );
        }
    };
    let skip = {
        let emulator_id = emulator_id.clone();
        move |_| {
            let emulation = emulation.clone();
            let emulator_id = emulator_id.clone();
            run(
                pending,
                failure,
                async move { emulation.skip_update(&emulator_id).await },
                move |_| on_settled.call(()),
            );
        }
    };
    rsx! {
        Button {
            id: "managed-update-{emulator_id}",
            size: ButtonSize::Sm,
            variant: ButtonVariant::Primary,
            icon: DOWNLOAD,
            disabled: pending(),
            "aria-label": t!("Update {name} to {version}", name = item.name.clone(), version = version.clone()),
            on_press: update,
            {t!(note = "Updates an emulator to the named version", "Update to {version}", version = version.clone())}
        }
        Button {
            id: "managed-skip-{emulator_id}",
            size: ButtonSize::Sm,
            variant: ButtonVariant::Ghost,
            disabled: pending(),
            "aria-label": t!("Skip this version of {name}", name = item.name.clone()),
            on_press: skip,
            {t!("Skip this version")}
        }
        if let Some(report) = failure() {
            p { role: "alert", class: "basis-full text-xs text-danger", "{report.summary}" }
        }
    }
}

/// In the emulator editor: what Caskade installed, and its updates.
#[component]
pub fn ManagedEmulatorInfo(emulator_id: String) -> Element {
    let emulation = super::use_emulation();
    let live = super::use_live();
    let mut poke = use_signal(|| 0u64);
    let checking = use_signal(|| false);
    let check_failure = use_signal(|| None::<ErrorReport>);
    let installable = {
        let emulation = emulation.clone();
        use_resource(move || {
            let _ = ((live.changed)(), poke());
            let emulation = emulation.clone();
            change(async move { emulation.installable().await })
        })
    };
    let item = match &*installable.read() {
        Some(Ok(rows)) => rows
            .iter()
            .find(|row| {
                row.managed
                    .as_ref()
                    .is_some_and(|managed| managed.emulator_id == emulator_id)
            })
            .cloned(),
        _ => None,
    };
    let Some(item) = item else {
        return rsx! {};
    };
    let Some(managed) = item.managed.clone() else {
        return rsx! {};
    };
    let job = latest_job(&live.installs.read(), |row| {
        row.definition_id == item.definition_id
    });
    let running = job.as_ref().is_some_and(setup_running);
    let checked =
        (managed.checked_at_ms != 0).then(|| crate::library::date_time(managed.checked_at_ms));
    let title = match &item.installed_label {
        Some(label) => format!("{} · {label}", t!("Installed by Caskade")),
        None => t!("Installed by Caskade"),
    };
    let state = if !item.updates {
        if item.source == "flatpak" {
            t!("Flatpak keeps it up to date.")
        } else {
            t!("Download only: Reinstall gets the newest build.")
        }
    } else if item.update_available {
        t!(
            note = "A newer version of an emulator",
            "{version} is available.",
            version = item.latest_label.clone().unwrap_or_default()
        )
    } else if let Some(error) = &managed.error {
        t!(
            "The last update check failed: {error}",
            error = error.clone()
        )
    } else if let Some(checked) = checked {
        t!("Up to date (checked {checked}).", checked = checked)
    } else {
        t!("Not checked for updates yet.")
    };
    let show_job = job.filter(|job| running || job.status == SetupStatus::Failed);
    let check = {
        let emulator_id = emulator_id.clone();
        move |_| {
            let emulation = emulation.clone();
            let emulator_id = emulator_id.clone();
            run(
                checking,
                check_failure,
                async move { emulation.check_update(&emulator_id).await },
                move |_| {
                    let mut poke = poke;
                    *poke.write() += 1;
                },
            );
        }
    };
    rsx! {
        section {
            "data-part": "managed-emulator",
            "aria-label": t!("Installed by Caskade"),
            class: "flex flex-wrap items-center gap-x-4 gap-y-2 rounded-panel border border-border bg-surface-raised px-5 py-4",
            Svg { icon: CLOUD_DOWNLOAD, class: "size-5 text-accent-alt" }
            div { class: "min-w-48 flex-1 text-sm",
                p { class: "font-medium", "{title}" }
                p { class: "text-xs text-fg-muted", "{state}" }
                if let Some(job) = show_job {
                    JobStatus { job }
                }
                if let Some(report) = check_failure() {
                    p { role: "alert", class: "text-xs text-danger", "{report.summary}" }
                }
            }
            if item.update_available && !running {
                div { class: "flex gap-2",
                    ManagedUpdate { item: item.clone(), on_settled: move |_| *poke.write() += 1 }
                }
            } else if item.updates {
                Button {
                    id: "managed-check-{emulator_id}",
                    size: ButtonSize::Sm,
                    variant: ButtonVariant::Ghost,
                    icon: REFRESH_CW,
                    disabled: checking() || running,
                    on_press: check,
                    if checking() {
                        {t!("Checking…")}
                    } else {
                        {t!("Check for updates")}
                    }
                }
            }
        }
    }
}

/// What a BIOS fetch did, in a sentence.
fn fetched(result: &BiosFetch) -> String {
    let mut text = if result.fetched.is_empty() {
        t!("No RomM server has them.")
    } else {
        t!(
            note = "BIOS files downloaded from RomM",
            "Got {files}.",
            files = result.fetched.join(", ")
        )
    };
    if !result.missing.is_empty() {
        let missing = match &result.dir {
            Some(dir) => t!(
                note = "BIOS files; the folder to put them in",
                "Still missing: {files}. Put them in {dir}.",
                files = result.missing.join(", "),
                dir = dir.clone()
            ),
            None => t!(
                note = "BIOS files",
                "Still missing: {files}.",
                files = result.missing.join(", ")
            ),
        };
        text = format!("{text} {missing}");
    }
    text
}

/// "Get from RomM" for a platform's missing BIOS files. A fetch that got
/// files changes the emulation, which reads the platforms again.
#[component]
pub fn FetchBios(platform_id: String) -> Element {
    let emulation = super::use_emulation();
    let pending = use_signal(|| false);
    let failure = use_signal(|| None::<ErrorReport>);
    let mut result = use_signal(|| None::<BiosFetch>);
    let fetch = {
        let platform_id = platform_id.clone();
        move |_| {
            let emulation = emulation.clone();
            let platform_id = platform_id.clone();
            result.set(None);
            run(
                pending,
                failure,
                async move { emulation.fetch_bios(&platform_id).await },
                move |fetch| {
                    let mut result = result;
                    result.set(fetch);
                },
            );
        }
    };
    rsx! {
        div { class: "space-y-1",
            Button {
                id: "fetch-bios-{platform_id}",
                size: ButtonSize::Sm,
                icon: CLOUD_DOWNLOAD,
                disabled: pending(),
                on_press: fetch,
                if pending() {
                    {t!("Looking on RomM…")}
                } else {
                    {t!("Get from RomM")}
                }
            }
            if let Some(report) = failure() {
                p { role: "alert", class: "text-xs text-danger", "{report.summary}" }
            }
            if let Some(result) = result() {
                p { role: "status", class: "text-xs text-fg-muted", {fetched(&result)} }
            }
        }
    }
}

/// Every emulator Caskade can install here.
#[component]
pub fn BrowseEmulators(snapshot: super::Snapshot) -> Element {
    let emulation = super::use_emulation();
    let live = super::use_live();
    let mut poke = use_signal(|| 0u64);
    let failure = use_signal(|| None::<ErrorReport>);
    let installable = use_resource(move || {
        let _ = ((live.changed)(), poke());
        let emulation = emulation.clone();
        change(async move { emulation.installable().await })
    });
    let loaded = (*installable.read()).clone();
    let short = |id: &String| {
        snapshot
            .platforms
            .iter()
            .find(|platform| &platform.platform.id == id)
            .map(|platform| platform.platform.short.clone())
            .unwrap_or_else(|| id.clone())
    };
    rsx! {
        SettingsSection {
            icon: CLOUD_DOWNLOAD,
            title: t!("Get emulators"),
            description: t!("Caskade downloads these for you, sets them up and makes them the default for platforms that have none. Emulators from GitHub are kept up to date."),
            if let Some(Err(report)) = &loaded {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
            }
            if let Some(report) = failure() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
            }
            match loaded.clone() {
                None => rsx! {
                    p { role: "status", class: "mt-4 text-sm text-fg-muted", {t!("Loading emulators…")} }
                },
                Some(Ok(rows)) if !rows.is_empty() => rsx! {
                    ul {
                        "aria-label": t!("Emulators to install"),
                        class: "mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-3",
                        for item in rows {
                            {
                                let platforms = item.platforms.iter().map(short).collect::<Vec<_>>().join(", ");
                                let key = item.definition_id.clone();
                                rsx! {
                                    EmulatorCard {
                                        key: "{key}",
                                        item,
                                        platforms,
                                        failure,
                                        on_settled: move |_| *poke.write() += 1,
                                    }
                                }
                            }
                        }
                    }
                },
                _ => rsx! {
                    p { class: "mt-4 text-sm text-fg-muted", {t!("Caskade can't install emulators on this system yet.")} }
                },
            }
        }
    }
}

/// Emulators the user's other devices use for a platform that this device
/// doesn't have (#275): a platform's preferred emulator syncs, and waits
/// here until its emulator is installed. Installing is one click, and only
/// when asked; Keep mine sets the other device's choice aside.
#[component]
pub fn EmulatorsFromDevices() -> Element {
    let emulation = super::use_emulation();
    let live = super::use_live();
    let poke = use_signal(|| 0u64);
    let installing = use_signal(|| false);
    let install_failure = use_signal(|| None::<ErrorReport>);
    let keeping = use_signal(|| false);
    let keep_failure = use_signal(|| None::<ErrorReport>);
    let offers = {
        let emulation = emulation.clone();
        use_resource(move || {
            let _ = ((live.changed)(), poke());
            let emulation = emulation.clone();
            change(async move { emulation.from_devices().await })
        })
    };
    let offers: Vec<DeviceEmulator> = match &*offers.read() {
        Some(Ok(rows)) => rows.clone(),
        _ => Vec::new(),
    };
    if offers.is_empty() {
        return rsx! {};
    }
    let jobs = (live.installs)();
    let failure = install_failure().or(keep_failure());
    rsx! {
        section {
            "data-part": "emulators-from-devices",
            "aria-labelledby": "emulators-from-devices-title",
            class: "space-y-4 rounded-panel border border-border bg-surface-raised p-5",
            div { class: "flex items-start gap-3",
                Svg { icon: MONITOR_SMARTPHONE, class: "mt-0.5 size-5 shrink-0 text-accent" }
                div {
                    h3 { id: "emulators-from-devices-title", class: "font-display font-semibold",
                        {t!("From your other devices")}
                    }
                    p { class: "mt-1 text-sm text-fg-muted",
                        {t!("Your other devices play these platforms with an emulator this device doesn't have. Install it to use it here too, or keep this device's.")}
                    }
                }
            }
            ul { class: "divide-y divide-border rounded-control border border-border",
                for offer in offers {
                    {
                        let job = latest_job(&jobs, |row| row.definition_id == offer.definition_id);
                        let running = job.as_ref().is_some_and(setup_running);
                        let failed = job.as_ref().is_some_and(|job| job.status == SetupStatus::Failed);
                        let icon = if failed { ROTATE_CCW } else { DOWNLOAD };
                        let (role, tone) = if failed { ("alert", "text-xs text-danger") } else { ("status", "text-xs text-fg-muted") };
                        let emulator = offer.emulator_name.clone();
                        let uses = if offer.device_name.is_empty() {
                            t!("Another of your devices uses {emulator}", emulator = emulator.clone())
                        } else {
                            t!("{device} uses {emulator}", device = offer.device_name.clone(), emulator = emulator.clone())
                        };
                        let install_emulation = emulation.clone();
                        let keep_emulation = emulation.clone();
                        let definition_id = offer.definition_id.clone();
                        let platform_id = offer.platform_id.clone();
                        rsx! {
                            li {
                                key: "{offer.platform_id}",
                                "data-part": "emulator-offer",
                                class: "flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3",
                                div { class: "min-w-48 flex-1",
                                    p { class: "font-medium", "{offer.platform_name}" }
                                    p { class: "text-xs text-fg-muted", "{uses}" }
                                    if let Some(job) = &job {
                                        p {
                                            role: "{role}",
                                            class: "{tone}",
                                            {setup_progress(job)}
                                        }
                                    }
                                }
                                if offer.installable {
                                    Button {
                                        id: "offer-install-{offer.platform_id}",
                                        variant: ButtonVariant::Primary,
                                        icon,
                                        disabled: running || installing(),
                                        on_press: move |_| install(&install_emulation, Target::Definition(definition_id.clone()), live.installs, installing, install_failure),
                                        if failed {
                                            {t!("Try again")}
                                        } else {
                                            {t!("Install {name}", name = emulator.clone())}
                                        }
                                    }
                                } else {
                                    p { class: "max-w-64 text-xs text-fg-muted",
                                        {t!("Caskade can't install {emulator} on this system.", emulator = emulator.clone())}
                                    }
                                }
                                Button {
                                    id: "offer-keep-{offer.platform_id}",
                                    variant: ButtonVariant::Ghost,
                                    disabled: running || keeping(),
                                    "aria-label": t!("Keep this device's emulator for {platform}", platform = offer.platform_name.clone()),
                                    on_press: move |_| {
                                        let emulation = keep_emulation.clone();
                                        let platform_id = platform_id.clone();
                                        run(
                                            keeping,
                                            keep_failure,
                                            async move { emulation.keep_mine(&platform_id).await },
                                            move |_| {
                                                let mut poke = poke;
                                                *poke.write() += 1;
                                            },
                                        );
                                    },
                                    {t!("Keep mine")}
                                }
                            }
                        }
                    }
                }
            }
            if let Some(report) = failure {
                p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
            }
        }
    }
}
