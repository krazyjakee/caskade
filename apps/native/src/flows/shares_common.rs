//! Pieces the share flows share, as the webview's `components/shares`
//! `common.tsx` and `ShareInfoDetails.tsx`: who received a share and how far
//! each got, what a share is doing, and what a link shares.

use caskade_ui_data::shares::{
    GameHere, ShareContents, ShareInfo, ShareReach, ShareRecipient, ShareRecord, ShareState,
};
use caskade_ui_i18n::{format, t};
use dioxus_native::prelude::*;

/// Who opened a share's link, and how far each has got.
#[component]
pub fn ShareRecipients(share: ShareRecord) -> Element {
    if share.recipients.is_empty() {
        return rsx! {
            p { role: "status", class: "text-xs text-fg-subtle", {t!("Nobody has opened the link yet.")} }
        };
    }
    rsx! {
        ul { "data-part": "recipients", class: "flex flex-col gap-2",
            for recipient in share.recipients.iter().cloned() {
                Recipient { key: "{recipient.name}", recipient, size: share.size }
            }
        }
    }
}

#[component]
fn Recipient(recipient: ShareRecipient, size: Option<u64>) -> Element {
    let total = size.unwrap_or(0);
    let ratio = if recipient.finished {
        1.0
    } else if total > 0 {
        (recipient.done as f64 / total as f64).min(1.0)
    } else {
        0.0
    };
    let done = format::size(Some(recipient.done));
    let all = format::size(Some(total));
    let note = if recipient.finished {
        t!("Received")
    } else if recipient.connected {
        if total > 0 {
            t!("{done} of {all}", done = done, all = all)
        } else {
            done
        }
    } else {
        t!("Waiting for them to come back")
    };
    let status = if recipient.finished {
        "done"
    } else if recipient.connected {
        "receiving"
    } else {
        "away"
    };
    let name = recipient.name.clone();
    rsx! {
        li { "data-cs": "share-recipient", "data-status": status, class: "flex flex-col gap-1.5",
            span { class: "flex items-center gap-3 text-xs",
                span { class: "flex-1 font-medium", "{name}" }
                if recipient.serving {
                    span { class: "text-fg-subtle", {t!("Helping others receive it")} }
                }
            }
            div {
                role: "progressbar",
                "aria-label": t!("How far {name} got", name = name.clone()),
                "aria-valuenow": "{(ratio * 100.0).round()}",
                "aria-valuemin": "0",
                "aria-valuemax": "100",
                class: "flex flex-col gap-1",
                div { class: "h-1.5 overflow-hidden rounded-full bg-surface-overlay",
                    div {
                        class: if recipient.finished { "h-full rounded-full bg-status-installed" } else { "h-full rounded-full bg-accent-alt" },
                        style: "width: {ratio * 100.0}%",
                    }
                }
                span { class: "text-xs text-fg-muted", "{note}" }
            }
        }
    }
}

/// What a share does now: reading its files, its link working until when,
/// or stopped.
#[component]
pub fn ShareStateLine(share: ShareRecord) -> Element {
    let until = share.expires_at_ms.map(crate::library::date_time);
    let (status, text) = match &share.state {
        ShareState::Preparing { done, total } => (
            "preparing",
            if *total > 0 {
                t!(
                    "Getting the files ready: {done} of {total}. The link works already.",
                    done = format::size(Some(*done)),
                    total = format::size(Some(*total))
                )
            } else {
                t!("Getting the files ready. The link works already.")
            },
        ),
        ShareState::Ready => (
            "ready",
            match &until {
                Some(until) => t!("The link works until {until}.", until = until.clone()),
                None => t!("The link works until you stop sharing."),
            },
        ),
        ShareState::Failed { reason } => (
            "failed",
            t!(
                "The game's files couldn't be read: {reason}",
                reason = reason.clone()
            ),
        ),
        ShareState::Stopped => ("stopped", t!("Stopped: the link doesn't work any more.")),
        ShareState::Expired => ("expired", t!("Expired: the link doesn't work any more.")),
    };
    rsx! {
        p {
            "data-part": "state",
            "data-status": status,
            role: "status",
            class: if status == "failed" { "text-xs text-danger" } else { "text-xs text-fg-muted" },
            "{text}"
        }
    }
}

/// What a share link shares, as its sharer says: who, what, how big, for
/// which system, until when, and whether it comes directly; for something
/// smaller than a game, where it goes here; for a collection, which of its
/// games this device has.
#[component]
pub fn ShareInfoDetails(info: ShareInfo) -> Element {
    let sharer = info.sharer.clone();
    let until = info.expires_at_ms.map(crate::library::date_time);
    let collection = matches!(info.contents, ShareContents::Collection { .. });
    let mut rows: Vec<(String, String)> =
        vec![(t!(note = "Who shared something", "From"), sharer.clone())];
    if let (Some(size), false) = (info.size, collection) {
        rows.push((t!("Size"), format::size(Some(size))));
    }
    if info.windows {
        rows.push((
            t!(note = "Which system a shared game was built for", "Build"),
            t!("Windows (runs through Proton on Linux)"),
        ));
    }
    match &info.contents {
        ShareContents::Web { url, .. } => rows.push((
            t!(note = "A shared web game's address", "Address"),
            url.clone(),
        )),
        ShareContents::Rom { platform, .. } => rows.push((
            t!(note = "A shared ROM's platform", "Platform"),
            platform.clone(),
        )),
        _ => {}
    }
    rows.push((
        t!(note = "How long a share link works", "Link"),
        match &until {
            Some(until) => t!("Works until {until}", until = until.clone()),
            None => t!("Works until it's stopped"),
        },
    ));
    let lead = match &info.contents {
        ShareContents::Save { game, .. } => t!(
            "{sharer} shares a save of {game} with you.",
            sharer = sharer.clone(),
            game = game.clone()
        ),
        ShareContents::Collection { name, .. } => t!(
            "{sharer} shares the collection {name} with you.",
            sharer = sharer.clone(),
            name = name.clone()
        ),
        _ => t!(
            "{sharer} shares {title} with you.",
            sharer = sharer.clone(),
            title = info.title.clone()
        ),
    };
    let reach = match info.reach {
        ShareReach::Direct => "direct",
        ShareReach::RelayOnly => "relayOnly",
    };
    rsx! {
        div { "data-cs": "share-info", "data-status": reach, class: "space-y-3",
            p { class: "text-sm", "{lead}" }
            dl { class: "grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 rounded-control border border-border p-3 text-xs",
                for (label, value) in rows {
                    div { key: "{label}", class: "contents",
                        dt { class: "text-fg-subtle", "{label}" }
                        dd { class: "break-all", "{value}" }
                    }
                }
            }
            Where { contents: info.contents.clone() }
            if info.reach == ShareReach::RelayOnly && !collection {
                p { role: "status", class: "text-xs text-warning",
                    {t!("Only a relay reaches {sharer}'s computer right now. The game waits for a direct connection: game files don't go through relays.", sharer = sharer.clone())}
                }
            }
        }
    }
}

/// Where what's shared goes here.
#[component]
fn Where(contents: ShareContents) -> Element {
    let text = match &contents {
        ShareContents::Game => return rsx! {},
        ShareContents::Save { game, here, .. } => match here {
            Some(_) => t!(
                "It arrives as a new version of your saves of {game}. Your own saves stay: the next Play asks which to use.",
                game = game.clone()
            ),
            None => t!(
                "{game} isn't in your library: Caskade adds it, with this save as its first version.",
                game = game.clone()
            ),
        },
        ShareContents::Web { here, .. } => match here {
            Some(_) => t!("It becomes the offline copy of the web game you have at this address."),
            None => t!("Caskade adds it to your web games, ready to play offline."),
        },
        ShareContents::Rom {
            emulator, ready, ..
        } => match (ready, emulator) {
            (true, _) => t!("This device has an emulator for it."),
            (false, Some(emulator)) => {
                t!(
                    "Caskade sets up {emulator} to play it.",
                    emulator = emulator.clone()
                )
            }
            (false, None) => t!("Caskade sets up an emulator to play it."),
        },
        ShareContents::Collection { games, .. } => {
            return rsx! {
                ul { "data-part": "games", class: "max-h-48 space-y-1 overflow-auto text-xs",
                    for game in games.iter().cloned() {
                        li { key: "{game.title}:{game.igdb_id.clone().unwrap_or_default()}", class: "flex gap-3",
                            span { class: "min-w-0 flex-1 truncate", "{game.title}" }
                            span {
                                "data-status": match game.here {
                                    GameHere::Installed => "installed",
                                    GameHere::Owned => "owned",
                                    GameHere::InLibrary => "inLibrary",
                                    GameHere::Missing => "missing",
                                },
                                class: "text-fg-subtle",
                                match game.here {
                                    GameHere::Installed => t!(note = "A game of a shared collection, on this device", "Installed"),
                                    GameHere::Owned => t!(note = "A game of a shared collection a store account owns", "Owned"),
                                    GameHere::InLibrary => t!(note = "A game of a shared collection", "In your library"),
                                    GameHere::Missing => t!(note = "A game of a shared collection", "Not in your library"),
                                }
                            }
                        }
                    }
                }
            };
        }
    };
    rsx! {
        p { "data-part": "where", class: "text-xs text-fg-muted", "{text}" }
    }
}
