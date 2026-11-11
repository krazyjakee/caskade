//! Settings → Devices → Copies and shares (#247), as the webview's
//! `SharesList`: the games this device shared, with who received them,
//! Copy link, Stop sharing and Forget; and the shares received here that
//! arrive or help their other recipients, with the switch to keep helping.

use caskade_core::error::ErrorReport;
use caskade_ui_data::shares::{ReceivedShare, ShareRecord, ShareState, Shares};
use caskade_ui_i18n::{format, t};
use dioxus_native::prelude::*;

use crate::data::change;
use crate::ui::icons::{COPY, X};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, FailureNotice, NoticeVariant, Switch, SwitchSize,
};

use super::shares_common::{ShareRecipients, ShareStateLine};

/// Whether a share's link works now.
fn running(share: &ShareRecord) -> bool {
    matches!(
        share.state,
        ShareState::Preparing { .. } | ShareState::Ready
    )
}

/// The list; nothing when there's nothing to list.
#[component]
pub fn SharesList(shares: Shares) -> Element {
    let mut made = use_signal(Vec::<ShareRecord>::new);
    let mut received = use_signal(Vec::<ReceivedShare>::new);
    {
        let shares = shares.clone();
        use_hook(move || {
            let mut changes = shares.subscribe();
            spawn(async move {
                loop {
                    let read = shares.clone();
                    if let Ok((list, here)) = change(async move {
                        let list = read.list().await?;
                        Ok((list, read.received()))
                    })
                    .await
                    {
                        made.set(list);
                        received.set(here);
                    }
                    if changes.recv().await.is_err() {
                        break;
                    }
                }
            });
        });
    }
    if made().is_empty() && received().is_empty() {
        return rsx! {};
    }
    rsx! {
        ul { "data-part": "shares", class: "flex flex-col gap-2",
            for share in made() {
                ShareRow { key: "{share.id}", shares: shares.clone(), share }
            }
            for share in received() {
                ReceivedRow { key: "{share.job}", shares: shares.clone(), share }
            }
        }
    }
}

#[component]
fn ShareRow(shares: Shares, share: ShareRecord) -> Element {
    let mut copied = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let title = share.title.clone();
    let line = match share.size {
        Some(size) => t!(
            "{title} · {size}",
            title = title.clone(),
            size = format::size(Some(size))
        ),
        None => title.clone(),
    };
    let state = match &share.state {
        ShareState::Preparing { .. } => "preparing",
        ShareState::Ready => "ready",
        ShareState::Failed { .. } => "failed",
        ShareState::Stopped => "stopped",
        ShareState::Expired => "expired",
    };
    // Runs `work` for the row's share, showing what failed.
    let mut run = move |work: std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<bool, ErrorReport>> + Send>,
    >| {
        busy.set(true);
        failure.set(None);
        spawn(async move {
            match change(work).await {
                Ok(wrote) => copied.set(wrote),
                Err(report) => failure.set(Some(report)),
            }
            busy.set(false);
        });
    };
    let id = share.id.clone();
    rsx! {
        li {
            "data-cs": "share",
            "data-status": state,
            class: "flex flex-col gap-2 rounded-control border border-border p-3",
            span { class: "flex flex-wrap items-center gap-3 text-sm",
                span { class: "min-w-0 flex-1 font-medium", "{line}" }
                if running(&share) {
                    Button {
                        size: ButtonSize::Sm,
                        icon: COPY,
                        disabled: busy(),
                        on_press: {
                            let (shares, id) = (shares.clone(), id.clone());
                            move |_| {
                                let (shares, id) = (shares.clone(), id.clone());
                                run(Box::pin(async move {
                                    let link = shares.link(&id).await?;
                                    Ok(caskade_app::commands::clipboard::clipboard_write_text(link).is_ok())
                                }))
                            }
                        },
                        if copied() { {t!("Link copied")} } else { {t!("Copy link")} }
                    }
                    Button {
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Danger,
                        icon: X,
                        disabled: busy(),
                        on_press: {
                            let (shares, id) = (shares.clone(), id.clone());
                            move |_| {
                                let (shares, id) = (shares.clone(), id.clone());
                                run(Box::pin(async move { shares.stop(&id).await.map(|()| false) }))
                            }
                        },
                        {t!("Stop sharing")}
                    }
                } else {
                    Button {
                        size: ButtonSize::Sm,
                        disabled: busy(),
                        on_press: {
                            let (shares, id) = (shares.clone(), id.clone());
                            move |_| {
                                let (shares, id) = (shares.clone(), id.clone());
                                run(Box::pin(async move { shares.forget(&id).await.map(|()| false) }))
                            }
                        },
                        {t!("Forget")}
                    }
                }
            }
            ShareStateLine { share: share.clone() }
            if !share.recipients.is_empty() {
                ShareRecipients { share: share.clone() }
            }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
        }
    }
}

#[component]
fn ReceivedRow(shares: Shares, share: ReceivedShare) -> Element {
    let mut failure = use_signal(|| None::<ErrorReport>);
    // It can help after it's here only if that was chosen while it arrived.
    let can_change = share.receiving || share.serving;
    let status = if share.receiving {
        "receiving"
    } else if share.serving {
        "serving"
    } else {
        "done"
    };
    let note = if share.serving {
        t!(
            "Helping {helping} others receive it.",
            helping = share.helping
        )
    } else if share.receiving {
        t!("Arriving: follow it in Activity.")
    } else {
        t!("Here.")
    };
    let job = share.job.clone();
    rsx! {
        li {
            "data-cs": "received-share",
            "data-status": status,
            class: "flex flex-col gap-2 rounded-control border border-border p-3",
            span { class: "flex flex-wrap items-center gap-3 text-sm",
                span { class: "min-w-0 flex-1 font-medium",
                    {t!("{title} from {sharer}", title = share.title.clone(), sharer = share.sharer.clone())}
                }
                Switch {
                    size: SwitchSize::Sm,
                    checked: share.keep_helping,
                    disabled: !can_change,
                    on_change: move |on: bool| {
                        if let Err(report) = shares.keep_helping(&job, on) {
                            failure.set(Some(report));
                        }
                    },
                    label: t!("Help others receive it"),
                }
            }
            p { role: "status", class: "text-xs text-fg-muted", "{note}" }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
        }
    }
}
