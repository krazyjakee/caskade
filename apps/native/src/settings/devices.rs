//! Settings → Devices (#231, #105), as the webview's `devices/`: this
//! device, the devices paired with it, Caskade on this network to pair,
//! what syncs, saves, and copies and shares under way. Devices find each
//! other on the local network and sync without the internet; elsewhere
//! they connect directly, or through a relay unless that is turned off.
//!
//! Every account sees the list; only an admin pairs, forgets, renames this
//! device or changes what syncs (the core refuses the rest whatever this
//! shows). The pairing itself (codes to compare) is the frame's
//! `flows::pairing::PairingHost`.

use std::time::Duration;

use caskade_core::error::ErrorReport;
use caskade_ui_data::sync::{
    DeviceKind, DeviceLink, DeviceSync, DevicesStatus, IdentityStatus, NearbyDevice, PairedDevice,
    PairingLinkInfo, TransferDirection, TransferState, TransferView,
};
use caskade_ui_i18n::{format, t, tp};
use dioxus_native::prelude::*;
use serde_json::{Value, json};

use super::state::{SectionState, SettingsStatus, use_settings_section};
use crate::data::change;
use crate::flows::devices::os_name;
use crate::ui::icons::{
    ARROW_DOWN_TO_LINE, ARROW_UP_FROM_LINE, CHEVRON_DOWN, CHEVRON_RIGHT, DOWNLOAD, GAMEPAD_2,
    GLOBE, HARD_DRIVE, KEY_ROUND, LAPTOP, LINK_2, MONITOR, MONITOR_SMARTPHONE, MOON, NETWORK, PLAY,
    RADAR, RADIO_TOWER, REFRESH_CCW, REFRESH_CW, SAVE, SHARE_2, SHIELD_CHECK, TRASH, TV,
};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, FailureNotice, NoticeVariant, Progress, Segment,
    SegmentedControl, Select, SelectOption, SettingRow, SettingsSection, Svg, Switch, TextField,
};

/// How long Pair a device looks before saying it found nobody (#60).
const NEARBY_PATIENCE: Duration = Duration::from_secs(30);
/// How often copies under way are read again.
const TRANSFERS_EVERY: Duration = Duration::from_secs(2);

/// The app's devices: a test's, else the running app's.
pub fn use_device_sync() -> Option<DeviceSync> {
    try_use_context::<DeviceSync>().or_else(|| crate::shell::services().map(DeviceSync::of))
}

async fn sleep(time: Duration) {
    // Timers need the app runtime's reactor; Dioxus's executor has none.
    let _ = caskade_app::runtime::spawn(async move { tokio::time::sleep(time).await }).await;
}

/// What the panel reads: the status (read again on every change to the
/// devices) and whether this account is an admin.
#[derive(Clone, Copy, PartialEq)]
struct Shown {
    status: Signal<Option<Result<DevicesStatus, ErrorReport>>>,
    admin: Signal<bool>,
    /// Bumped to read again now.
    poke: Signal<u64>,
}

fn use_shown() -> Shown {
    use_context::<Shown>()
}

/// Runs `work` on the app's runtime; `done` hears it worked, `failure`
/// why not.
fn act<T: Send + 'static>(
    devices: &DeviceSync,
    mut pending: Signal<bool>,
    mut failure: Signal<Option<ErrorReport>>,
    work: impl FnOnce(
        DeviceSync,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<T, ErrorReport>> + Send>>
    + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let devices = devices.clone();
    let mut poke = try_consume_context::<Shown>().map(|shown| shown.poke);
    pending.set(true);
    failure.set(None);
    spawn(async move {
        let result = change(work(devices)).await;
        pending.set(false);
        match result {
            Ok(value) => done(value),
            Err(report) => failure.set(Some(report)),
        }
        if let Some(poke) = poke.as_mut() {
            *poke.write() += 1;
        }
    });
}

/// Settings → Devices.
#[component]
pub fn DevicesSettings() -> Element {
    let Some(devices) = use_device_sync() else {
        return rsx! {};
    };
    rsx! {
        DevicesPanel { devices }
    }
}

#[component]
fn DevicesPanel(devices: DeviceSync) -> Element {
    let shown = Shown {
        status: use_signal(|| None),
        admin: use_signal(|| false),
        poke: use_signal(|| 0),
    };
    use_context_provider(|| shown);
    {
        let devices = devices.clone();
        use_hook(move || {
            let mut changes = devices.changes();
            spawn(async move {
                loop {
                    let read = devices.clone();
                    let read = change(async move {
                        let status = read.status().await;
                        Ok((status, read.is_admin().await))
                    })
                    .await;
                    if let Ok((status, admin)) = read {
                        let (mut current, mut is_admin) = (shown.status, shown.admin);
                        current.set(Some(status));
                        is_admin.set(admin);
                    }
                    if changes.changed().await.is_err() {
                        break;
                    }
                }
            });
        });
    }
    // An action reads the status at once, as the webview invalidates it.
    {
        let devices = devices.clone();
        use_effect(move || {
            let _ = (shown.poke)();
            let devices = devices.clone();
            spawn(async move {
                if let Ok(status) = change(async move { Ok(devices.status().await) }).await {
                    let mut current = shown.status;
                    current.set(Some(status));
                }
            });
        });
    }
    let admin = (shown.admin)();
    let status = (shown.status)();
    let unpaired = matches!(&status, Some(Ok(status)) if status.devices.is_empty());
    let loaded = status.is_some();
    rsx! {
        div { class: "flex flex-col gap-6",
            // This device, its paired devices and pairing need the status;
            // when it can't be read, say so once (#55).
            if let Some(Err(report)) = status.clone() {
                FailureNotice {
                    error: report,
                    title: t!("Caskade couldn't read this device's connections"),
                    on_retry: move |_| {
                        let mut poke = shown.poke;
                        *poke.write() += 1;
                    },
                }
            }
            if !admin {
                p { role: "note", class: "text-sm text-fg-muted",
                    {t!("Only an admin can pair devices, forget them or change what syncs.")}
                }
            }
            IdentityNotice { devices: devices.clone() }
            ThisDeviceSection { devices: devices.clone() }
            // Before the first pairing, Pair a device comes before the
            // empty list (#54).
            div { class: "flex flex-col gap-6",
                if admin && unpaired {
                    NearbySection { devices: devices.clone() }
                }
                PairedSection { devices: devices.clone() }
                if admin && !unpaired {
                    NearbySection { devices: devices.clone() }
                }
            }
            // Waits for the status, so the page doesn't start long and shrink.
            if loaded {
                SyncChoices { collapsed: unpaired, admin }
            }
            SavesSection { admin }
            CopiesAndShares { devices }
        }
    }
}

/// After the keychain lost this device's key: a new identity, once asked.
#[component]
fn IdentityNotice(devices: DeviceSync) -> Element {
    let shown = use_shown();
    let admin = (shown.admin)();
    let pending = use_signal(|| false);
    let failure = use_signal(|| None::<ErrorReport>);
    let mut confirming = use_signal(|| false);
    let identity = match &*shown.status.read() {
        Some(Ok(status)) => status.this_device.identity,
        _ => return rsx! {},
    };
    if matches!(identity, IdentityStatus::Ready | IdentityStatus::Starting) {
        return rsx! {};
    }
    let unavailable = identity == IdentityStatus::KeychainUnavailable;
    rsx! {
        div { role: "alert", class: "flex flex-col gap-3 rounded-panel border border-warning/60 bg-surface-raised p-4",
            p { class: "flex items-center gap-2 font-medium",
                Svg { icon: KEY_ROUND, class: "size-4 text-warning" }
                if unavailable {
                    {t!("Sync is waiting for the system keychain")}
                } else {
                    {t!("This device's sync key is gone")}
                }
            }
            p { class: "text-sm text-fg-muted",
                if unavailable {
                    {t!("The keychain didn't answer. Unlock it, then try again. Everything else works meanwhile.")}
                } else {
                    {t!("The keychain no longer holds the key your other devices know this one by. A new key makes this a new device to them: pair each of them again afterwards.")}
                }
            }
            if admin {
                div { class: "flex flex-wrap gap-2",
                    if unavailable {
                        Button {
                            icon: REFRESH_CW,
                            disabled: pending(),
                            on_press: {
                                let devices = devices.clone();
                                move |_| act(&devices, pending, failure, |devices| Box::pin(async move { devices.retry_identity().await }), |_| {})
                            },
                            {t!("Try again")}
                        }
                    } else if confirming() {
                        Button {
                            variant: ButtonVariant::Danger,
                            disabled: pending(),
                            on_press: {
                                let devices = devices.clone();
                                move |_| act(&devices, pending, failure, |devices| Box::pin(async move { devices.create_identity().await }), |_| {})
                            },
                            {t!("Make a new key")}
                        }
                        Button { variant: ButtonVariant::Ghost, on_press: move |_| confirming.set(false), {t!("Not now")} }
                    } else {
                        Button { icon: KEY_ROUND, on_press: move |_| confirming.set(true), {t!("Make a new key…")} }
                    }
                }
            }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
        }
    }
}

fn kind_id(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Desktop => "desktop",
        DeviceKind::Laptop => "laptop",
        DeviceKind::Handheld => "handheld",
        DeviceKind::Tv => "tv",
    }
}

fn kinds() -> Vec<(DeviceKind, String)> {
    vec![
        (DeviceKind::Desktop, tp!("device kind", "Desktop")),
        (DeviceKind::Laptop, tp!("device kind", "Laptop")),
        (DeviceKind::Handheld, tp!("device kind", "Handheld")),
        (DeviceKind::Tv, tp!("device kind", "TV")),
    ]
}

/// A switch on one field of `core.sync`.
#[component]
fn SyncSwitch(
    sync: SectionState,
    admin: bool,
    field: &'static str,
    default: bool,
    label: String,
    description: String,
    icon: crate::ui::Icon,
) -> Element {
    let value = sync.value();
    let on = value
        .as_ref()
        .and_then(|value| value.get(field))
        .and_then(Value::as_bool)
        .unwrap_or(default);
    rsx! {
        SettingRow { icon, label, description,
            Switch {
                checked: on,
                disabled: !admin || value.is_none() || sync.saving(),
                on_change: move |next: bool| sync.set_field(field, Value::Bool(next)),
            }
        }
    }
}

#[component]
fn ThisDeviceSection(devices: DeviceSync) -> Element {
    let shown = use_shown();
    let admin = (shown.admin)();
    let sync = use_settings_section("core.sync");
    let mut name = use_signal(|| None::<String>);
    let renaming = use_signal(|| false);
    let mut renamed = use_signal(|| false);
    let rename_failure = use_signal(|| None::<ErrorReport>);
    let kind_pending = use_signal(|| false);
    let kind_failure = use_signal(|| None::<ErrorReport>);
    let device = match &*shown.status.read() {
        Some(Ok(status)) => status.this_device.clone(),
        _ => return rsx! {},
    };
    let saved_name = device.name.clone();
    let save = {
        let devices = devices.clone();
        use_callback(move |()| {
            let Some(next) = name() else { return };
            if next == saved_name {
                return;
            }
            act(
                &devices,
                renaming,
                rename_failure,
                move |devices| Box::pin(async move { devices.rename(&next).await }),
                move |_| renamed.set(true),
            );
        })
    };
    let addresses = device.lan_addresses.join(", ");
    let kind_options: Vec<SelectOption> = kinds()
        .into_iter()
        .map(|(kind, label)| SelectOption::new(kind_id(kind), label))
        .collect();
    let description = if !admin {
        None
    } else if renaming() {
        Some(t!("Saving…"))
    } else if renamed() {
        Some(t!("Saved."))
    } else {
        Some(t!("Saved when you leave the field or press Enter."))
    };
    let network = if addresses.is_empty() {
        t!("Not on a local network.")
    } else {
        t!("On this network at {addresses}.", addresses = addresses)
    };
    let internet = match device.internet {
        Some(true) => Some(t!("Connected to the internet.")),
        Some(false) => Some(t!("No internet: devices on this network still sync.")),
        None => None,
    };
    rsx! {
        SettingsSection {
            icon: MONITOR,
            title: t!("This device"),
            description: t!("Your other devices know it by this name. Devices on the same network find each other and sync without the internet."),
            div { class: "flex flex-wrap items-start gap-4",
                div {
                    class: "min-w-64 flex-1",
                    // Saved on leaving the field or Enter, and says so (#55).
                    onfocusout: move |_| save.call(()),
                    TextField {
                        id: "device-name",
                        label: t!("Device name"),
                        value: name().unwrap_or_else(|| device.name.clone()),
                        readonly: !admin,
                        description,
                        on_input: move |text| {
                            renamed.set(false);
                            name.set(Some(text));
                        },
                        on_submit: move |_| save.call(()),
                    }
                }
                Select {
                    id: "device-kind",
                    label: t!("Kind"),
                    options: kind_options,
                    value: Some(kind_id(device.kind).to_owned()),
                    disabled: !admin,
                    class: "w-44",
                    on_change: {
                        let devices = devices.clone();
                        move |id: String| {
                            if let Some((kind, _)) = kinds().into_iter().find(|(kind, _)| kind_id(*kind) == id) {
                                act(&devices, kind_pending, kind_failure, move |devices| Box::pin(async move { devices.set_kind(kind).await }), |_| {});
                            }
                        }
                    },
                }
            }
            p { class: "mt-2 text-xs text-fg-muted",
                "{network}"
                if let Some(internet) = internet {
                    " {internet}"
                }
            }
            if let Some(report) = rename_failure().or(kind_failure()) {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
            div { class: "mt-4",
                SyncSwitch {
                    sync, admin, field: "neverRelay", default: device.never_relay,
                    icon: SHIELD_CHECK,
                    label: t!("Connect only directly"),
                    description: t!("Away from home, devices that can't reach each other directly go through a server that can't read what passes. On, they connect only directly: on this network, or over the internet where the routers allow it, and some may then not connect."),
                }
                SyncSwitch {
                    sync, admin, field: "remotePlay", default: true,
                    icon: PLAY,
                    label: t!("Play requests from my devices"),
                    description: t!("Your other devices can start a game here, as Play here would."),
                }
                SyncSwitch {
                    sync, admin, field: "acceptCopies", default: false,
                    icon: DOWNLOAD,
                    label: t!("Accept copies from my devices"),
                    description: t!("A game another of your devices sends here is copied into the default storage without asking."),
                }
                SyncSwitch {
                    sync, admin, field: "peerDownloads", default: true,
                    icon: NETWORK,
                    label: t!("Downloads from my devices"),
                    description: t!("Get a download from another of your devices on this network when it has the same file, before the internet. Game files only ever travel directly between your devices."),
                }
                SettingsStatus { section: sync, what: t!("Connections") }
            }
        }
    }
}

/// How a device is reached, for people (`components/sync/common`).
fn link_label(link: DeviceLink) -> String {
    match link {
        DeviceLink::Lan => t!("Local network"),
        DeviceLink::Direct => t!("Internet, direct"),
        DeviceLink::Relay => t!("Internet, through a server"),
        DeviceLink::Offline => t!("Offline"),
    }
}

fn link_style(link: DeviceLink) -> (crate::ui::Icon, &'static str) {
    match link {
        DeviceLink::Lan => (NETWORK, "text-status-installed"),
        DeviceLink::Direct => (GLOBE, "text-accent-alt"),
        DeviceLink::Relay => (RADIO_TOWER, "text-warning"),
        DeviceLink::Offline => (MOON, "text-fg-subtle"),
    }
}

fn link_name(link: DeviceLink) -> &'static str {
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
fn LinkBadge(link: DeviceLink) -> Element {
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

#[component]
fn PairedSection(devices: DeviceSync) -> Element {
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
fn DeviceRow(devices: DeviceSync, device: PairedDevice) -> Element {
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
            last = format::date_time(crate::library::local_date_time(at))
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
fn NearbySection(devices: DeviceSync) -> Element {
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
                                        copied.set(Some(caskade_app::commands::clipboard::clipboard_write_text(url).is_ok()));
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
fn NearbyRow(device: NearbyDevice, pending: bool, on_pair: EventHandler<()>) -> Element {
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

/// What syncs between paired devices (#227, `core.sync.categories`).
const SYNCED: [&str; 8] = [
    "library",
    "playSessions",
    "accounts",
    "artwork",
    "saves",
    "achievementLinks",
    "rommServers",
    "appearance",
];
/// Kept on each device until the user turns them on.
const OPTIONAL: [&str; 5] = [
    "launchActions",
    "gameScripts",
    "preferredEmulators",
    "emulatorConfig",
    "addons",
];

fn category_label(category: &str) -> (String, String) {
    match category {
        "library" => (
            t!("Library"),
            t!(
                "Game details you changed, favourites, hidden games, collections, matches you set and games you kept apart."
            ),
        ),
        "playSessions" => (
            t!("Play history"),
            t!("Every session from every device, so play time adds up wherever you play."),
        ),
        "accounts" => (
            t!("Accounts and PINs"),
            t!(
                "Who plays here, their age limits and PINs. Wrong PIN attempts are counted on each device."
            ),
        ),
        "artwork" => (
            t!("Your artwork"),
            t!("Covers and images you chose or imported."),
        ),
        "saves" => (t!("Saves"), t!("Game saves, with every version kept.")),
        "achievementLinks" => (
            t!("Achievement links you made"),
            t!("Games you linked to their achievements by hand."),
        ),
        "rommServers" => (
            t!("RomM servers"),
            t!("Servers and what they import. Each device signs in with its own password."),
        ),
        "appearance" => (t!("Appearance"), t!("Theme, accent and layout.")),
        "launchActions" => (
            t!("Launch actions"),
            t!(
                "Play and utility actions you added. Their programs must be at the same place on each device."
            ),
        ),
        "gameScripts" => (
            t!("Game scripts"),
            t!("Scripts that run with one game. They arrive turned off on other devices."),
        ),
        "preferredEmulators" => (
            t!("Preferred emulators"),
            t!(
                "Which emulator each platform uses. One this device doesn't have is offered in Settings → Emulation."
            ),
        ),
        "emulatorConfig" => (
            t!("Emulator configuration"),
            t!(
                "Controller mappings, core options and shader presets, each in this device's own emulator folders."
            ),
        ),
        _ => (
            t!("Plugins and themes"),
            t!(
                "Offers to install the same ones. Plugins still ask for their permissions on each device."
            ),
        ),
    }
}

fn local_only() -> [String; 7] {
    [
        t!("Where games are installed"),
        t!("Storage folders"),
        t!("Emulators and their profiles"),
        t!("ROM folders"),
        t!("Store sign-ins (connect each device)"),
        t!("Downloaded metadata and images"),
        t!("Console mode, window and start-up choices"),
    ]
}

/// A category's state when the settings don't name it: synced ones sync.
fn state_of(value: &Value, category: &str) -> &'static str {
    match value
        .pointer(&format!("/categories/{category}"))
        .and_then(Value::as_str)
    {
        Some("sync") => "sync",
        Some(_) => "thisDeviceOnly",
        None if SYNCED.contains(&category) => "sync",
        None => "thisDeviceOnly",
    }
}

#[component]
fn SyncChoices(collapsed: bool, admin: bool) -> Element {
    let sync = use_settings_section("core.sync");
    let mut expanded = use_signal(|| false);
    let title = t!("What syncs");
    let description = t!(
        "Changes made on any device reach the others when they next connect. Where two devices changed the same thing, the later change wins. This device only keeps a kind out of sync here: this device neither sends its changes nor takes the others'."
    );
    let Some(value) = sync.value() else {
        // Loading says nothing; a failed load says why, with Try again.
        return match sync.load_error() {
            Some(_) => rsx! {
                SettingsSection { icon: REFRESH_CCW, title: title.clone(), description,
                    SettingsStatus { section: sync, what: title, class: "" }
                }
            },
            None => rsx! {},
        };
    };
    let segments = vec![
        Segment::new("sync", tp!("sync choice", "Sync")),
        Segment::new("thisDeviceOnly", t!("This device only")),
    ];
    let row = move |category: &'static str| {
        let (label, description) = category_label(category);
        let segments = segments.clone();
        rsx! {
            SettingRow { key: "{category}", label, description,
                SegmentedControl {
                    "data-category": category,
                    options: segments,
                    value: state_of(&value, category).to_owned(),
                    disabled: !admin || sync.saving(),
                    on_change: move |state: String| {
                        sync.set(move |mut current| {
                            if !current.get("categories").is_some_and(Value::is_object) {
                                current["categories"] = json!({});
                            }
                            current["categories"][category] = Value::String(state.clone());
                            current
                        })
                    },
                }
            }
        }
    };
    let rows = rsx! {
        div { class: "flex flex-col",
            for category in SYNCED { {row(category)} }
        }
        h3 { class: "mt-6 text-sm font-medium", {t!("Usually different on each device")} }
        p { class: "mb-1 text-xs text-fg-muted", {t!("These stay on each device until you choose to sync them.")} }
        div { class: "flex flex-col",
            for category in OPTIONAL { {row(category)} }
        }
    };
    let never_title = t!("Never synced");
    let never_description = t!("These belong to each device's own drives and hardware.");
    let never = rsx! {
        ul { "data-part": "never", class: "grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-x-6 gap-y-1.5 text-sm text-fg-muted",
            for label in local_only() {
                li { key: "{label}", class: "flex items-center gap-2",
                    span { "aria-hidden": "true", class: "size-1 rounded-full bg-fg-subtle" }
                    "{label}"
                }
            }
        }
    };
    if collapsed {
        return rsx! {
            SettingsSection {
                icon: REFRESH_CCW,
                title: title.clone(),
                description: t!("Nothing syncs until a device is paired. Then your library, play history and saves stay in step, unless you choose otherwise."),
                SettingsStatus { section: sync, what: title.clone(), class: "mb-3" }
                Button {
                    id: "sync-choose",
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    icon: if expanded() { CHEVRON_DOWN } else { CHEVRON_RIGHT },
                    "aria-expanded": if expanded() { "true" } else { "false" },
                    on_press: move |_| expanded.toggle(),
                    if admin { {t!("Choose what syncs")} } else { {t!("See what syncs")} }
                }
                if expanded() {
                    p { class: "mt-2 text-sm text-fg-muted", "{description}" }
                    {rows}
                    h3 { class: "mt-6 text-sm font-medium", "{never_title}" }
                    p { class: "mb-2 text-xs text-fg-muted", "{never_description}" }
                    {never}
                }
            }
        };
    }
    rsx! {
        SettingsSection { icon: REFRESH_CCW, title: title.clone(), description,
            SettingsStatus { section: sync, what: title, class: "mb-3" }
            {rows}
        }
        SettingsSection { icon: HARD_DRIVE, title: never_title, description: never_description,
            {never}
        }
    }
}

/// Saves (#241): whether saves sync (set in What syncs) and how many
/// versions each game keeps.
#[component]
fn SavesSection(admin: bool) -> Element {
    let sync = use_settings_section("core.sync");
    let saves = use_settings_section("core.saves");
    let title = t!("Saves");
    let description = t!(
        "A version of a game's saves is kept each time you finish playing, and the newest goes to your other devices, so you can carry on where you left off."
    );
    let (Some(sync_value), Some(saves_value)) = (sync.value(), saves.value()) else {
        // Loading says nothing; a failed load says why, with Try again.
        return if sync.load_error().is_some() || saves.load_error().is_some() {
            rsx! {
                SettingsSection { icon: SAVE, title: title.clone(), description,
                    SettingsStatus { section: sync, what: t!("Sync settings"), class: "" }
                    SettingsStatus { section: saves, what: title, class: "" }
                }
            }
        } else {
            rsx! {}
        };
    };
    let syncing = state_of(&sync_value, "saves") == "sync";
    let keep = saves_value
        .get("keep")
        .and_then(Value::as_u64)
        .unwrap_or(20);
    let mut options: Vec<SelectOption> = [5u64, 10, 20, 50, 100]
        .into_iter()
        .map(|count| SelectOption::new(count.to_string(), t!("{count} versions", count = count)))
        .collect();
    if !options
        .iter()
        .any(|option| option.value == keep.to_string())
    {
        options.push(SelectOption::new(
            keep.to_string(),
            t!("{keep} versions", keep = keep),
        ));
    }
    rsx! {
        SettingsSection { icon: SAVE, title, description,
            // Whether saves sync is What syncs' Saves row; one control (#54).
            p { "data-part": "saves-sync", class: "mt-1 text-sm text-fg-muted",
                if syncing {
                    {t!("Saves go to your other devices. Saves in What syncs above changes that.")}
                } else {
                    {t!("Saves stay on this device. Saves in What syncs above changes that.")}
                }
            }
            SettingRow {
                label: t!("Versions to keep"),
                description: t!("For each game. The newest stay, and so does any save that isn't settled between devices yet."),
                Select {
                    id: "saves-keep",
                    options,
                    value: Some(keep.to_string()),
                    disabled: !admin || saves.saving(),
                    on_change: move |count: String| {
                        if let Ok(count) = count.parse::<u64>() {
                            saves.set_field("keep", json!(count));
                        }
                    },
                }
            }
            SettingsStatus { section: sync, what: t!("Sync settings") }
            SettingsStatus { section: saves, what: t!("Versions to keep") }
        }
    }
}

/// Games moving between this user's devices (#233), and games shared with
/// other people and received from them (#246, #247). With nothing under
/// way or shared, the section stays out of sight.
#[component]
fn CopiesAndShares(devices: DeviceSync) -> Element {
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
                    if let Ok(list) = change(async move { read.transfers().await }).await
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
fn TransferRow(transfer: TransferView) -> Element {
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
