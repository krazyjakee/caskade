//! This device: its name and kind, whether it syncs, and why a device
//! can't be identified.

use super::*;

/// After the keychain lost this device's key: a new identity, once asked.
#[component]
pub(super) fn IdentityNotice(devices: DeviceSync) -> Element {
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

pub(super) fn kind_id(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Desktop => "desktop",
        DeviceKind::Laptop => "laptop",
        DeviceKind::Handheld => "handheld",
        DeviceKind::Tv => "tv",
    }
}

pub(super) fn kinds() -> Vec<(DeviceKind, String)> {
    vec![
        (DeviceKind::Desktop, tp!("device kind", "Desktop")),
        (DeviceKind::Laptop, tp!("device kind", "Laptop")),
        (DeviceKind::Handheld, tp!("device kind", "Handheld")),
        (DeviceKind::Tv, tp!("device kind", "TV")),
    ]
}

/// A switch on one field of `core.sync`.
#[component]
pub(super) fn SyncSwitch(
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
pub(super) fn ThisDeviceSection(devices: DeviceSync) -> Element {
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
