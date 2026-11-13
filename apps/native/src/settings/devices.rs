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

use crate::data::sleep;
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

mod paired;
mod sync_choices;
mod this_device;
mod transfers;
use paired::{LinkBadge, NearbySection, PairedSection};
use sync_choices::{SavesSection, SyncChoices};
use this_device::{IdentityNotice, ThisDeviceSection};
use transfers::CopiesAndShares;

/// The app's devices: a test's, else the running app's.
pub fn use_device_sync() -> Option<DeviceSync> {
    try_use_context::<DeviceSync>().or_else(|| crate::shell::services().map(DeviceSync::of))
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
    pending: Signal<bool>,
    failure: Signal<Option<ErrorReport>>,
    work: impl FnOnce(
        DeviceSync,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<T, ErrorReport>> + Send>>
    + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let poke = try_consume_context::<Shown>().map(|shown| shown.poke);
    crate::data::run(pending, failure, work(devices.clone()), move |value| {
        if let Some(value) = value {
            done(value);
        }
        if let Some(mut poke) = poke {
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
    // An action reads the status at once, as the webview invalidates it;
    // the watch above reads it on opening (#111).
    {
        let devices = devices.clone();
        use_effect(move || {
            if (shown.poke)() == 0 {
                return;
            }
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
