//! Install requests (#101), as the webview's `InstallIngress` and
//! `InstallConfirmation`: a deep link, a file dropped outside the library,
//! a share link or a game another device sends waits, inert, until the
//! user confirms it here; closing dismisses it and never starts a download.
//! A confirmed install asks where it goes, then its job is followed, so a
//! choice it needs opens where the user is. Dropped on the library, a file
//! is the library's (`library/view.rs`); it still confirms here.

use caskade_app::commands::install_requests::{InputKind, InstallRequest, PeerSend};
use caskade_core::install::dropped::{DropKind, DropPlan};
use caskade_ui_data::shares::ShareContents;
use caskade_ui_i18n::{format, t, tp};
use dioxus_native::prelude::*;

use crate::data::{change, use_install_requests_query, use_jobs_handle};
use crate::flows::shares_common::ShareInfoDetails;
use crate::flows::storage::{InstallRootPicker, use_storage};
use crate::router::{Route, use_router};
use crate::ui::{Button, ButtonVariant, Dialog, use_toasts};

fn report(message: String) -> caskade_ui_data::ErrorReport {
    caskade_ui_data::ErrorReport::new(caskade_ui_data::ErrorCode::Other, message)
}

fn is_collection(request: &InstallRequest) -> bool {
    request
        .share
        .as_ref()
        .is_some_and(|share| matches!(share.contents, ShareContents::Collection { .. }))
}

/// Shows the oldest install request waiting, and takes files dropped
/// outside the library. Mounted once, over every view.
#[component]
pub fn InstallIngress() -> Element {
    let mut requests = use_install_requests_query();
    let handle = use_jobs_handle().filter(|jobs| jobs.takes_requests());
    let storage = use_storage();
    let router = use_router();
    let toasts = use_toasts();
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    // The request waiting for where it installs.
    let mut placing = use_signal(|| None::<String>);
    use_hook({
        let handle = handle.clone();
        move || {
            let Some(handle) = handle else {
                return;
            };
            let mut drops = crate::drops::drops().subscribe();
            spawn(async move {
                loop {
                    let paths = match drops.recv().await {
                        Ok(crate::drops::DropEvent::Dropped(paths)) => paths,
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    // The library has its own drop zone.
                    if matches!(router.peek(), Route::Library(_)) {
                        continue;
                    }
                    for path in paths {
                        let handle = handle.clone();
                        let path = path.display().to_string();
                        let asked =
                            change(
                                async move { handle.request_dropped(path).await.map_err(report) },
                            )
                            .await;
                        match asked {
                            Ok(_) => requests.restart(),
                            Err(error) => {
                                toasts.show(error.summary, true, None);
                            }
                        }
                    }
                }
            });
        }
    });
    let finish = use_callback({
        let handle = handle.clone();
        let has_storage = storage.is_some();
        move |(request, accept, root_id): (InstallRequest, bool, Option<String>)| {
            let Some(handle) = handle.clone() else {
                return;
            };
            let collection = is_collection(&request);
            // A shared collection is a list: nothing installs, so no
            // storage (#247).
            if accept && !collection && has_storage && root_id.is_none() {
                placing.set(Some(request.request_id.clone()));
                return;
            }
            busy.set(true);
            error.set(None);
            spawn(async move {
                let id = request.request_id.clone();
                let outcome = change(async move {
                    let followed = if !accept {
                        handle.dismiss_request(&id).map(|()| None)
                    } else if collection {
                        handle.add_shared_collection(&id).await.map(|()| None)
                    } else {
                        handle
                            .confirm_request(&id, root_id)
                            .await
                            .map(|job| Some(job.id))
                    };
                    followed.map_err(report)
                })
                .await;
                busy.set(false);
                match outcome {
                    Ok(job) => {
                        placing.set(None);
                        // A choice it needs (an asset, an installer)
                        // opens where the user is.
                        if let Some(job) = job {
                            super::follow(&job);
                        }
                    }
                    Err(failed) => error.set(Some(failed.summary)),
                }
                requests.restart();
            });
        }
    });
    if handle.is_none() {
        return rsx! {};
    }
    let Some(Ok(list)) = requests.read().clone() else {
        return rsx! {};
    };
    let Some(request) = list.first().cloned() else {
        return rsx! {};
    };
    if placing().as_deref() == Some(request.request_id.as_str())
        && let Some(storage) = storage
    {
        return rsx! {
            Dialog {
                title: t!("Choose installation storage"),
                class: "max-w-xl!",
                on_close: move |_| {
                    if !busy() {
                        placing.set(None);
                    }
                },
                if let Some(message) = error() {
                    p { role: "alert", class: "mb-3 text-sm text-danger", "{message}" }
                }
                div { "aria-busy": if busy() { "true" } else { "false" },
                    InstallRootPicker {
                        storage,
                        heading: false,
                        on_cancel: move |_| placing.set(None),
                        on_select: {
                            let request = request.clone();
                            move |root_id: String| {
                                if !busy() {
                                    finish((request.clone(), true, Some(root_id)));
                                }
                            }
                        },
                    }
                }
            }
        };
    }
    rsx! {
        InstallConfirmation {
            key: "{request.request_id}",
            request: request.clone(),
            busy: busy(),
            error: error(),
            on_answer: move |accept: bool| finish((request.clone(), accept, None)),
        }
    }
}

/// What a request asks, and Dismiss or its confirmation. Closing
/// dismisses.
#[component]
fn InstallConfirmation(
    request: InstallRequest,
    busy: bool,
    error: Option<String>,
    on_answer: EventHandler<bool>,
) -> Element {
    let title = match (&request.share, &request.peer, &request.plan) {
        (Some(share), _, _) => t!("Receive {shared}?", shared = share.title.clone()),
        (None, Some(peer), _) => t!(
            "Copy {title} from {device}?",
            title = peer.title.clone(),
            device = peer.device_name.clone()
        ),
        (
            None,
            None,
            Some(DropPlan {
                title: Some(title), ..
            }),
        ) => {
            t!("Install {title}?", title = title.clone())
        }
        _ => t!("Install this game?"),
    };
    let confirm = if is_collection(&request) {
        t!("Add to my collections")
    } else if request.share.is_some() {
        tp!("button", "Receive")
    } else if request.peer.is_some() {
        t!("Copy here")
    } else if request.plan.is_some() {
        tp!(note = "Starts an install", "button", "Install")
    } else {
        t!("Confirm installation")
    };
    let external = request.share.is_none() && request.peer.is_none() && request.plan.is_none();
    rsx! {
        Dialog {
            title,
            "data-cs": "install-confirmation",
            "data-focus-scope": "install-confirmation",
            "data-kind": match request.kind {
                InputKind::Path => "path",
                InputKind::Url => "url",
                InputKind::Peer => "peer",
                InputKind::Share => "share",
            },
            class: "max-w-xl!",
            on_close: move |_| {
                if !busy {
                    on_answer.call(false);
                }
            },
            if let Some(share) = request.share.clone() {
                div { "data-part": "share", class: "flex flex-col gap-3",
                    ShareInfoDetails { info: share }
                    p { class: "text-sm text-fg-muted",
                        {t!("It comes straight from their computer and installs like any other game; nothing is written until you say so.")}
                    }
                }
            } else if let Some(peer) = request.peer.clone() {
                PeerDetails { peer }
            } else if let Some(plan) = request.plan.clone() {
                PlanDetails { plan }
            }
            if external {
                p { class: "text-sm text-fg-muted",
                    if request.kind == InputKind::Url {
                        {t!("An external link asked Caskade to install from this URL. Confirm only if you trust its source.")}
                    } else {
                        {t!("An external link asked Caskade to install from this path. Confirm only if you trust its source.")}
                    }
                }
                p {
                    "data-part": "source",
                    class: "mt-4 max-h-40 overflow-auto break-all rounded-control border border-border p-3 font-mono text-xs",
                    "{request.value}"
                }
            }
            if request.kind == InputKind::Url {
                p { class: "mt-3 text-sm text-fg-muted",
                    {t!("Nothing will be downloaded until you confirm. If the download is an installer program, Caskade runs it, unless Settings → Installer asks first for downloaded installers.")}
                }
            }
            if let Some(message) = error {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{message}" }
            }
            div { class: "mt-6 flex justify-end gap-2",
                Button {
                    "data-part": "dismiss",
                    "data-spatial": "focusable",
                    disabled: busy,
                    on_press: move |_| on_answer.call(false),
                    {t!("Dismiss")}
                }
                Button {
                    "data-part": "confirm",
                    "data-spatial": "focusable",
                    variant: ButtonVariant::Primary,
                    disabled: busy,
                    on_press: move |_| on_answer.call(true),
                    "{confirm}"
                }
            }
        }
    }
}

/// A game another of the user's devices sends here (#235): nothing is
/// written until it's confirmed.
#[component]
fn PeerDetails(peer: PeerSend) -> Element {
    let device = peer.device_name.clone();
    let title = peer.title.clone();
    let text = match peer.size_bytes {
        Some(bytes) => t!(
            "{device} sends {title} ({size}) to this computer. It's copied straight from {device} and installed like any other game; nothing is written until you say so.",
            device = device,
            title = title,
            size = format::size(Some(bytes))
        ),
        None => t!(
            "{device} sends {title} to this computer. It's copied straight from {device} and installed like any other game; nothing is written until you say so.",
            device = device,
            title = title
        ),
    };
    rsx! {
        p { "data-part": "peer", class: "text-sm text-fg-muted", "{text}" }
    }
}

/// What a dropped file or folder installs as, worked out by the core.
#[component]
fn PlanDetails(plan: DropPlan) -> Element {
    let summary = match plan.kind {
        DropKind::Shortcut => {
            t!("Caskade installs what this shortcut opens, with the shortcut's options.")
        }
        DropKind::Executable => {
            t!("Caskade installs the game folder this program is in and launches it from there.")
        }
        DropKind::Folder => t!("Caskade installs this folder and picks the game's executable."),
        DropKind::File => t!(
            "Caskade works out how to install this file: an installer runs, an archive is unpacked."
        ),
    };
    let mut rows = Vec::new();
    if !plan.dropped.is_empty() && plan.dropped != plan.input {
        rows.push((
            t!(note = "The file or folder dropped on Caskade", "Dropped"),
            plan.dropped.clone(),
        ));
    }
    rows.push((
        t!(
            note = "The file or folder an install request installs",
            "Installs"
        ),
        plan.input.clone(),
    ));
    if let Some(launch) = &plan.launch {
        let mut command = vec![launch.path.clone()];
        command.extend(launch.args.iter().cloned());
        rows.push((
            t!(
                note = "The command an installed game launches with",
                "Launches"
            ),
            command.join(" "),
        ));
    }
    rsx! {
        p { class: "text-sm text-fg-muted", "{summary}" }
        dl {
            "data-part": "plan",
            class: "mt-4 grid max-h-48 grid-cols-[auto_1fr] gap-x-3 gap-y-2 overflow-auto rounded-control border border-border p-3 text-xs",
            for (label, value) in rows {
                dt { key: "{label}", class: "text-fg-subtle", "{label}" }
                dd { class: "break-all font-mono", "{value}" }
            }
        }
    }
}
