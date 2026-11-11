//! Detect emulators (#115), as the webview's `DetectionReview`: searching
//! the usual places and an optional folder for installed emulators, and
//! importing the ones chosen. Detection doesn't launch anything.

use std::collections::BTreeMap;

use caskade_core::emulation::{DetectJob, DetectStatus};
use caskade_core::error::ErrorReport;
use caskade_ui_i18n::{t, tn};
use dioxus_native::prelude::*;

use super::fields::{PathField, install_kind_label, install_location};
use super::{use_emulation, use_live};
use crate::data::change;
use crate::ui::icons::{SCAN_SEARCH, X};
use crate::ui::{Button, ButtonSize, ButtonVariant, Checkbox, SettingsSection};

/// What the detection says, in a line.
fn message(job: Option<&DetectJob>) -> String {
    let Some(job) = job else {
        return t!("No detection run yet.");
    };
    let found = job.results.len();
    match job.status {
        DetectStatus::Running => t!("Searching… {found} found so far", found = found),
        DetectStatus::Cancelled => t!("Detection cancelled · {found} found", found = found),
        _ => tn!(
            "# emulator installation found",
            "# emulator installations found",
            found
        ),
    }
}

/// The detection, with Search again, Cancel and what it found.
#[component]
pub fn DetectionReview(on_close: EventHandler<()>) -> Element {
    let emulation = use_emulation();
    let live = use_live();
    let mut folder = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let job = live.detection.read().clone();
    let running = job
        .as_ref()
        .is_some_and(|job| job.status == DetectStatus::Running);
    let search = {
        let emulation = emulation.clone();
        move |_| {
            let emulation = emulation.clone();
            let wanted = folder().trim().to_owned();
            let mut detection = live.detection;
            pending.set(true);
            failure.set(None);
            spawn(async move {
                let wanted = (!wanted.is_empty()).then_some(wanted);
                match change(async move { emulation.detect(wanted).await }).await {
                    Ok(job) => detection.set(Some(job)),
                    Err(report) => failure.set(Some(report)),
                }
                pending.set(false);
            });
        }
    };
    let cancel = {
        let emulation = emulation.clone();
        let id = job.as_ref().map(|job| job.id.clone());
        move |_| {
            let Some(id) = id.clone() else { return };
            let mut detection = live.detection;
            match emulation.cancel_detection(&id) {
                Ok(job) => detection.set(Some(job)),
                Err(report) => failure.set(Some(report)),
            }
        }
    };
    let error = failure()
        .map(|report| report.summary)
        .or_else(|| job.as_ref().and_then(|job| job.error.clone()))
        .or_else(|| {
            job.as_ref()
                .filter(|job| job.status == DetectStatus::Failed)
                .map(|_| t!("Emulator detection failed."))
        });
    let status = if running {
        format!("{} · {}", message(job.as_ref()), t!("Searching…"))
    } else {
        message(job.as_ref())
    };
    rsx! {
        SettingsSection {
            icon: SCAN_SEARCH,
            title: t!("Detect emulators"),
            description: t!("Reads standard locations and the optional folder. Detection does not launch emulators."),
            aside: rsx! {
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    icon: X,
                    "aria-label": t!("Close emulator detection"),
                    on_press: move |_| on_close.call(()),
                }
            },
            div { class: "mt-3 flex flex-wrap items-end gap-3",
                div { class: "min-w-72 flex-1",
                    PathField {
                        id: "detect-folder",
                        label: t!("Extra folder"),
                        value: folder(),
                        on_change: move |next| folder.set(next),
                    }
                }
                Button { id: "detect-again", disabled: running || pending(), on_press: search, {t!("Search again")} }
                Button { id: "detect-cancel", disabled: !running || pending(), on_press: cancel, {t!("Cancel detection")} }
            }
            p { role: "status", class: "mt-3 text-sm text-fg-muted", "{status}" }
            if let Some(error) = error {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
            if let Some(job) = job {
                Results { key: "{job.id}", job, on_close }
            }
        }
    }
}

#[component]
fn Results(job: DetectJob, on_close: EventHandler<()>) -> Element {
    let emulation = use_emulation();
    let mut picked = use_signal(BTreeMap::<String, bool>::new);
    let mut importing = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let searching = job.status == DetectStatus::Running;
    let chosen: Vec<String> = job
        .results
        .iter()
        .filter(|row| {
            !row.configured
                && picked
                    .read()
                    .get(&row.key)
                    .copied()
                    .unwrap_or_else(|| row.profiles.iter().any(|profile| profile.available))
        })
        .map(|row| row.key.clone())
        .collect();
    let import = {
        let chosen = chosen.clone();
        let id = job.id.clone();
        let revision = job.revision;
        move |_| {
            let emulation = emulation.clone();
            let (chosen, id) = (chosen.clone(), id.clone());
            importing.set(true);
            failure.set(None);
            spawn(async move {
                let done =
                    change(async move { emulation.import_detected(&id, revision, &chosen).await })
                        .await;
                importing.set(false);
                match done {
                    Ok(_) => on_close.call(()),
                    Err(report) => failure.set(Some(report)),
                }
            });
        }
    };
    let label = if importing() {
        t!("Importing…")
    } else if chosen.is_empty() {
        t!("Import selected")
    } else {
        t!("Import selected ({count})", count = chosen.len())
    };
    rsx! {
        div { class: "mt-4 space-y-3",
            if job.results.is_empty() {
                p { class: "rounded-control border border-dashed border-border p-5 text-sm text-fg-muted",
                    if searching {
                        {t!("Waiting for detection results…")}
                    } else {
                        {t!("No matching emulators found. Try a portable emulator folder.")}
                    }
                }
            }
            ul { class: "space-y-2",
                for row in job.results.iter().cloned() {
                    li {
                        key: "{row.key}",
                        "data-part": "detected",
                        "data-configured": if row.configured { "true" } else { "false" },
                        class: "rounded-control border border-border bg-surface-overlay/40 p-3",
                        Checkbox {
                            checked: chosen.contains(&row.key),
                            disabled: row.configured || importing() || searching,
                            label: {
                                let mut label = row.name.clone();
                                if let Some(version) = &row.version {
                                    label.push_str(&format!(" · {version}"));
                                }
                                if row.configured {
                                    label.push_str(" · ");
                                    label.push_str(&t!(note = "A detected emulator Caskade already has", "Already configured"));
                                }
                                label
                            },
                            on_change: {
                                let key = row.key.clone();
                                move |selected: bool| {
                                    picked.write().insert(key.clone(), selected);
                                }
                            },
                        }
                        p { class: "mt-1 break-all font-mono text-xs text-fg-muted",
                            {format!("{} · {}", install_kind_label(&row.install), install_location(&row.install))}
                        }
                        ul { class: "mt-2 flex flex-wrap gap-2 text-xs text-fg-muted",
                            for profile in row.profiles.iter() {
                                li { key: "{profile.name}",
                                    {
                                        let state = if profile.available {
                                            t!(note = "A detected emulator's profile", "Available")
                                        } else {
                                            t!("Missing required files")
                                        };
                                        format!("{}: {state}", profile.name)
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
            div { class: "flex justify-end gap-2",
                Button { disabled: importing(), on_press: move |_| on_close.call(()), {t!("Close")} }
                Button {
                    id: "detect-import",
                    variant: ButtonVariant::Primary,
                    disabled: job.status != DetectStatus::Done || chosen.is_empty() || importing(),
                    on_press: import,
                    "{label}"
                }
            }
        }
    }
}
