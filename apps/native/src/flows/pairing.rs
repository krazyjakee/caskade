//! Pairing over any view (#226, #231), as the webview's `PairingHost`: the
//! code of a pairing this device started or another device started, for
//! both users to compare and confirm; and a `caskade://pair` link opened
//! from outside, which pairs only when an admin says so here (a web page
//! can open such a link). Mounted in the shell, inside the input root, so a
//! pad reaches every control; in console mode the code is large enough to
//! read from a sofa.

use caskade_core::devices::{Pairing as Pair, PairingLinkTarget, PairingState};
use caskade_core::error::ErrorReport;
use caskade_ui_data::pairing::Pairing;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use crate::data::change;
use crate::ui::{Button, ButtonVariant, Dialog, FailureNotice, NoticeVariant};

/// The app's devices for the pairing host: a test's, else the running
/// app's; `None` before its services start.
pub fn use_pairing() -> Option<Pairing> {
    try_use_context::<Pairing>().or_else(|| crate::shell::services().map(Pairing::of))
}

/// The pairing dialogs, over whatever view is shown. `console` makes the
/// code large.
#[component]
pub fn PairingHost(pairing: Pairing, #[props(default)] console: bool) -> Element {
    let mut current = use_signal(|| None::<Pair>);
    let mut opened = use_signal(|| None::<PairingLinkTarget>);
    let mut admin = use_signal(|| false);
    // A pairing dismissed here stays out of sight until the core forgets it.
    let mut dismissed = use_signal(|| None::<String>);
    {
        let pairing = pairing.clone();
        use_hook(move || {
            let mut changes = pairing.changes();
            spawn(async move {
                loop {
                    let read = pairing.clone();
                    let read = change(async move {
                        let status = read.status().await?;
                        let link = read.opened_link().await?;
                        Ok((status, link, read.is_admin().await))
                    })
                    .await;
                    // A failed read keeps what's shown until the next change.
                    if let Ok((status, link, is_admin)) = read {
                        if status.pairing.is_none() {
                            dismissed.set(None);
                        }
                        current.set(status.pairing);
                        opened.set(link);
                        admin.set(is_admin);
                    }
                    if changes.changed().await.is_err() {
                        break;
                    }
                }
            });
        });
    }
    if let Some(pair) = current()
        && dismissed().as_deref() != Some(pair.device_id.as_str())
    {
        let id = pair.device_id.clone();
        return rsx! {
            PairDialog {
                key: "{pair.device_id}:{pair.state:?}",
                pairing: pairing.clone(),
                pair,
                admin: admin(),
                console,
                on_dismiss: move |_| dismissed.set(Some(id.clone())),
            }
        };
    }
    if let Some(target) = opened() {
        return rsx! {
            OpenedLinkDialog {
                pairing: pairing.clone(),
                target,
                admin: admin(),
                on_done: move |_| opened.set(None),
            }
        };
    }
    rsx! {}
}

#[component]
fn PairDialog(
    pairing: Pairing,
    pair: Pair,
    admin: bool,
    console: bool,
    on_dismiss: EventHandler<()>,
) -> Element {
    let mut failure = use_signal(|| None::<ErrorReport>);
    let mut confirming = use_signal(|| false);
    let name = pair.name.clone();
    let waiting = pair.state == PairingState::Waiting;
    let id = pair.device_id.clone();
    // A finished pairing is dismissed here and in the core.
    let close = {
        let pairing = pairing.clone();
        let id = id.clone();
        move |_| {
            if admin {
                let pairing = pairing.clone();
                let id = id.clone();
                spawn(async move {
                    let _ = change(async move { pairing.cancel(&id).await }).await;
                });
            }
            on_dismiss.call(());
        }
    };
    let confirm = {
        let pairing = pairing.clone();
        let id = id.clone();
        move |_| {
            let pairing = pairing.clone();
            let id = id.clone();
            confirming.set(true);
            spawn(async move {
                if let Err(report) = change(async move { pairing.confirm(&id).await }).await {
                    failure.set(Some(report));
                }
                confirming.set(false);
            });
        }
    };
    // Cancels it, then dismisses it: whoever cancelled needs no word that
    // it was cancelled.
    let refuse = {
        let pairing = pairing.clone();
        let id = id.clone();
        move |_| {
            let pairing = pairing.clone();
            let id = id.clone();
            spawn(async move {
                let _ = change(async move {
                    pairing.cancel(&id).await?;
                    pairing.cancel(&id).await
                })
                .await;
            });
            on_dismiss.call(());
        }
    };
    let heading = match pair.state {
        PairingState::Paired => t!("Paired with {name}", name = name.clone()),
        PairingState::Cancelled => t!("Pairing with {name} was cancelled", name = name.clone()),
        PairingState::Failed => t!("Pairing with {name} didn't finish", name = name.clone()),
        _ if pair.incoming => t!("{name} wants to pair", name = name.clone()),
        _ => t!("Pair with {name}?", name = name.clone()),
    };
    let code = format!(
        "{} {}",
        pair.code.get(..3).unwrap_or(&pair.code),
        pair.code.get(3..).unwrap_or("")
    );
    let state = match pair.state {
        PairingState::Waiting => "waiting",
        PairingState::Paired => "paired",
        PairingState::Cancelled => "cancelled",
        PairingState::Failed => "failed",
    };
    rsx! {
        Dialog {
            title: heading,
            on_close: close.clone(),
            "data-cs": "pair-device",
            "data-focus-scope": "pair-device",
            div { "data-part": "state", "data-status": state, class: "space-y-4",
                if waiting {
                    p { class: "text-sm text-fg-muted",
                        {t!("{name} shows a code too. Pair only if it is the same as this one.", name = name.clone())}
                    }
                    p {
                        "data-part": "code",
                        class: if console { "text-center font-mono text-7xl font-semibold tracking-[0.3em]" } else { "text-center font-mono text-4xl font-semibold tracking-[0.3em]" },
                        "{code}"
                    }
                    if pair.confirmed {
                        p { role: "status", class: "text-center text-sm text-fg-muted",
                            {t!("Waiting for {name} to confirm…", name = name.clone())}
                        }
                    }
                    if !admin {
                        p { class: "text-sm text-fg-muted",
                            {t!("Only an admin can pair devices. Ask one to confirm here.")}
                        }
                    }
                }
                if pair.state == PairingState::Paired {
                    p { class: "text-sm text-fg-muted",
                        {t!("The first sync starts now. Your library, play history and collections show on both devices; each keeps its own games and settings.")}
                    }
                }
                if pair.state == PairingState::Cancelled {
                    p { class: "text-sm text-fg-muted", {t!("Nothing was paired. Start again from either device.")} }
                }
                if pair.state == PairingState::Failed {
                    p { class: "text-sm text-fg-muted",
                        {t!("The devices lost each other, or nobody confirmed in time. Nothing was paired.")}
                    }
                }
                if let Some(report) = failure() {
                    FailureNotice { error: report, variant: NoticeVariant::Inline }
                }
                div { class: "flex justify-end gap-2",
                    if waiting && admin {
                        Button { id: "pair-refuse", on_press: refuse, {t!("They don't match")} }
                        Button {
                            id: "pair-confirm",
                            variant: ButtonVariant::Primary,
                            "data-focus-default": "true",
                            disabled: confirming() || pair.confirmed,
                            on_press: confirm,
                            {t!("They match: pair")}
                        }
                    } else {
                        Button {
                            id: "pair-close",
                            variant: ButtonVariant::Primary,
                            "data-focus-default": "true",
                            on_press: close,
                            if waiting { {t!("Close")} } else { {t!("Done")} }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn OpenedLinkDialog(
    pairing: Pairing,
    target: PairingLinkTarget,
    admin: bool,
    on_done: EventHandler<()>,
) -> Element {
    let mut paired = use_signal(|| None::<String>);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let mut accepting = use_signal(|| false);
    let device = target
        .device_id
        .get(..8)
        .unwrap_or(&target.device_id)
        .to_owned();
    let until = crate::library::date_time(target.expires_at_ms);
    let close = {
        let pairing = pairing.clone();
        move |_| {
            let pairing = pairing.clone();
            spawn(async move {
                let _ = change(async move { pairing.dismiss_opened_link().await }).await;
            });
            on_done.call(());
        }
    };
    let accept = {
        let pairing = pairing.clone();
        move |_| {
            let pairing = pairing.clone();
            accepting.set(true);
            failure.set(None);
            spawn(async move {
                match change(async move { pairing.accept_opened_link().await }).await {
                    Ok(Some(device)) => paired.set(Some(device.name)),
                    Ok(None) => {}
                    Err(report) => failure.set(Some(report)),
                }
                accepting.set(false);
            });
        }
    };
    let heading = match paired() {
        Some(name) => t!("Paired with {name}", name = name),
        None => t!("Pair with the device from this link?"),
    };
    rsx! {
        Dialog {
            title: heading,
            on_close: close.clone(),
            "data-cs": "pair-device",
            "data-focus-scope": "pair-device",
            div {
                "data-part": "link",
                "data-status": if target.expired { "expired" } else { "valid" },
                class: "space-y-4",
                if paired().is_none() {
                    p { class: "text-sm text-fg-muted",
                        if target.expired {
                            {t!("This pairing link has expired. Make a new one on the other device (it starts with {device}).", device = device.clone())}
                        } else {
                            {t!("A pairing link was opened. Pairing hands the device that made it (it starts with {device}) your library and accounts. The link works until {until}.", device = device.clone(), until = until.clone())}
                        }
                    }
                    if !admin {
                        p { class: "text-sm text-fg-muted", {t!("Only an admin can pair devices.")} }
                    }
                }
                if let Some(report) = failure() {
                    FailureNotice { error: report, variant: NoticeVariant::Inline }
                }
                div { class: "flex justify-end gap-2",
                    if paired().is_some() {
                        Button { id: "pair-link-done", variant: ButtonVariant::Primary, "data-focus-default": "true", on_press: close, {t!("Done")} }
                    } else {
                        Button {
                            id: "pair-link-not-now",
                            "data-focus-default": (!admin || target.expired).then_some("true"),
                            on_press: close,
                            {t!("Not now")}
                        }
                        if admin && !target.expired {
                            Button {
                                id: "pair-link-accept",
                                variant: ButtonVariant::Primary,
                                "data-focus-default": "true",
                                disabled: accepting(),
                                on_press: accept,
                                {t!("Pair")}
                            }
                        }
                    }
                }
            }
        }
    }
}
