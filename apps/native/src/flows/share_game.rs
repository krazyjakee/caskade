//! Share with someone… (#247), as the webview's `ShareGameDialog`: makes a
//! link to one installed game (or something smaller, #248). Whoever opens
//! it in their Caskade gets it straight from this computer (and from others
//! it reached first), never through a relay. Stopping the share makes the
//! link fail at once; copies already received stay theirs. Console mode
//! shows the link as a QR code, for a phone to pass on.

use caskade_core::error::ErrorReport;
use caskade_ui_data::shares::Shares;
use caskade_ui_data::shares::{ShareExpiry, ShareItem, ShareRecord};
use caskade_ui_i18n::{format, t, tp};
use dioxus_native::prelude::*;

use crate::data::change;
use crate::ui::icons::COPY;
use crate::ui::{
    Button, ButtonVariant, Dialog, FailureNotice, NoticeVariant, Segment, SegmentedControl,
};

use super::shares_common::{ShareRecipients, ShareStateLine};

/// What Share with someone… shares, and what it's called.
#[derive(Clone, Debug, PartialEq)]
pub enum ShareTarget {
    /// A game's release.
    Game { entry_id: String, title: String },
    /// Something smaller (#248): a save, a web game, a ROM, a collection.
    Item { item: ShareItem, title: String },
}

impl ShareTarget {
    fn title(&self) -> &str {
        match self {
            Self::Game { title, .. } | Self::Item { title, .. } => title,
        }
    }
}

#[component]
pub fn ShareGameDialog(
    shares: Shares,
    target: ShareTarget,
    on_close: EventHandler<()>,
    /// Console mode: the link also as a QR code.
    #[props(default)]
    console: bool,
) -> Element {
    let title = target.title().to_owned();
    let mut expiry = use_signal(|| "week".to_owned());
    let mut copied = use_signal(|| false);
    let mut made = use_signal(|| None::<(ShareRecord, String)>);
    let mut making = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let mut stopping = use_signal(|| false);
    let mut qr = use_signal(|| None::<String>);

    // The share as it is now: its files read, who received it.
    {
        let shares = shares.clone();
        use_hook(move || {
            let mut changes = shares.subscribe();
            spawn(async move {
                while changes.recv().await.is_ok() {
                    let Some((share, link)) = made.peek().clone() else {
                        continue;
                    };
                    let shares = shares.clone();
                    if let Ok(list) = change(async move { shares.list().await }).await
                        && let Some(now) = list.into_iter().find(|s| s.id == share.id)
                    {
                        made.set(Some((now, link)));
                    }
                }
            });
        });
    }
    let make = {
        let shares = shares.clone();
        let target = target.clone();
        move |_| {
            let shares = shares.clone();
            let target = target.clone();
            let until = match expiry.peek().as_str() {
                "day" => ShareExpiry::Day,
                "never" => ShareExpiry::Never,
                _ => ShareExpiry::Week,
            };
            making.set(true);
            failure.set(None);
            spawn(async move {
                let qr_shares = shares.clone();
                let created = change(async move {
                    match target {
                        ShareTarget::Game { entry_id, .. } => shares.share(&entry_id, until).await,
                        ShareTarget::Item { item, .. } => shares.share_item(item, until).await,
                    }
                })
                .await;
                making.set(false);
                match created {
                    Ok(created) => {
                        if console {
                            let id = created.share.id.clone();
                            if let Ok(svg) = change(async move { qr_shares.qr(&id).await }).await {
                                qr.set(Some(svg));
                            }
                        }
                        made.set(Some((created.share, created.link)));
                    }
                    Err(report) => failure.set(Some(report)),
                }
            });
        }
    };
    let close = move |_| on_close.call(());
    let Some((share, link)) = made() else {
        let lead = match &target {
            ShareTarget::Game { .. } => t!(
                "Caskade makes a link. Whoever opens it in their Caskade gets the game straight from this computer, on this network or over the internet."
            ),
            ShareTarget::Item {
                item: ShareItem::Collection { .. },
                ..
            } => t!(
                "Caskade makes a link. Whoever opens it in their Caskade sees the collection's games and which of them they have. Nothing is downloaded."
            ),
            ShareTarget::Item { .. } => t!(
                "Caskade makes a link. Whoever opens it in their Caskade gets it straight from this computer, on this network or over the internet."
            ),
        };
        return rsx! {
            Dialog {
                title: t!("Share {title}", title = title.clone()),
                on_close: close,
                class: "max-h-[90vh] max-w-lg overflow-auto",
                "data-cs": "share-game",
                p { class: "text-sm text-fg-muted", "{lead}" }
                div { class: "flex flex-col gap-2",
                    span { id: "share-expiry", class: "text-sm font-medium", {t!("The link works")} }
                    SegmentedControl {
                        "aria-labelledby": "share-expiry",
                        options: vec![
                            Segment::new("day", t!("For a day")),
                            Segment::new("week", t!("For a week")),
                            Segment::new("never", t!("Until I stop sharing")),
                        ],
                        value: expiry(),
                        on_change: move |id| expiry.set(id),
                    }
                }
                if let Some(report) = failure() {
                    FailureNotice { error: report, variant: NoticeVariant::Inline }
                }
                div { class: "flex justify-end gap-2",
                    Button { id: "share-cancel", on_press: close, {t!("Cancel")} }
                    Button {
                        id: "share-make",
                        variant: ButtonVariant::Primary,
                        "data-focus-default": "true",
                        disabled: making(),
                        on_press: make,
                        {t!("Make a link")}
                    }
                }
            }
        };
    };
    let size = share.size.map(|size| format::size(Some(size)));
    let send = match &size {
        Some(size) => t!(
            "Send this link to whoever you're sharing {title} ({size}) with. They open it, or paste it in Add game → Receive a shared game. Keep Caskade open until it arrives.",
            title = title.clone(),
            size = size.clone()
        ),
        None => t!(
            "Send this link to whoever you're sharing {title} with. They open it, or paste it in Add game → Receive a shared game. Keep Caskade open until it arrives.",
            title = title.clone()
        ),
    };
    let stop = {
        let shares = shares.clone();
        let id = share.id.clone();
        move |_| {
            let shares = shares.clone();
            let id = id.clone();
            stopping.set(true);
            failure.set(None);
            spawn(async move {
                match change(async move { shares.stop(&id).await }).await {
                    Ok(()) => on_close.call(()),
                    Err(report) => {
                        failure.set(Some(report));
                        stopping.set(false);
                    }
                }
            });
        }
    };
    let copy_link = link.clone();
    rsx! {
        Dialog {
            title: t!("Share {title}", title = title.clone()),
            on_close: close,
            class: "max-h-[90vh] max-w-lg overflow-auto",
            "data-cs": "share-game",
            p { class: "text-sm text-fg-muted", "{send}" }
            if let Some(svg) = qr() {
                img {
                    "data-part": "qr",
                    src: "data:image/svg+xml;utf8,{percent_encoding::utf8_percent_encode(&svg, percent_encoding::NON_ALPHANUMERIC)}",
                    alt: t!("The link as a QR code"),
                    class: "mx-auto size-56 rounded-control bg-white p-2",
                }
            }
            div { class: "flex gap-2",
                p { "data-part": "link", title: "{link}", class: "min-w-0 flex-1 truncate rounded-control border border-border p-2 font-mono text-xs select-all", "{link}" }
                Button {
                    id: "share-copy",
                    icon: COPY,
                    "data-focus-default": "true",
                    on_press: move |_| copied.set(crate::paste::copy_text(copy_link.clone()).is_ok()),
                    if copied() { {t!("Copied")} } else { {tp!("button", "Copy")} }
                }
            }
            ShareStateLine { share: share.clone() }
            ShareRecipients { share: share.clone() }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
            div { class: "flex justify-between gap-2",
                Button { id: "share-stop", variant: ButtonVariant::Danger, disabled: stopping(), on_press: stop, {t!("Stop sharing")} }
                Button { id: "share-done", variant: ButtonVariant::Primary, on_press: close, {t!("Done")} }
            }
        }
    }
}
