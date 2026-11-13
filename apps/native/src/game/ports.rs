//! A game's fan ports on its page (#100, the webview's
//! `views/game/PortsPanel.tsx`): decompilations, recompilations and source
//! ports that run it natively from the user's own copy, from Decomp Buddy's
//! install plans, each with what it needs and Install when it has a release
//! for this system (a confirmation, then an install job). Nothing shows
//! while the catalogue is off, missing, or has none for the game.

use super::data::{use_answer, use_page_data};
use super::system_name;
use crate::ui::{Button, ButtonSize, ButtonVariant, Dialog, Svg, icons};
use caskade_core::install::JobStatus;
use caskade_core::ports::{Port, PortFileHow, PortHere, PortMatch, PortNeeds};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;

/// What a port wants from the user, and how it gets it.
#[component]
fn Need(need: PortNeeds) -> Element {
    let how = match need.how {
        PortFileHow::Folder if need.have => t!("Installing copies yours in."),
        PortFileHow::Folder => t!("Put it in the port's folder once installed."),
        PortFileHow::CommandLine if need.have => t!("Caskade gives it yours when it starts."),
        PortFileHow::CommandLine => t!("It takes the file when it starts."),
        PortFileHow::InAppPicker => t!("It asks for the file the first time it starts."),
    };
    rsx! {
        li {
            span { class: "text-fg", "{need.label}" }
            if let Some(region) = need.region.clone() {
                span { class: "text-fg-muted",
                    " "
                    {t!(note = "The region of a game file, e.g. USA", "({region} version)", region = region)}
                }
            }
            if need.kind == "game" {
                span { class: if need.have { "text-accent-alt" } else { "text-fg-muted" },
                    " · "
                    if need.have { {t!("in your library")} } else { {t!("not in your library")} }
                }
            }
            span { class: "text-fg-muted", " · {how}" }
        }
    }
}

/// A port install as it runs.
#[component]
fn PortProgress(job_id: String) -> Element {
    let jobs = crate::data::use_jobs_query();
    let job = jobs
        .read()
        .as_ref()
        .and_then(|jobs| jobs.as_ref().ok().cloned())
        .and_then(|jobs| jobs.into_iter().find(|each| each.id == job_id));
    let Some(job) = job else {
        return rsx! { p { role: "status", class: "text-fg-muted", {t!("Starting the install…")} } };
    };
    match job.status {
        JobStatus::Completed => {
            rsx! { p { class: "text-accent-alt", {t!("Installed: it's in Play from your library.")} } }
        }
        JobStatus::Failed | JobStatus::Cancelled => {
            let message = if job.status == JobStatus::Cancelled {
                t!("The install was cancelled.")
            } else {
                t!(
                    "Install failed: {error}",
                    error = job.error.clone().unwrap_or_default()
                )
            };
            rsx! { p { role: "alert", class: "text-danger", "{message}" } }
        }
        JobStatus::NeedsUser => {
            rsx! { p { class: "text-fg-muted", {t!("The install needs a choice in Activity.")} } }
        }
        _ => {
            let percent = job.progress.as_ref().and_then(|progress| {
                progress
                    .total_bytes
                    .filter(|&total| total > 0)
                    .map(|total| progress.bytes * 100 / total)
            });
            let text = match percent {
                Some(percent) => t!("Installing… {percent}%", percent = percent.to_string()),
                None => t!("Installing…"),
            };
            rsx! { p { role: "status", class: "text-fg-muted", "{text}" } }
        }
    }
}

#[component]
fn InstallPortDialog(
    port: Port,
    pending: bool,
    error: Option<String>,
    on_confirm: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let PortHere::Installable {
        asset,
        release,
        checked,
    } = port.here.clone()
    else {
        return rsx! {};
    };
    let repository = format!("{}/{}", port.owner, port.name);
    rsx! {
        Dialog {
            title: t!("Install {repository}?", repository = repository.clone()),
            on_close: move |_| {
                if !pending {
                    on_close.call(());
                }
            },
            div { class: "flex flex-col gap-4",
                p { class: "text-sm text-fg-muted",
                    {t!(
                        "Caskade downloads {asset} ({release}) from {repository} on GitHub and adds it to this game as another way to play. It's a fan project, not the publisher's: installing it runs code from that repository.",
                        asset = asset,
                        release = release,
                        repository = repository.clone()
                    )}
                }
                p { class: "text-sm text-fg-muted",
                    if checked {
                        {t!("The download is checked against the checksum in its install plan.")}
                    } else {
                        {t!("Its install plan has no checksum for the download, so it can't be checked.")}
                    }
                }
                if !port.needs.is_empty() {
                    div { class: "text-sm",
                        h3 { class: "font-semibold", {t!("It needs")} }
                        ul { class: "mt-1 list-disc pl-5",
                            for need in port.needs.clone() {
                                Need { key: "{need.label}", need }
                            }
                        }
                    }
                }
                if !port.notes.is_empty() {
                    div { class: "text-sm",
                        h3 { class: "font-semibold", {t!("Notes")} }
                        ul { class: "mt-1 list-disc pl-5 text-fg-muted",
                            for note in port.notes.clone() {
                                li { key: "{note}", "{note}" }
                            }
                        }
                    }
                }
                if !port.unsure.is_empty() {
                    div { class: "text-sm",
                        h3 { class: "font-semibold", {t!("Not certain in the plan")} }
                        ul { class: "mt-1 list-disc pl-5 text-fg-muted",
                            for note in port.unsure.clone() {
                                li { key: "{note}", "{note}" }
                            }
                        }
                    }
                }
                if let Some(error) = error {
                    p { role: "alert", class: "text-sm text-danger", "{error}" }
                }
                div { class: "flex justify-end gap-2",
                    Button { disabled: pending, on_press: move |_| on_close.call(()), {t!("Cancel")} }
                    Button {
                        "data-part": "confirm",
                        variant: ButtonVariant::Primary,
                        disabled: pending,
                        on_press: move |_| on_confirm.call(()),
                        if pending { {t!("Starting…")} } else { {tp!("port", "Install")} }
                    }
                }
            }
        }
    }
}

#[component]
fn PortRow(game_id: String, port: Port) -> Element {
    let data = use_page_data();
    let mut asking = use_signal(|| false);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut job = use_signal(|| None::<String>);
    let systems = port
        .runs_on
        .iter()
        .map(|system| system_name(system))
        .collect::<Vec<_>>()
        .join(", ");
    let mut line = format!("{} · {}", port.game_title, port.console);
    if !systems.is_empty() {
        line.push_str(" · ");
        line.push_str(&t!(
            note = "Operating systems a port has releases for, e.g. Windows, Linux",
            "runs on {systems}",
            systems = systems
        ));
    }
    let status = if port.entry_id.is_some() {
        "installed"
    } else {
        match port.here {
            PortHere::Installable { .. } => "installable",
            PortHere::BuildFromSource => "buildFromSource",
            PortHere::OtherSystems => "otherSystems",
        }
    };
    let confirm = {
        let (data, game_id, project) = (data.clone(), game_id.clone(), port.project.clone());
        move |_| {
            let (data, game_id, project) = (data.clone(), game_id.clone(), project.clone());
            pending.set(true);
            error.set(None);
            spawn(async move {
                match (data.install_port)(game_id, project).await {
                    Ok(id) => {
                        job.set(Some(id));
                        asking.set(false);
                    }
                    Err(report) => error.set(Some(report.summary)),
                }
                pending.set(false);
            });
        }
    };
    rsx! {
        li { "data-part": "port", "data-status": status, class: "rounded-control border border-border px-4 py-3 text-sm",
            div { class: "flex flex-wrap items-start justify-between gap-3",
                div { class: "flex min-w-0 flex-col gap-1",
                    p { class: "font-semibold",
                        super::ExternalLink { href: port.url.clone(), class: "inline-flex items-center gap-1 text-accent-alt", "{port.owner}/{port.name}" }
                    }
                    p { class: "text-xs text-fg-muted", "{line}" }
                    p { class: "text-xs text-fg-muted",
                        if port.matched_by == PortMatch::Igdb {
                            {t!("The same game on IGDB")}
                        } else {
                            {t!("Matched by title: check it's this game")}
                        }
                    }
                }
                div { class: "flex shrink-0 flex-col items-end gap-1",
                    if port.entry_id.is_some() {
                        span { class: "inline-flex items-center gap-1 rounded-full border border-accent px-2 py-0.5 text-xs text-accent-alt",
                            Svg { icon: icons::CIRCLE_CHECK, class: "size-3.5" }
                            {t!("Installed")}
                        }
                    } else {
                        match port.here {
                            PortHere::Installable { .. } => rsx! {
                                Button {
                                    "data-part": "install",
                                    size: ButtonSize::Sm,
                                    variant: ButtonVariant::Primary,
                                    icon: Some(icons::DOWNLOAD),
                                    disabled: pending() || job().is_some(),
                                    on_press: move |_| asking.set(true),
                                    {tp!("port", "Install")}
                                }
                            },
                            PortHere::BuildFromSource => rsx! { p { class: "text-xs text-fg-muted", {t!("Built from source on this system")} } },
                            PortHere::OtherSystems => rsx! { p { class: "text-xs text-fg-muted", {t!("No release for this system")} } },
                        }
                    }
                }
            }
            if !port.needs.is_empty() {
                ul { class: "mt-2 flex flex-col gap-0.5 text-xs",
                    for need in port.needs.clone() {
                        Need { key: "{need.label}", need }
                    }
                }
            }
            if let Some(job_id) = job() {
                div { class: "mt-2 text-xs", PortProgress { job_id } }
            }
            if asking() {
                InstallPortDialog {
                    port: port.clone(),
                    pending: pending(),
                    error: error(),
                    on_close: move |_| asking.set(false),
                    on_confirm: confirm,
                }
            }
        }
    }
}

#[component]
pub fn PortsPanel(game_id: String) -> Element {
    let data = use_page_data();
    let ports = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(DataKey::Game(game_id.clone()), move || {
            (data.ports)(game_id.clone())
        })
    };
    let Some(Ok(Some(ports))) = ports.read().as_ref().map(|result| result.clone()) else {
        return rsx! {};
    };
    if !ports.status.enabled || ports.ports.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { class: "mt-8", "aria-labelledby": "ports-heading", "data-part": "ports",
            h2 { id: "ports-heading", class: "font-display text-xl font-semibold", {t!("Ports")} }
            p { class: "mt-1 text-sm text-fg-muted",
                {t!("Fan decompilations, recompilations and source ports that run this game natively, from your own copy.")}
                " "
                super::ExternalLink { href: ports.status.credit_url.clone(), class: "inline-flex items-center gap-1 text-accent-alt",
                    {t!("Install plans from Decomp Buddy")}
                }
            }
            ul { class: "mt-4 flex flex-col gap-2",
                for port in ports.ports.clone() {
                    PortRow { key: "{port.project}", game_id: game_id.clone(), port }
                }
            }
        }
    }
}
