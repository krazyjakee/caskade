//! The status bar (`shell/StatusBar.tsx`, `SyncStatusLink.tsx`,
//! `AppUpdateLink.tsx`): what runs in the background, paired devices in
//! reach, a new Caskade, and the version. The spinner turns in eight steps
//! a turn (`animate-worker`), which keeps it under 2% of a core (#82).

use crate::data::status::{is_running, use_app_update, use_background, use_devices, use_jobs};
use crate::router::RouteLink;
use crate::ui::{Svg, icons};
use caskade_app::commands::app_update::AppUpdatePhase;
use caskade_core::devices::{DeviceLink, PairedDevice};
use caskade_core::install::{JobPhase, JobSnapshot, JobStatus};
use caskade_ui_i18n::{t, tn};
use dioxus_native::prelude::*;

/// A job's phase as the UI names it (`jobLabels.ts`'s `jobPhase`).
pub fn job_phase(phase: Option<JobPhase>) -> String {
    match phase {
        Some(JobPhase::Download) => t!(note = "A background job's phase", "Downloading"),
        Some(JobPhase::Install) => t!(note = "A background job's phase", "Installing"),
        Some(JobPhase::Move) => t!(note = "A background job's phase", "Moving"),
        None => t!(note = "A running job with no phase", "Working"),
    }
}

/// What the status bar says, from the background tasks, the jobs and
/// whether the background status could be read.
pub fn activity_label(
    tasks: &[caskade_core::jobs::Task],
    jobs: &[JobSnapshot],
    unavailable: bool,
    checking: bool,
) -> String {
    if unavailable {
        return t!("Background status unavailable");
    }
    let active: Vec<&JobSnapshot> = jobs
        .iter()
        .filter(|job| is_running(job) || job.status == JobStatus::Queued)
        .collect();
    let waiting = jobs
        .iter()
        .filter(|job| job.status == JobStatus::NeedsUser)
        .count();
    let running = tasks.iter().find(|task| !task.queued);
    let queued = tasks.iter().filter(|task| task.queued).count();
    let job = active
        .iter()
        .find(|job| is_running(job))
        .or_else(|| active.first())
        .copied();
    let description = running
        .map(|task| task.label.clone())
        .or_else(|| (queued > 0).then(|| tasks[0].label.clone()))
        .or_else(|| {
            job.map(|job| {
                let phase = if job.status == JobStatus::Queued {
                    t!(note = "A job waiting for its turn", "Queued")
                } else {
                    job_phase(job.phase)
                };
                t!(
                    note = "The status bar's running job: its phase, then the game",
                    "{phase}: {title}",
                    phase = phase,
                    title = job.title
                )
            })
        });
    match description {
        Some(description) if queued > 0 => t!(
            note = "The running task, then how many wait after it",
            "{description} · {queued} queued",
            description = description,
            queued = queued
        ),
        Some(description) => description,
        None if checking => t!("Checking background tasks…"),
        None if waiting > 0 => tn!("# task needs attention", "# tasks need attention", waiting),
        None => t!(
            note = "The status bar when nothing runs in the background",
            "Ready"
        ),
    }
}

#[component]
pub fn StatusBar() -> Element {
    let background = use_background();
    let jobs = use_jobs();
    let background = background.read();
    let (status, unavailable, checking) = match &*background {
        Some(Ok(status)) => (status.clone(), false, false),
        Some(Err(_)) => (None, true, false),
        None => (None, false, true),
    };
    let tasks = status
        .as_ref()
        .map(|status| status.background.tasks.clone())
        .unwrap_or_default();
    let version = status
        .as_ref()
        .map_or(env!("CARGO_PKG_VERSION"), |status| status.version.as_str())
        .to_owned();
    let jobs = jobs
        .read()
        .as_ref()
        .and_then(|jobs| jobs.as_ref().ok().cloned().flatten())
        .unwrap_or_default();
    let busy = !unavailable
        && (!tasks.is_empty()
            || jobs
                .iter()
                .any(|job| is_running(job) || job.status == JobStatus::Queued));
    let label = activity_label(&tasks, &jobs, unavailable, checking);
    let details: Vec<String> = tasks
        .iter()
        .map(|task| {
            if task.queued {
                t!(
                    note = "A background task waiting for its turn",
                    "Queued: {task}",
                    task = task.label
                )
            } else {
                task.label.clone()
            }
        })
        .collect();
    let title = if details.is_empty() {
        label.clone()
    } else {
        details.join("\n")
    };
    rsx! {
        // Moves the window like the title bar; the links keep their clicks.
        footer {
            "data-cs": "status-bar",
            "data-window-drag": "true",
            class: "relative z-10 flex min-h-9 shrink-0 items-center gap-4 border-t border-border bg-surface-raised px-4 py-2 text-xs text-fg-muted",
            output {
                "aria-live": "polite",
                "aria-atomic": "true",
                class: "flex min-w-0 flex-1 items-center gap-2",
                title: "{title}",
                if busy {
                    Svg {
                        icon: icons::LOADER_CIRCLE,
                        part: "spinner",
                        class: "size-3.5 shrink-0 animate-worker text-accent-alt motion-reduce:animate-none",
                    }
                }
                span { "data-part": "activity", class: "truncate", "{label}" }
            }
            SyncStatus {}
            UpdateLink {}
            span { "data-part": "version", class: "shrink-0 font-mono text-fg-subtle",
                {t!("Caskade v{version}", version = version)}
            }
        }
    }
}

/// Paired devices in reach, best link first (`api/sync.ts`'s `reachable`).
pub fn reachable(devices: &[PairedDevice]) -> Vec<&PairedDevice> {
    let rank = |link: DeviceLink| match link {
        DeviceLink::Lan => 0,
        DeviceLink::Direct => 1,
        DeviceLink::Relay => 2,
        DeviceLink::Offline => 3,
    };
    let mut reached: Vec<&PairedDevice> = devices
        .iter()
        .filter(|device| device.link != DeviceLink::Offline)
        .collect();
    reached.sort_by_key(|device| rank(device.link));
    reached
}

fn link_name(link: DeviceLink) -> &'static str {
    match link {
        DeviceLink::Lan => "lan",
        DeviceLink::Direct => "direct",
        DeviceLink::Relay => "relay",
        DeviceLink::Offline => "offline",
    }
}

/// With paired devices (#231): how many are in reach and the best way one
/// is reached, or that they're out of reach; to Settings → Devices.
#[component]
fn SyncStatus() -> Element {
    let devices = use_devices();
    let devices = devices
        .read()
        .as_ref()
        .and_then(|status| status.as_ref().ok().cloned().flatten())
        .map(|status| status.devices)
        .unwrap_or_default();
    if devices.is_empty() {
        return rsx! {};
    }
    let in_reach = reachable(&devices);
    let syncing = devices.iter().any(|device| device.syncing);
    let best = in_reach
        .first()
        .map_or(DeviceLink::Offline, |device| device.link);
    let count = in_reach.len();
    rsx! {
        RouteLink {
            to: "/settings?tab=devices",
            // Its own `data-status` says how devices are reached.
            current: false,
            "data-part": "sync",
            "data-status": if syncing { "syncing" } else { link_name(best) },
            "aria-label": t!("Devices: open Settings → Devices"),
            class: "flex shrink-0 items-center gap-1.5 rounded-sm hover:text-fg data-focus-visible:focus-ring",
            if syncing {
                Svg {
                    icon: icons::LOADER_CIRCLE,
                    class: "size-3.5 animate-worker text-accent-alt motion-reduce:animate-none",
                }
            }
            span {
                if count == 0 {
                    {t!("Devices out of reach")}
                } else {
                    {tn!("# device in reach", "# devices in reach", count)}
                }
            }
        }
    }
}

/// A new Caskade, once a check has found one: to Settings → General,
/// where it installs.
#[component]
fn UpdateLink() -> Element {
    let update = use_app_update();
    let update = update
        .read()
        .as_ref()
        .and_then(|status| status.as_ref().ok().cloned().flatten());
    let Some(status) = update else {
        return rsx! {};
    };
    let Some(version) = status
        .release
        .as_ref()
        .map(|release| release.version.clone())
    else {
        return rsx! {};
    };
    let (phase, label) = match status.phase {
        AppUpdatePhase::Available => (
            "available",
            t!("Caskade v{version} available", version = version),
        ),
        AppUpdatePhase::Ready => (
            "ready",
            t!("Restart for Caskade v{version}", version = version),
        ),
        _ => return rsx! {},
    };
    rsx! {
        RouteLink {
            to: "/settings?tab=general&section=updates",
            current: false,
            "data-part": "update",
            "data-status": phase,
            class: "flex shrink-0 items-center gap-1.5 rounded-sm text-accent-alt hover:text-fg data-focus-visible:focus-ring",
            Svg { icon: icons::CIRCLE_ARROW_UP, class: "size-3.5" }
            "{label}"
        }
    }
}
