//! Set up from another device (#242, #276), as the webview's
//! `onboarding/DeviceStep.tsx` and `GamesStep.tsx`: pair with a Caskade on
//! this network (or by a pairing link), then bring its library over in the
//! first sync: games, accounts with their PINs, collections, play history
//! and the settings that sync. This device's own folders, emulators and
//! hardware settings are asked for next, as on any first start. Nothing
//! here needs the internet. After the folder question, the games that
//! device has installed and that run here can be copied over the local
//! network: nothing copies until chosen, each chosen game becomes a copy
//! job in Activity (#233), and setup goes on while they arrive.

use caskade_core::error::ErrorReport;
use caskade_core::storage::RootSummary;
use caskade_core::sync::offers::DeviceGame;
use caskade_ui_data::sync::{DeviceKind, DeviceSync, DevicesStatus, IdentityStatus, PairedDevice};
use caskade_ui_i18n::{format, t, tn};
use dioxus_native::prelude::*;

use super::{Continue, StepHeading};
use crate::data::change;
use crate::settings::use_device_sync;
use crate::ui::icons::{CIRCLE_CHECK, GAMEPAD_2, HARD_DRIVE, LAPTOP, LINK_2, MONITOR, TV};
use crate::ui::{Button, ButtonVariant, Checkbox, FailureNotice, NoticeVariant, Svg, TextField};

/// What a pairing link looks like, as the field's hint.
const LINK_PLACEHOLDER: &str = "caskade://pair?…";

/// The devices' status, read again on every change to them; `None` until
/// it's read (or when it can't be), as the webview's query data.
fn use_devices_status(devices: Option<DeviceSync>) -> Signal<Option<DevicesStatus>> {
    let status = use_signal(|| None::<DevicesStatus>);
    use_hook(move || {
        let Some(devices) = devices else { return };
        let mut changes = devices.changes();
        spawn(async move {
            loop {
                reread(&devices, status).await;
                if changes.changed().await.is_err() {
                    break;
                }
            }
        });
    });
    status
}

/// Reads the status into `status`; a failed read keeps what's shown.
async fn reread(devices: &DeviceSync, mut status: Signal<Option<DevicesStatus>>) {
    let read = devices.clone();
    if let Ok(next) = change(async move { read.status().await }).await {
        status.set(Some(next));
    }
}

#[component]
fn DeviceIcon(kind: DeviceKind, #[props(default = "size-5".to_owned())] class: String) -> Element {
    let icon = match kind {
        DeviceKind::Desktop => MONITOR,
        DeviceKind::Laptop => LAPTOP,
        DeviceKind::Handheld => GAMEPAD_2,
        DeviceKind::Tv => TV,
    };
    rsx! {
        Svg { icon, class }
    }
}

/// Pairs with the other device and waits for its library; the pairing's
/// codes come up in the pairing host beside it.
#[component]
pub fn DeviceStep(on_next: EventHandler<()>) -> Element {
    let devices = use_device_sync();
    let status = use_devices_status(devices.clone());
    let pairing = crate::flows::pairing::use_pairing();
    rsx! {
        if let Some(devices) = devices {
            FromDevice { devices, status, on_next }
        }
        // The codes to compare, as the webview mounts `PairingHost` here:
        // the app's own host isn't shown during setup.
        if let Some(pairing) = pairing {
            crate::flows::pairing::PairingHost { pairing }
        }
    }
}

#[component]
fn FromDevice(
    devices: DeviceSync,
    status: Signal<Option<DevicesStatus>>,
    on_next: EventHandler<()>,
) -> Element {
    let mut pairing = use_signal(|| false);
    let mut pair_error = use_signal(|| None::<ErrorReport>);
    let mut accepting = use_signal(|| false);
    let mut accept_error = use_signal(|| None::<ErrorReport>);
    let mut link = use_signal(String::new);
    // The first device paired here: a fresh install has no other.
    let source: Option<PairedDevice> = status
        .read()
        .as_ref()
        .and_then(|status| status.devices.first().cloned());
    let source_id = source.as_ref().map(|source| source.id.clone());
    // Its library comes over in a sync started now, once per device.
    let mut asked = use_signal(|| None::<String>);
    {
        let devices = devices.clone();
        use_effect(use_reactive!(|source_id| {
            let Some(id) = source_id else { return };
            if asked.peek().as_deref() == Some(id.as_str()) {
                return;
            }
            asked.set(Some(id.clone()));
            let devices = devices.clone();
            spawn(async move {
                let sync = devices.clone();
                let _ = change(async move { sync.sync_now(&id).await }).await;
                reread(&devices, status).await;
            });
        }));
    }
    let Some(current) = status() else {
        return rsx! {};
    };
    if current.this_device.identity != IdentityStatus::Ready {
        return rsx! {
            section { "data-part": "from-device",
                StepHeading {
                    title: t!("Set up from another device"),
                    description: t!("Getting this device ready to pair…"),
                }
            }
        };
    }

    if let Some(source) = source {
        let name = source.name.clone();
        let done = source.last_sync_ms.is_some() && !source.syncing;
        let (title, description) = if done {
            (
                t!("Your library is here"),
                t!(
                    "Games, accounts, collections and play history came from {name}. Next, set up what belongs to this device: its folders, emulators and how it starts.",
                    name = name
                ),
            )
        } else {
            (
                t!("Bringing your library from {name}", name = name),
                t!("This takes a moment on the same network, with or without the internet."),
            )
        };
        return rsx! {
            section {
                "data-part": "from-device",
                "data-status": if done { "synced" } else { "syncing" },
                StepHeading { title, description }
                div { class: "flex flex-col items-center gap-4",
                    if done {
                        Svg { icon: CIRCLE_CHECK, class: "size-10 text-success" }
                    } else {
                        p { role: "status", class: "text-sm text-fg-muted",
                            {t!("Syncing with {name}…", name = name)}
                        }
                    }
                    if source.sync_error.is_some() && !source.syncing {
                        p { role: "alert", class: "text-sm text-danger",
                            {t!("The last try didn't reach {name}. It tries again in a moment.", name = name)}
                        }
                    }
                    Button {
                        id: "onboarding-device-continue",
                        variant: ButtonVariant::Primary,
                        "data-focus-default": "true",
                        "data-spatial": "focusable",
                        disabled: !done,
                        on_press: move |_| on_next.call(()),
                        {t!("Continue")}
                    }
                }
            }
        };
    }

    let pair = {
        let devices = devices.clone();
        move |id: String| {
            let devices = devices.clone();
            pairing.set(true);
            pair_error.set(None);
            spawn(async move {
                let start = devices.clone();
                let result = change(async move { start.pair_nearby(&id).await }).await;
                pairing.set(false);
                if let Err(report) = result {
                    pair_error.set(Some(report));
                }
                reread(&devices, status).await;
            });
        }
    };
    let mut accept = {
        let devices = devices.clone();
        move || {
            let value = link().trim().to_owned();
            if value.is_empty() || accepting() {
                return;
            }
            let devices = devices.clone();
            accepting.set(true);
            accept_error.set(None);
            spawn(async move {
                let start = devices.clone();
                let result = change(async move { start.accept_pairing_link(&value).await }).await;
                accepting.set(false);
                if let Err(report) = result {
                    accept_error.set(Some(report));
                }
                reread(&devices, status).await;
            });
        }
    };
    let failure = pair_error().or_else(|| accept_error.cloned());
    let nearby: Vec<_> = current
        .nearby
        .iter()
        .enumerate()
        .map(|(index, device)| (index, device.clone(), device.id.clone()))
        .collect();
    rsx! {
        section { "data-part": "from-device", "data-status": "pairing",
            StepHeading {
                title: t!("Set up from another device"),
                description: t!("Open Caskade on the device you already use, on the same network. Pair with it here and compare the codes both show."),
            }
            div { class: "flex flex-col gap-6",
                div {
                    h2 { class: "mb-2 text-sm font-semibold", {t!("Nearby")} }
                    if current.nearby.is_empty() {
                        p { role: "status", class: "text-sm text-fg-muted",
                            {t!("Looking for Caskade on this network…")}
                        }
                    } else {
                        ul { "aria-label": t!("Nearby devices"), class: "flex flex-col gap-2",
                            for (index, device, id) in nearby {
                                li {
                                    key: "{device.id}",
                                    class: "flex items-center justify-between gap-3 rounded-control border border-border p-3",
                                    span { class: "flex items-center gap-3",
                                        DeviceIcon { kind: device.kind, class: "size-5 text-fg-muted" }
                                        span { class: "font-medium", "{device.name}" }
                                    }
                                    Button {
                                        id: "onboarding-pair-{index}",
                                        "data-spatial": "focusable",
                                        "data-focus-default": (index == 0).then_some("true"),
                                        variant: ButtonVariant::Primary,
                                        disabled: pairing(),
                                        on_press: {
                                            let mut pair = pair.clone();
                                            move |_| pair(id.clone())
                                        },
                                        {t!("Pair")}
                                    }
                                }
                            }
                        }
                    }
                }
                div {
                    h2 { class: "mb-2 text-sm font-semibold", {t!("Or use a pairing link")} }
                    p { class: "mb-2 text-sm text-fg-muted",
                        {t!("On the other device, open Settings → Devices and copy a pairing link, then paste it here.")}
                    }
                    div { class: "flex items-end gap-2",
                        TextField {
                            id: "onboarding-pairing-link",
                            "aria-label": t!("Pairing link"),
                            placeholder: LINK_PLACEHOLDER.to_owned(),
                            icon: LINK_2,
                            value: link(),
                            on_input: move |text| link.set(text),
                            class: "flex-1",
                        }
                        Button {
                            id: "onboarding-pairing-link-pair",
                            "data-spatial": "focusable",
                            disabled: link().trim().is_empty() || accepting(),
                            on_press: move |_| accept(),
                            {t!("Pair")}
                        }
                    }
                }
                if let Some(report) = failure {
                    FailureNotice { error: report, variant: NoticeVariant::Inline }
                }
            }
        }
    }
}

/// A size as Storage shows it.
fn bytes(value: u64) -> String {
    format::bytes(Some(value))
}

/// The other device's installed games that run here, to copy (#276).
#[component]
pub fn GamesStep(on_next: EventHandler<()>) -> Element {
    let devices = use_device_sync();
    let status = use_devices_status(devices.clone());
    let source: Option<PairedDevice> = status
        .read()
        .as_ref()
        .and_then(|status| status.devices.first().cloned());
    let source_id = source.as_ref().map(|source| source.id.clone());
    // Each answer says which device it's for: a new device's games are
    // still loading while the last answer is shown.
    let games = use_resource(use_reactive!(|source_id| async move {
        let Some(id) = source_id else {
            return (None, Ok(Vec::new()));
        };
        let Some(services) = crate::shell::services() else {
            return (Some(id), Ok(Vec::new()));
        };
        let read = id.clone();
        let games = change(async move {
            caskade_app::commands::addon_offers::device_games(
                &services.store,
                &services.devices,
                &read,
            )
            .await
        })
        .await;
        (Some(id), games)
    }));
    let storage = crate::flows::storage::use_storage();
    let roots = use_resource(move || {
        let storage = storage.clone();
        async move {
            match storage {
                Some(storage) => change(async move { storage.roots().await }).await,
                None => Ok(Vec::new()),
            }
        }
    });
    let mut chosen = use_signal(Vec::<String>::new);
    let mut copying = use_signal(|| false);
    let mut copy_error = use_signal(|| None::<ErrorReport>);

    let name = source
        .as_ref()
        .map(|source| source.name.clone())
        .unwrap_or_default();
    let loaded_for = games.read().as_ref().map(|(id, _)| id.clone());
    // Before the status is read too: the webview has it from the last step.
    let pending =
        status.read().is_none() || (source_id.is_some() && loaded_for.as_ref() != Some(&source_id));
    // A list that can't be read shows as none, as in the webview.
    let list: Vec<DeviceGame> = match &*games.read() {
        Some((_, Ok(list))) => list.clone(),
        _ => Vec::new(),
    };
    let roots_list: Vec<RootSummary> = match &*roots.read() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };
    let root = roots_list
        .iter()
        .find(|row| row.root.is_default)
        .or(roots_list.first());
    let free = root.and_then(|root| root.capabilities.free_bytes);
    let picked = chosen();
    let sizes: Vec<u64> = list
        .iter()
        .filter(|game| picked.contains(&game.game_id))
        .map(|game| game.size_bytes.unwrap_or(0))
        .collect();
    let needed: u64 = sizes.iter().sum();
    // A copy arrives in the cache before it installs: on one disk, the
    // largest game takes twice its size while it does (#233).
    let peak = needed + sizes.iter().copied().max().unwrap_or(0);
    let fits = free.is_none_or(|free| peak <= free);

    if pending {
        return rsx! {
            section { "data-part": "question", "data-question": "games",
                StepHeading {
                    title: t!("Games from {name}", name = name),
                    description: t!("Looking at what it has…"),
                }
            }
        };
    }

    if list.is_empty() {
        return rsx! {
            section { "data-part": "question", "data-question": "games",
                StepHeading {
                    title: t!("Games from {name}", name = name),
                    description: t!(
                        "{name} has no installed games that can be copied to this device. Install games here as usual.",
                        name = name
                    ),
                }
                Continue { on_press: move |_| on_next.call(()) }
            }
        };
    }

    let count = picked.len();
    let needed_text = bytes(needed);
    let peak_text = bytes(peak);
    let space = match free {
        None => t!("{neededText} chosen", neededText = needed_text),
        Some(free) if fits => t!(
            "{neededText} chosen, {freeText} free here",
            neededText = needed_text,
            freeText = bytes(free)
        ),
        Some(free) => t!(
            "{neededText} chosen needs {peakText} while it copies, but only {freeText} is free here",
            neededText = needed_text,
            peakText = peak_text,
            freeText = bytes(free)
        ),
    };
    let copy = {
        let source_id = source.as_ref().map(|source| source.id.clone());
        move || {
            let Some(device_id) = source_id.clone() else {
                on_next.call(());
                return;
            };
            let games = chosen();
            copying.set(true);
            copy_error.set(None);
            spawn(async move {
                let result = change(async move {
                    // Without the app's services there's nothing to copy with,
                    // as the webview without its API.
                    let Some(services) = crate::shell::services() else {
                        return Ok(());
                    };
                    let Some(install) = services.install.as_ref() else {
                        return Ok(());
                    };
                    for game_id in games {
                        caskade_app::commands::copies::game_copy_here(
                            install,
                            &services.store,
                            &services.devices,
                            game_id,
                            device_id.clone(),
                            None,
                        )
                        .await?;
                    }
                    Ok::<(), ErrorReport>(())
                })
                .await;
                copying.set(false);
                match result {
                    Ok(()) => on_next.call(()),
                    Err(report) => copy_error.set(Some(report)),
                }
            });
        }
    };
    let label = if count == 0 {
        t!("Continue without copying")
    } else {
        tn!("Copy # game", "Copy # games", count)
    };
    rsx! {
        section { "data-part": "question", "data-question": "games",
            StepHeading {
                title: t!("Games from {name}", name = name),
                description: t!("Choose the games to copy here over your network. They copy in the background while you finish setting up. Nothing copies unless you choose it."),
            }
            ul {
                "aria-label": t!("Games to copy"),
                class: "flex max-h-[40vh] flex-col gap-1 overflow-y-auto rounded-panel border border-border-strong bg-surface-raised p-2",
                for game in list {
                    li {
                        key: "{game.game_id}",
                        "data-part": "device-game",
                        class: "flex items-center justify-between gap-4 rounded-control px-3 py-2",
                        Checkbox {
                            id: "onboarding-copy-{game.game_id}",
                            checked: picked.contains(&game.game_id),
                            label: game.title.clone(),
                            on_change: {
                                let id = game.game_id.clone();
                                move |on: bool| {
                                    let mut list = chosen.write();
                                    list.retain(|known| *known != id);
                                    if on {
                                        list.push(id.clone());
                                    }
                                }
                            },
                        }
                        span { class: "shrink-0 text-xs text-fg-muted",
                            {match game.size_bytes {
                                Some(size) => bytes(size),
                                None => t!(note = "A game whose size isn't known", "Size unknown"),
                            }}
                            if game.through_proton {
                                {format!(" · {}", t!("Runs through Proton"))}
                            }
                        }
                    }
                }
            }
            p {
                "data-part": "space",
                "data-status": if fits { "fits" } else { "too-big" },
                role: if fits { None } else { Some("alert") },
                class: if fits { "mt-3 flex items-center gap-2 text-sm text-fg-muted" } else { "mt-3 flex items-center gap-2 text-sm text-danger" },
                Svg { icon: HARD_DRIVE, class: "size-4 shrink-0" }
                "{space}"
            }
            if let Some(report) = copy_error() {
                p { role: "alert", class: "mt-3 text-center text-sm text-danger", "{report.summary}" }
            }
            Continue {
                on_press: {
                    let mut copy = copy.clone();
                    move |_| {
                        if count == 0 {
                            on_next.call(());
                        } else {
                            copy();
                        }
                    }
                },
                disabled: !fits || copying(),
                label,
            }
            if count > 0 {
                div { class: "mt-3 flex justify-center",
                    Button {
                        id: "onboarding-games-not-now",
                        variant: ButtonVariant::Ghost,
                        disabled: copying(),
                        on_press: move |_| on_next.call(()),
                        {t!("Not now")}
                    }
                }
            }
        }
    }
}
