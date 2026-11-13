//! An install's updates on the game page (#100, the webview's
//! `views/game/EntryUpdate.tsx`): the release installed, a newer one when
//! there is, and Update, Skip this version (or Offer it again), Check now,
//! What's new or Releases, and Prereleases. Nothing for installs no update
//! provider follows.

use super::data::{UpdateChange, use_page_data};
use super::launch::version_label;
use crate::ui::{Button, ButtonSize, ButtonVariant, Switch, icons};
use caskade_core::install::{JobPhase, JobStatus};
use caskade_core::updates::GameUpdate;
use caskade_ui_i18n::{format, t};
use dioxus_native::prelude::*;

/// The update job's progress, then how it ended.
#[component]
pub fn UpdateProgress(job_id: String) -> Element {
    let jobs = crate::data::use_jobs_query();
    let job = jobs
        .read()
        .as_ref()
        .and_then(|jobs| jobs.as_ref().ok().cloned())
        .and_then(|jobs| jobs.into_iter().find(|each| each.id == job_id));
    let Some(job) = job else {
        return rsx! {};
    };
    let text = match job.status {
        JobStatus::Completed => {
            return rsx! { p { role: "status", class: "text-fg-muted", {t!("Updated.")} } };
        }
        JobStatus::Failed | JobStatus::Cancelled => {
            let error = job.error.clone().unwrap_or_default();
            let message = if job.status == JobStatus::Cancelled {
                t!("The update was cancelled.")
            } else {
                t!("Update failed: {error}", error = error)
            };
            return rsx! { p { role: "alert", class: "text-danger", "{message}" } };
        }
        JobStatus::NeedsUser => t!("The update needs a choice in Activity."),
        _ => {
            let downloading = job.phase == Some(JobPhase::Download);
            let percent = job.progress.as_ref().and_then(|progress| {
                progress
                    .total_bytes
                    .filter(|&total| total > 0)
                    .map(|total| progress.bytes * 100 / total)
            });
            match (percent, downloading) {
                (None, true) => t!("Downloading the update…"),
                (None, false) => t!("Installing the update…"),
                (Some(percent), true) => {
                    t!("Downloading the update… {percent}%", percent = percent)
                }
                (Some(percent), false) => {
                    t!("Installing the update… {percent}%", percent = percent)
                }
            }
        }
    };
    rsx! { p { role: "status", class: "text-fg-muted", "{text}" } }
}

#[component]
pub fn EntryUpdate(update: GameUpdate, is_running: bool) -> Element {
    let data = use_page_data();
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut job = use_signal(|| None::<String>);
    let entry_id = update.entry_id.clone();
    let latest = update.latest.clone();
    let latest_version = latest.as_deref().map(version_label).unwrap_or_default();
    let installed_version = update.installed.as_deref().map(version_label);
    let state = {
        let mut parts = vec![match &installed_version {
            Some(installed) => t!(
                note = "{installedVersion} is a version, e.g. v1.2",
                "{installedVersion} · up to date",
                installedVersion = installed.clone()
            ),
            None => t!("Not checked yet"),
        }];
        if update.checked_at_ms > 0 {
            parts.push(t!(
                note = "When updates were last checked for; {checked} is a date",
                "checked {checked}",
                checked = format::day(crate::library::local_date_time(update.checked_at_ms))
            ));
        }
        parts.join(" · ")
    };
    let skipped_latest = update.skipped.is_some() && update.skipped == latest;
    let change = {
        let data = data.clone();
        let entry_id = entry_id.clone();
        move |what: UpdateChange| {
            let (data, entry_id) = (data.clone(), entry_id.clone());
            busy.set(true);
            error.set(None);
            spawn(async move {
                if let Err(report) = (data.change_update)(entry_id, what).await {
                    error.set(Some(report.summary));
                }
                busy.set(false);
            });
        }
    };
    let install = {
        let data = data.clone();
        let entry_id = entry_id.clone();
        move |_| {
            let (data, entry_id) = (data.clone(), entry_id.clone());
            busy.set(true);
            error.set(None);
            spawn(async move {
                match (data.install_update)(entry_id).await {
                    Ok(id) => job.set(Some(id)),
                    Err(report) => error.set(Some(report.summary)),
                }
                busy.set(false);
            });
        }
    };
    let link = if update.available {
        update.latest_url.clone().map(|url| (url, t!("What's new")))
    } else {
        update.project_url.clone().map(|url| (url, t!("Releases")))
    };
    rsx! {
        div {
            "data-part": "update",
            "data-status": if update.available { "available" } else { "current" },
            class: "mt-2 flex flex-col gap-1.5 text-xs",
            if update.available && latest.is_some() {
                p { class: "font-semibold text-accent-alt",
                    match &installed_version {
                        Some(installed) => rsx! {
                            {t!(note = "A version, e.g. v1.2", "{latestVersion} is out", latestVersion = latest_version.clone())}
                            " "
                            span { class: "font-normal text-fg-muted",
                                {t!(note = "A version, e.g. v1.2", "· you have {installedVersion}", installedVersion = installed.clone())}
                            }
                        },
                        None => rsx! { {t!(note = "A version, e.g. v1.2", "{latestVersion} is out", latestVersion = latest_version.clone())} },
                    }
                }
            } else {
                p { class: "text-fg-muted", "{state}" }
            }
            if let Some(last) = update.error.clone() {
                p { class: "text-fg-muted", {t!("The last check failed: {lastError}", lastError = last)} }
            }
            if skipped_latest {
                p { class: "text-fg-muted",
                    {t!("You skipped {skipped}.", skipped = update.skipped.as_deref().map(version_label).unwrap_or_default())}
                }
            }
            div { class: "flex flex-wrap items-center gap-x-3 gap-y-2",
                if update.available {
                    Button {
                        "data-part": "install-update",
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Primary,
                        icon: Some(icons::DOWNLOAD),
                        disabled: is_running || busy() || job().is_some(),
                        on_press: install,
                        {t!(note = "Button: installs the newer release", "Update")}
                    }
                }
                if update.available && let Some(latest) = latest.clone() {
                    Button {
                        "data-part": "skip",
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Ghost,
                        disabled: busy(),
                        on_press: {
                            let mut change = change.clone();
                            move |_| change(UpdateChange::Skip(Some(latest.clone())))
                        },
                        {t!("Skip this version")}
                    }
                }
                if skipped_latest {
                    Button {
                        "data-part": "offer-again",
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Ghost,
                        disabled: busy(),
                        on_press: {
                            let mut change = change.clone();
                            move |_| change(UpdateChange::Skip(None))
                        },
                        {t!("Offer it again")}
                    }
                }
                Button {
                    "data-part": "check",
                    size: ButtonSize::Sm,
                    variant: ButtonVariant::Ghost,
                    icon: Some(icons::REFRESH_CW),
                    disabled: busy(),
                    on_press: {
                        let mut change = change.clone();
                        move |_| change(UpdateChange::Check)
                    },
                    if busy() { {t!("Checking…")} } else { {t!("Check now")} }
                }
                if let Some((url, label)) = link {
                    super::ExternalLink { href: url, class: "inline-flex items-center gap-1 text-accent-alt", "{label}" }
                }
                Switch {
                    "data-part": "prereleases",
                    checked: update.prereleases,
                    disabled: busy(),
                    label: t!("Prereleases"),
                    on_change: {
                        let mut change = change.clone();
                        move |on: bool| change(UpdateChange::Prereleases(on))
                    },
                }
            }
            if let Some(job_id) = job() {
                UpdateProgress { job_id }
            }
            if let Some(error) = error() {
                p { role: "alert", class: "text-danger", "{error}" }
            }
        }
    }
}
