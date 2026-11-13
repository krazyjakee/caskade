//! Games copying between devices, and shares (the webview's
//! `CopiesAndShares`).

use super::*;

/// How often copies under way are read again.
pub(super) const TRANSFERS_EVERY: Duration = Duration::from_secs(2);

/// Games moving between this user's devices (#233), and games shared with
/// other people and received from them (#246, #247). With nothing under
/// way or shared, the section stays out of sight.
#[component]
pub(super) fn CopiesAndShares(devices: DeviceSync) -> Element {
    let mut transfers = use_signal(Vec::<TransferView>::new);
    let shares = crate::flows::receive_share::use_shares();
    let has_shares = {
        let shares = shares.clone();
        use_resource(move || {
            let shares = shares.clone();
            async move {
                match shares {
                    Some(shares) => change(async move { shares.has_any().await })
                        .await
                        .unwrap_or(false),
                    None => false,
                }
            }
        })
    };
    {
        let devices = devices.clone();
        use_hook(move || {
            spawn(async move {
                loop {
                    let read = devices.clone();
                    // Nothing to show while the window is hidden (#111).
                    if crate::shell::main_window_shown()
                        && let Ok(list) = change(async move { read.transfers().await }).await
                        && *transfers.peek() != list
                    {
                        transfers.set(list);
                    }
                    // Progress moves on its own: read again while copies go on.
                    let every = if transfers.peek().is_empty() {
                        TRANSFERS_EVERY * 5
                    } else {
                        TRANSFERS_EVERY
                    };
                    sleep(every).await;
                }
            });
        });
    }
    let list = transfers();
    let shared = has_shares.read().unwrap_or(false);
    if list.is_empty() && !shared {
        return rsx! {};
    }
    rsx! {
        SettingsSection {
            icon: SHARE_2,
            title: t!("Copies and shares"),
            description: t!("Games copied between your devices, and games you shared. Both computers need Caskade open until a game arrives."),
            if !list.is_empty() {
                ul { class: "flex flex-col gap-2",
                    for transfer in list {
                        TransferRow { key: "{transfer.id}", transfer }
                    }
                }
            }
            if let Some(shares) = shares {
                crate::flows::shares_list::SharesList { shares }
            }
        }
    }
}

#[component]
pub(super) fn TransferRow(transfer: TransferView) -> Element {
    let incoming = transfer.direction == TransferDirection::Incoming;
    let text = if incoming {
        t!(
            "{title} from {device}",
            title = transfer.title.clone(),
            device = transfer.device_name.clone()
        )
    } else {
        t!(
            "{title} to {device}",
            title = transfer.title.clone(),
            device = transfer.device_name.clone()
        )
    };
    let waiting = transfer.state == TransferState::WaitingDirect;
    let ratio = if transfer.total_bytes > 0 {
        transfer.done_bytes as f64 / transfer.total_bytes as f64
    } else {
        0.0
    };
    let note = if waiting {
        t!("Waiting for a direct connection. Game files only travel directly between devices.")
    } else {
        t!(
            "{done} of {total} · {speed}/s",
            done = format::size(Some(transfer.done_bytes)),
            total = format::size(Some(transfer.total_bytes)),
            speed = format::size(Some(transfer.bytes_per_second))
        )
    };
    let state = match transfer.state {
        TransferState::Running => "running",
        TransferState::WaitingDirect => "waitingDirect",
    };
    rsx! {
        li { "data-cs": "transfer", "data-status": state, class: "flex flex-col gap-2 rounded-control border border-border p-3",
            span { class: "flex flex-wrap items-center gap-3 text-sm",
                Svg { icon: if incoming { ARROW_DOWN_TO_LINE } else { ARROW_UP_FROM_LINE }, class: "size-4 text-fg-muted" }
                span { class: "min-w-0 flex-1 font-medium", "{text}" }
                LinkBadge { link: transfer.link }
            }
            Progress {
                value: (!waiting).then_some(ratio),
                "aria-label": t!("Transfer progress"),
                value_text: note.clone(),
            }
            span { class: "text-xs text-fg-muted", "{note}" }
        }
    }
}
