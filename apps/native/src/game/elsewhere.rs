//! What the game page says about the game elsewhere (#100): the stores
//! that own it and installing it through their launchers (the webview's
//! `OwnedReleases.tsx`), its copies on the user's other devices with Copy
//! to this computer and Play there (`OtherDevices.tsx`), where its play
//! time comes from (`PlaytimeSources.tsx`), and where it's running now
//! (`PlayingElsewhere.tsx`'s `PlayingElsewhereLine`).

use super::data::{use_answer, use_page_data};
use super::system_name;
use crate::flows::devices::peer_note;
use crate::ui::{Button, ButtonSize, ButtonVariant, icons};
use caskade_core::devices::DeviceLink;
use caskade_core::store_accounts::OwnedInstallState;
use caskade_core::sync::copies::PeerCopy;
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{format, t, tp};
use dioxus_native::prelude::*;

/// What to say about an owned release, beside its store.
fn describe(state: &OwnedInstallState) -> Option<String> {
    use caskade_core::store_accounts::UnavailableReason as Unavailable;
    Some(match state {
        OwnedInstallState::Installed => t!("Installed."),
        OwnedInstallState::Available { .. } => return None,
        OwnedInstallState::Installing { launcher, .. } => t!(
            "Waiting for {launcher} to finish installing it.",
            launcher = launcher.clone()
        ),
        OwnedInstallState::LauncherMissing { launcher } => t!(
            "{launcher} isn't installed on this computer.",
            launcher = launcher.clone()
        ),
        OwnedInstallState::UnsupportedSystem { systems } => t!(
            "Runs on {systems}, not on this computer's system.",
            systems = systems
                .iter()
                .map(|system| system_name(system))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        OwnedInstallState::Unavailable { reason } => match reason {
            Unavailable::NoLongerOwned => t!("The account no longer owns it."),
            Unavailable::Disconnected => {
                t!("The account is disconnected. Connect it again in Settings → Store accounts.")
            }
            Unavailable::NoProvider => t!("This store isn't available in this version of Caskade."),
        },
        OwnedInstallState::NotSupported => t!("The store can't install games from Caskade."),
    })
}

fn state_name(state: &OwnedInstallState) -> &'static str {
    match state {
        OwnedInstallState::Installed => "installed",
        OwnedInstallState::Available { .. } => "available",
        OwnedInstallState::Installing { .. } => "installing",
        OwnedInstallState::LauncherMissing { .. } => "launcherMissing",
        OwnedInstallState::UnsupportedSystem { .. } => "unsupportedSystem",
        OwnedInstallState::Unavailable { .. } => "unavailable",
        OwnedInstallState::NotSupported => "notSupported",
    }
}

/// A game's owned releases: which store account owns it, and installing
/// it through that store's launcher. Installing is done when Library
/// Discovery finds the copy; until then the release says it waits.
#[component]
pub fn OwnedReleases(game_id: String, primary: bool) -> Element {
    let data = use_page_data();
    let owned = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(DataKey::Game(game_id.clone()), move || {
            (data.owned)(game_id.clone())
        })
    };
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let list = owned
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok().cloned())
        .unwrap_or_default();
    if list.is_empty() {
        return rsx! {};
    }
    let act = move |(entry_id, install): (String, bool)| {
        let data = data.clone();
        pending.set(true);
        error.set(None);
        spawn(async move {
            let result = if install {
                (data.owned_install)(entry_id).await
            } else {
                (data.owned_cancel)(entry_id).await
            };
            if let Err(report) = result {
                error.set(Some(report.summary));
            }
            pending.set(false);
        });
    };
    rsx! {
        div { "data-part": "owned", class: "flex flex-col gap-2",
            for (index, release) in list.into_iter().enumerate() {
                div {
                    key: "{release.entry_id}",
                    "data-part": "owned-release",
                    "data-status": state_name(&release.state),
                    class: "flex flex-col gap-1.5 rounded-control border border-border p-3",
                    span { class: "text-sm",
                        {t!("Owned on {provider} ({account})", provider = release.provider_name.clone(), account = release.account_name.clone())}
                    }
                    if let Some(at) = release.refreshed_at_ms {
                        span { class: "text-xs text-fg-subtle",
                            {t!("Library refreshed {refreshed}.", refreshed = crate::library::date_time(at))}
                        }
                    }
                    if let Some(note) = describe(&release.state) {
                        span { class: "text-xs text-fg-muted", "{note}" }
                    }
                    // The launcher's download is its own (#131).
                    if let OwnedInstallState::Installing { launcher, .. } = &release.state {
                        span { "data-part": "pause-elsewhere", class: "text-xs text-fg-muted",
                            {t!(
                                note = "A store's launcher (Steam, GOG Galaxy, the Epic Games Launcher) is installing a game",
                                "To pause or cancel it, use {launcher}: Caskade can't pause a store's own download.",
                                launcher = launcher.clone()
                            )}
                        }
                    }
                    if let OwnedInstallState::Available { launcher } = &release.state {
                        Button {
                            "data-part": "install",
                            variant: if primary && index == 0 { ButtonVariant::Primary } else { ButtonVariant::Secondary },
                            icon: Some(icons::DOWNLOAD),
                            disabled: pending(),
                            on_press: {
                                let mut act = act.clone();
                                let id = release.entry_id.clone();
                                move |_| act((id.clone(), true))
                            },
                            {t!("Install with {launcher}", launcher = launcher.clone())}
                        }
                    }
                    if matches!(release.state, OwnedInstallState::Installing { .. }) {
                        Button {
                            "data-part": "stop-waiting",
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Ghost,
                            on_press: {
                                let mut act = act.clone();
                                let id = release.entry_id.clone();
                                move |_| act((id.clone(), false))
                            },
                            {t!("Stop waiting")}
                        }
                    }
                }
            }
            if let Some(error) = error() {
                p { role: "alert", class: "text-xs text-danger", "{error}" }
            }
        }
    }
}

/// The game's copies on the user's other devices: Copy to this computer
/// (over a direct connection only), and Play there (waking a device that
/// can be woken).
#[component]
pub fn OtherDevices(game_id: String, primary: bool) -> Element {
    let data = use_page_data();
    let copies = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(DataKey::Devices, move || (data.copies)(game_id.clone()))
    };
    let mut copying = use_signal(|| None::<String>);
    let mut copied = use_signal(|| false);
    let mut playing = use_signal(|| None::<String>);
    let mut played = use_signal(|| None::<String>);
    let mut error = use_signal(|| None::<String>);
    let list: Vec<PeerCopy> = copies
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok().cloned())
        .unwrap_or_default();
    if list.is_empty() {
        return rsx! {};
    }
    let best = list
        .iter()
        .find(|peer| peer.blocked.is_none() && peer.link != DeviceLink::Offline)
        .map(|peer| peer.device_id.clone());
    let copy = {
        let (data, game_id) = (data.clone(), game_id.clone());
        move |device: String| {
            let (data, game_id) = (data.clone(), game_id.clone());
            copying.set(Some(device.clone()));
            error.set(None);
            spawn(async move {
                match (data.copy_here)(game_id, device).await {
                    Ok(()) => copied.set(true),
                    Err(report) => {
                        copying.set(None);
                        error.set(Some(report.summary));
                    }
                }
            });
        }
    };
    let play = move |device: String| {
        let (data, game_id) = (data.clone(), game_id.clone());
        playing.set(Some(device.clone()));
        played.set(None);
        error.set(None);
        spawn(async move {
            match (data.play_on)(game_id, device.clone()).await {
                Ok(()) => played.set(Some(device)),
                Err(report) => error.set(Some(report.summary)),
            }
            playing.set(None);
        });
    };
    rsx! {
        section { "data-part": "other-devices", "aria-labelledby": "other-devices-heading", class: "flex flex-col gap-2",
            h3 { id: "other-devices-heading", class: "text-sm font-semibold", {t!("On your other devices")} }
            for peer in list {
                PeerRow {
                    key: "{peer.device_id}",
                    primary: primary && best.as_ref() == Some(&peer.device_id),
                    copying: copying().is_some(),
                    playing: playing().as_ref() == Some(&peer.device_id),
                    played: played().as_ref() == Some(&peer.device_id),
                    on_copy: {
                        let mut copy = copy.clone();
                        move |device: String| copy(device)
                    },
                    on_play: {
                        let mut play = play.clone();
                        move |device: String| play(device)
                    },
                    peer,
                }
            }
            if copied() {
                p { role: "status", class: "text-xs text-status-installed", {t!("Copying to this computer. Follow it in Activity.")} }
            }
            if let Some(error) = error() {
                p { role: "alert", class: "text-xs text-danger", "{error}" }
            }
        }
    }
}

/// A link's name, as `data-status` has it.
fn link_name(link: DeviceLink) -> &'static str {
    match link {
        DeviceLink::Lan => "lan",
        DeviceLink::Direct => "direct",
        DeviceLink::Relay => "relay",
        DeviceLink::Offline => "offline",
    }
}

#[component]
fn PeerRow(
    peer: PeerCopy,
    primary: bool,
    copying: bool,
    playing: bool,
    played: bool,
    on_copy: EventHandler<String>,
    on_play: EventHandler<String>,
) -> Element {
    let note = peer_note(&peer);
    let can_copy = peer.blocked.is_none() && peer.link != DeviceLink::Offline;
    let asleep = peer.link == DeviceLink::Offline;
    let can_play = !peer.playing && (!asleep || peer.wakeable);
    let device = peer.device_name.clone();
    let details: Vec<String> = [peer.size_bytes.map(|size| format::size(Some(size))), note]
        .into_iter()
        .flatten()
        .collect();
    let details = details.join(" · ");
    let id = peer.device_id.clone();
    let play_id = peer.device_id.clone();
    rsx! {
        div {
            "data-part": "peer-copy",
            "data-status": link_name(peer.link),
            class: "flex flex-col gap-1 rounded-control border border-border p-3",
            span { class: "flex items-center gap-2 text-sm",
                span { class: "flex-1", {t!("On {device}", device = device.clone())} }
            }
            if !details.is_empty() {
                span { class: "text-xs text-fg-muted", "{details}" }
            }
            if playing {
                span { role: "status", class: "text-xs text-fg-muted",
                    if asleep { {t!("Waking {device}…", device = device.clone())} } else { {t!("Starting on {device}…", device = device.clone())} }
                }
            }
            if played {
                span { role: "status", class: "text-xs text-status-installed", {t!("Started on {device}.", device = device.clone())} }
            }
            if can_copy || can_play {
                div { class: "flex flex-wrap gap-2",
                    if can_copy {
                        Button {
                            "data-part": "copy-here",
                            size: ButtonSize::Sm,
                            variant: if primary { ButtonVariant::Primary } else { ButtonVariant::Secondary },
                            icon: Some(icons::DOWNLOAD),
                            disabled: copying,
                            on_press: move |_| on_copy.call(id.clone()),
                            {t!("Copy to this computer")}
                        }
                    }
                    if can_play {
                        Button {
                            "data-part": "play-there",
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Ghost,
                            icon: Some(if asleep { icons::POWER } else { icons::PLAY }),
                            disabled: playing,
                            on_press: move |_| on_play.call(play_id.clone()),
                            if asleep { {t!("Wake and play on {device}", device = device.clone())} } else { {t!("Play on {device}", device = device.clone())} }
                        }
                    }
                }
            }
        }
    }
}

const PROVIDER_NAMES: &[(&str, &str)] = &[
    ("steam", "Steam"),
    ("gog", "GOG"),
    ("epic", "Epic Games"),
    ("lutris", "Lutris"),
    ("heroic", "Heroic"),
    ("legendary", "Legendary"),
];

fn provider_name(provider: &str) -> String {
    PROVIDER_NAMES
        .iter()
        .find(|(id, _)| *id == provider)
        .map(|(_, name)| (*name).to_owned())
        .unwrap_or_else(|| provider.to_owned())
}

/// Where a game's play time comes from: Caskade's own sessions and what
/// stores or launchers reported beyond them.
#[component]
pub fn PlaytimeSources(game_id: String) -> Element {
    let data = use_page_data();
    let breakdown = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(DataKey::Game(game_id.clone()), move || {
            (data.playtime)(game_id.clone())
        })
    };
    let Some(Ok(data)) = &*breakdown.read() else {
        return rsx! {};
    };
    if data.sources.is_empty() || data.reported_ms == 0 {
        return rsx! {};
    }
    let tracked = format::playtime(data.tracked_ms / 1000);
    let reported = format::playtime(data.reported_ms / 1000);
    let mut names: Vec<String> = Vec::new();
    for source in &data.sources {
        let provider = provider_name(&source.provider);
        let name = match &source.account_name {
            Some(account) if !account.is_empty() => format!("{provider} ({account})"),
            _ => provider,
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let details = data
        .sources
        .iter()
        .map(|source| {
            let total = source
                .total_ms
                .map(|ms| format::playtime(ms / 1000))
                .unwrap_or_else(|| t!("unknown"));
            t!(
                "{provider}: {total}, read {read}",
                provider = provider_name(&source.provider),
                total = total,
                read = crate::library::date_time(source.observed_at_ms)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    rsx! {
        span { "data-part": "playtime-sources", title: "{details}", class: "text-xs text-fg-subtle",
            {t!("{tracked} in Caskade, {reported} more reported by {names}", tracked = tracked, reported = reported, names = names.join(", "))}
        }
    }
}

/// Where the game is running now.
#[component]
pub fn PlayingElsewhereLine(game_id: String) -> Element {
    let steps = super::launch::use_launch_steps();
    let playing = use_answer(DataKey::Devices, move || (steps.playing_elsewhere)());
    let devices: Vec<String> = playing
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok())
        .map(|list| {
            list.iter()
                .filter(|each| each.game_id == game_id)
                .map(|each| {
                    if each.device_name.is_empty() {
                        tp!("playing on", "another device")
                    } else {
                        each.device_name.clone()
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    if devices.is_empty() {
        return rsx! {};
    }
    rsx! {
        p { "data-part": "playing-elsewhere", role: "status", class: "mt-2 text-sm text-accent-alt",
            {t!("Playing on {devices}", devices = devices.join(", "))}
        }
    }
}

/// Which games are running on another device, read once for every card in
/// console mode's shell.
#[derive(Clone, Copy)]
pub struct PlayingMarks(crate::data::Query<Vec<caskade_app::commands::presence::PlayingElsewhere>>);

/// Gives the cards below what's being played elsewhere (console mode).
pub fn provide_playing_marks() {
    let steps = super::launch::use_launch_steps();
    let playing = use_answer(DataKey::Devices, move || (steps.playing_elsewhere)());
    use_context_provider(|| PlayingMarks(playing));
}

/// A game's card in console mode: the device it's running on.
#[component]
pub fn PlayingElsewhereMark(game_id: String) -> Element {
    let Some(PlayingMarks(playing)) = try_use_context::<PlayingMarks>() else {
        return rsx! {};
    };
    let device = playing
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok())
        .and_then(|list| list.iter().find(|each| each.game_id == game_id))
        .map(|each| {
            if each.device_name.is_empty() {
                tp!("playing on", "another device")
            } else {
                each.device_name.clone()
            }
        });
    let Some(device) = device else {
        return rsx! {};
    };
    rsx! {
        span {
            "data-part": "playing-elsewhere",
            class: "absolute right-2 bottom-16 left-2 truncate rounded-control bg-surface-base/90 px-2 py-1 text-[11px] text-accent-alt",
            {t!("Playing on {device}", device = device)}
        }
    }
}
