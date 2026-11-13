//! Paired devices and the devices in reach to pair with.

use super::*;

/// How long Pair a device looks before saying it found nobody (#60).
pub(super) const NEARBY_PATIENCE: Duration = Duration::from_secs(30);

/// How a device is reached, for people (`components/sync/common`).
pub(super) fn link_label(link: DeviceLink) -> String {
    match link {
        DeviceLink::Lan => t!("Local network"),
        DeviceLink::Direct => t!("Internet, direct"),
        DeviceLink::Relay => t!("Internet, through a server"),
        DeviceLink::Offline => t!("Offline"),
    }
}

pub(super) fn link_style(link: DeviceLink) -> (crate::ui::Icon, &'static str) {
    match link {
        DeviceLink::Lan => (NETWORK, "text-status-installed"),
        DeviceLink::Direct => (GLOBE, "text-accent-alt"),
        DeviceLink::Relay => (RADIO_TOWER, "text-warning"),
        DeviceLink::Offline => (MOON, "text-fg-subtle"),
    }
}

pub(super) fn link_name(link: DeviceLink) -> &'static str {
    match link {
        DeviceLink::Lan => "lan",
        DeviceLink::Direct => "direct",
        DeviceLink::Relay => "relay",
        DeviceLink::Offline => "offline",
    }
}

/// How a device is reached: the local network, the internet, a relay, or
/// not at all.
#[component]
pub(super) fn LinkBadge(link: DeviceLink) -> Element {
    let (icon, tone) = link_style(link);
    rsx! {
        span {
            "data-cs": "peer-link",
            "data-status": link_name(link),
            class: "inline-flex items-center gap-1.5 text-xs whitespace-nowrap {tone}",
            Svg { icon, class: "size-3.5" }
            {link_label(link)}
        }
    }
}

/// A device's kind, as an icon.
#[component]
pub(super) fn DeviceIcon(
    kind: DeviceKind,
    #[props(default = "size-5".to_owned())] class: String,
) -> Element {
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

#[component]
pub(super) fn PairedSection(devices: DeviceSync) -> Element {
    let shown = use_shown();
    let paired = match &*shown.status.read() {
        Some(Ok(status)) => status.devices.clone(),
        _ => return rsx! {},
    };
    rsx! {
        SettingsSection {
            icon: MONITOR_SMARTPHONE,
            title: t!("Your devices"),
            description: t!("Paired devices keep your library, play history and collections in step. Each keeps its own games, folders and emulators."),
            if paired.is_empty() {
                p { class: "text-sm text-fg-muted", {t!("No devices paired yet.")} }
            } else {
                ul { class: "flex flex-col gap-2",
                    for device in paired {
                        DeviceRow { key: "{device.id}", devices: devices.clone(), device }
                    }
                }
            }
        }
    }
}

#[component]
pub(super) fn DeviceRow(devices: DeviceSync, device: PairedDevice) -> Element {
    let shown = use_shown();
    let admin = (shown.admin)();
    let syncing = use_signal(|| false);
    let forgetting = use_signal(|| false);
    let failure = use_signal(|| None::<ErrorReport>);
    let mut confirming = use_signal(|| false);
    let os = os_name(device.os);
    let name = device.name.clone();
    let state = if device.syncing {
        t!("{os} · Syncing…", os = os)
    } else if let Some(at) = device.last_sync_ms {
        t!(
            "{os} · Synced {last}",
            os = os,
            last = crate::library::date_time(at)
        )
    } else {
        t!("{os} · Not synced yet", os = os)
    };
    let id = device.id.clone();
    rsx! {
        li {
            "data-cs": "paired-device",
            "data-status": link_name(device.link),
            class: "flex flex-wrap items-center gap-4 rounded-control border border-border p-3",
            DeviceIcon { kind: device.kind, class: "size-6 shrink-0 text-fg-muted" }
            div { class: "flex min-w-0 flex-1 flex-col gap-1",
                span { class: "flex flex-wrap items-center gap-3",
                    span { class: "font-medium", "{name}" }
                    LinkBadge { link: device.link }
                }
                span { "data-part": "sync", class: "text-xs text-fg-muted", "{state}" }
                if device.sync_error.is_some() && !device.syncing {
                    span { "data-part": "sync-error", class: "text-xs text-fg-muted",
                        {t!("Last try didn't reach it. It syncs when it's back.")}
                    }
                }
            }
            div { class: "flex flex-wrap gap-2",
                Button {
                    size: ButtonSize::Sm,
                    icon: REFRESH_CW,
                    disabled: device.syncing || syncing(),
                    on_press: {
                        let (devices, id) = (devices.clone(), id.clone());
                        move |_| {
                            let id = id.clone();
                            act(&devices, syncing, failure, move |devices| Box::pin(async move { devices.sync_now(&id).await }), |_| {});
                        }
                    },
                    {t!("Sync now")}
                }
                if admin {
                    if confirming() {
                        Button {
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Danger,
                            disabled: forgetting(),
                            on_press: {
                                let (devices, id) = (devices.clone(), id.clone());
                                move |_| {
                                    let id = id.clone();
                                    act(&devices, forgetting, failure, move |devices| Box::pin(async move { devices.forget(&id).await }), |_| {});
                                }
                            },
                            {t!("Forget {name}", name = name.clone())}
                        }
                        Button { size: ButtonSize::Sm, variant: ButtonVariant::Ghost, on_press: move |_| confirming.set(false), {t!("Keep")} }
                    } else {
                        Button {
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Ghost,
                            icon: TRASH,
                            "aria-label": t!("Forget {name}", name = name.clone()),
                            on_press: move |_| confirming.set(true),
                        }
                    }
                }
            }
            if confirming() {
                p { class: "w-full text-xs text-fg-muted",
                    {t!("It stops syncing with this device. Nothing is deleted on either device, and what synced stays.")}
                }
            }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
        }
    }
}

#[component]
pub(super) fn NearbySection(devices: DeviceSync) -> Element {
    let shown = use_shown();
    let pairing = use_signal(|| false);
    let pair_failure = use_signal(|| None::<ErrorReport>);
    let linking = use_signal(|| false);
    let link_failure = use_signal(|| None::<ErrorReport>);
    let mut link = use_signal(|| None::<PairingLinkInfo>);
    let mut copied = use_signal(|| None::<bool>);
    let accepting = use_signal(|| false);
    let accept_failure = use_signal(|| None::<ErrorReport>);
    let mut accepted = use_signal(|| None::<String>);
    let mut pasted = use_signal(String::new);
    let mut found_nobody = use_signal(|| false);
    let status = match &*shown.status.read() {
        Some(Ok(status)) => status.clone(),
        _ => return rsx! {},
    };
    let searching = status.nearby.is_empty();
    // Says it found nobody after looking a while; the core goes on looking.
    let _patience = use_resource(use_reactive!(|searching| async move {
        found_nobody.set(false);
        if searching {
            sleep(NEARBY_PATIENCE).await;
            found_nobody.set(true);
        }
    }));
    let busy = pairing() || status.pairing.is_some();
    let accept = {
        let devices = devices.clone();
        use_callback(move |()| {
            let value = pasted().trim().to_owned();
            if value.is_empty() || accepting() {
                return;
            }
            act(
                &devices,
                accepting,
                accept_failure,
                move |devices| Box::pin(async move { devices.accept_pairing_link(&value).await }),
                move |device| {
                    pasted.set(String::new());
                    accepted.set(Some(device.name));
                },
            );
        })
    };
    rsx! {
        SettingsSection {
            icon: RADAR,
            title: t!("Pair a device"),
            description: t!("Caskade on this network shows here. Both devices show a code: pair when they match."),
            if searching {
                p { role: "status", class: "text-sm text-fg-muted",
                    if found_nobody() {
                        {t!("No devices found on this network yet. Caskade keeps looking, and one that appears shows here. Otherwise, pair with a link below.")}
                    } else {
                        {t!("Looking for Caskade on this network…")}
                    }
                }
            } else {
                ul { class: "flex flex-col gap-2",
                    for device in status.nearby.clone() {
                        NearbyRow {
                            key: "{device.id}",
                            device: device.clone(),
                            pending: busy,
                            on_pair: {
                                let devices = devices.clone();
                                move |_| {
                                    let id = device.id.clone();
                                    act(&devices, pairing, pair_failure, move |devices| Box::pin(async move { devices.pair_nearby(&id).await }), |_| {});
                                }
                            },
                        }
                    }
                }
            }
            if let Some(report) = pair_failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
            div { class: "mt-6 flex flex-col gap-3 border-t border-border pt-4",
                p { class: "text-sm text-fg-muted",
                    {t!("Device not here? Guest Wi‑Fi and some routers keep devices apart, and devices elsewhere never show. Pair with a link instead: copy it here and paste it on the other device. It works once, for ten minutes.")}
                }
                div { class: "flex flex-wrap gap-2",
                    Button {
                        id: "pairing-link-copy",
                        icon: LINK_2,
                        disabled: linking(),
                        on_press: {
                            let devices = devices.clone();
                            move |_| {
                                act(
                                    &devices,
                                    linking,
                                    link_failure,
                                    |devices| Box::pin(async move { devices.pairing_link().await }),
                                    move |info: PairingLinkInfo| {
                                        let url = info.url.clone();
                                        link.set(Some(info));
                                        // Through the core: the clipboard is the app's.
                                        copied.set(Some(crate::paste::copy_text(url).is_ok()));
                                    },
                                );
                            }
                        },
                        if copied() == Some(true) { {t!("Pairing link copied")} } else { {t!("Copy a pairing link")} }
                    }
                }
                if let Some(info) = link() {
                    p { "data-part": "pairing-link", class: "break-all rounded-control border border-border p-2 font-mono text-xs text-fg-muted",
                        "{info.url}"
                    }
                    if copied() == Some(false) {
                        p { role: "alert", class: "text-sm text-warning",
                            {t!("The link couldn't be copied. Select it above and copy it yourself.")}
                        }
                    }
                }
                if let Some(report) = link_failure() {
                    FailureNotice { error: report, variant: NoticeVariant::Inline }
                }
                TextField {
                    id: "pairing-link-paste",
                    label: t!("Paste a pairing link from another device"),
                    value: pasted(),
                    mono: true,
                    on_input: move |text| {
                        accepted.set(None);
                        pasted.set(text);
                    },
                    on_submit: move |_| accept.call(()),
                    action: rsx! {
                        Button {
                            id: "pairing-link-pair",
                            disabled: pasted().trim().is_empty() || accepting(),
                            on_press: move |_| accept.call(()),
                            {t!("Pair")}
                        }
                    },
                }
                if let Some(name) = accepted() {
                    p { role: "status", class: "text-sm text-status-installed",
                        {t!("Paired with {0}.", 0 = name)}
                    }
                }
                if let Some(report) = accept_failure() {
                    FailureNotice { error: report, variant: NoticeVariant::Inline }
                }
            }
        }
    }
}

#[component]
pub(super) fn NearbyRow(device: NearbyDevice, pending: bool, on_pair: EventHandler<()>) -> Element {
    let os = os_name(device.os);
    let address = device.addresses.first().cloned().unwrap_or_default();
    rsx! {
        li { "data-part": "nearby", class: "flex items-center gap-4 rounded-control border border-dashed border-border-strong p-3",
            DeviceIcon { kind: device.kind, class: "size-6 shrink-0 text-fg-muted" }
            div { class: "flex min-w-0 flex-1 flex-col",
                span { class: "font-medium", "{device.name}" }
                span { class: "font-mono text-xs text-fg-muted", "{os} · {address}" }
            }
            Button {
                variant: ButtonVariant::Primary,
                disabled: pending,
                on_press: move |_| on_pair.call(()),
                {t!("Pair…")}
            }
        }
    }
}
