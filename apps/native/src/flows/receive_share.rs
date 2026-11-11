//! Add game → Receive a shared game… (#247), as the webview's
//! `ReceiveShareDialog`: a link someone shared, pasted (a pad types or
//! pastes it on the on-screen keyboard). Caskade asks the sharer what it
//! shares first; nothing comes until Receive. The game then arrives as an
//! install in Activity; a collection joins the account's collections.

use caskade_core::error::ErrorReport;
use caskade_ui_data::shares::Shares;
use caskade_ui_data::shares::{ShareContents, ShareInfo};
use caskade_ui_data::storage::Storage;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use crate::data::change;
use crate::ui::icons::SEARCH;
use crate::ui::{Button, ButtonVariant, Checkbox, Dialog, FailureNotice, NoticeVariant, TextField};

use super::shares_common::ShareInfoDetails;
use super::storage::{InstallRootPicker, Needs};

/// The app's shares for a flow: a test's or a host's, else the running
/// app's; `None` where it has none.
pub fn use_shares() -> Option<Shares> {
    try_use_context::<Shares>().or_else(|| crate::shell::services().and_then(Shares::of))
}

#[component]
pub fn ReceiveShareDialog(
    shares: Shares,
    #[props(default)] storage: Option<Storage>,
    on_close: EventHandler<()>,
    /// The install job the share arrives as.
    #[props(default)]
    on_received: Option<EventHandler<String>>,
    /// A collection received: its id, to open.
    #[props(default)]
    on_collection: Option<EventHandler<String>>,
) -> Element {
    let mut link = use_signal(String::new);
    let mut help = use_signal(|| false);
    let mut info = use_signal(|| None::<ShareInfo>);
    let mut looking = use_signal(|| false);
    let mut look_error = use_signal(|| None::<ErrorReport>);
    let mut choosing = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);

    let look = {
        let shares = shares.clone();
        move |_| {
            let text = link.peek().trim().to_owned();
            if text.is_empty() {
                return;
            }
            looking.set(true);
            look_error.set(None);
            info.set(None);
            let shares = shares.clone();
            spawn(async move {
                match change(async move { shares.inspect(&text).await }).await {
                    Ok(found) => info.set(Some(found)),
                    Err(report) => look_error.set(Some(report)),
                }
                looking.set(false);
            });
        }
    };
    let receive = {
        let shares = shares.clone();
        move |root: Option<String>| {
            let text = link.peek().trim().to_owned();
            let keep = *help.peek();
            let shares = shares.clone();
            busy.set(true);
            failure.set(None);
            spawn(async move {
                match change(async move { shares.receive(&text, root, keep).await }).await {
                    Ok(job) => {
                        if let Some(on_received) = on_received {
                            on_received.call(job.id);
                        }
                        on_close.call(());
                    }
                    Err(report) => {
                        failure.set(Some(report));
                        choosing.set(false);
                        busy.set(false);
                    }
                }
            });
        }
    };
    let add_list = {
        let shares = shares.clone();
        move |_| {
            let text = link.peek().trim().to_owned();
            let shares = shares.clone();
            busy.set(true);
            failure.set(None);
            spawn(async move {
                match change(async move { shares.receive_collection(&text).await }).await {
                    Ok(collection) => {
                        on_close.call(());
                        if let Some(on_collection) = on_collection {
                            on_collection.call(collection.id);
                        }
                    }
                    Err(report) => {
                        failure.set(Some(report));
                        busy.set(false);
                    }
                }
            });
        }
    };

    let shown = info();
    let list = shown
        .as_ref()
        .is_some_and(|info| matches!(info.contents, ShareContents::Collection { .. }));
    let close = move |_| {
        if !busy() {
            on_close.call(());
        }
    };
    if let (true, Some(storage), Some(found)) = (choosing(), storage.clone(), shown.clone()) {
        let mut receive = receive.clone();
        return rsx! {
            Dialog {
                title: t!("Install location"),
                on_close: move |_| choosing.set(false),
                class: "max-h-[90vh] max-w-lg overflow-auto",
                "data-cs": "receive-share",
                InstallRootPicker {
                    storage,
                    heading: false,
                    needs: Needs { estimated_bytes: found.size, ..Needs::default() },
                    on_cancel: move |_| choosing.set(false),
                    on_select: move |root: String| receive(Some(root)),
                }
            }
        };
    }
    let mut look_submit = look.clone();
    rsx! {
        Dialog {
            title: t!("Receive a shared game"),
            on_close: (!busy()).then_some(EventHandler::new(close)),
            class: "max-h-[90vh] max-w-lg overflow-auto",
            "data-cs": "receive-share",
            TextField {
                id: "share-link-input",
                label: t!("Share link"),
                value: link(),
                mono: true,
                description: t!("The caskade://receive link someone sent you."),
                on_input: move |value| {
                    link.set(value);
                    info.set(None);
                    look_error.set(None);
                },
                on_submit: move |_| look_submit(()),
                action: rsx! {
                    Button { id: "share-look", icon: SEARCH, disabled: link().trim().is_empty() || looking(), on_press: look,
                        {t!("Look")}
                    }
                },
            }
            if looking() {
                p { role: "status", class: "text-sm text-fg-muted", {t!("Asking whoever shared it…")} }
            }
            if let Some(report) = look_error() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
            if let Some(found) = shown.clone() {
                ShareInfoDetails { info: found }
            }
            if shown.is_some() && !list {
                Checkbox {
                    id: "share-help",
                    checked: help(),
                    on_change: move |on| help.set(on),
                    label: t!("Help others receive it after it's here"),
                }
                p { class: "text-xs text-fg-subtle",
                    {t!("While it arrives, this computer helps the share's other recipients. After, it keeps a second copy of the files to go on helping until the share stops.")}
                }
            }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
            div { class: "flex justify-end gap-2",
                Button { id: "share-cancel", disabled: busy(), on_press: close, {t!("Cancel")} }
                if list {
                    Button { id: "share-add-list", variant: ButtonVariant::Primary, disabled: busy(), on_press: add_list,
                        {t!("Add to my collections")}
                    }
                } else {
                    Button {
                        id: "share-receive",
                        variant: ButtonVariant::Primary,
                        "data-focus-default": shown.is_some().then_some("true"),
                        disabled: shown.is_none() || busy(),
                        on_press: {
                            let mut receive = receive.clone();
                            move |_| {
                                if storage.is_some() {
                                    choosing.set(true);
                                } else {
                                    receive(None);
                                }
                            }
                        },
                        {t!("Receive")}
                    }
                }
            }
        }
    }
}
