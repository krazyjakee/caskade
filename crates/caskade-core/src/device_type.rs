//! What kind of device this is (#244), and the first settings each kind
//! starts with: a handheld or a TV starts in console mode with emulators
//! fullscreen and a larger interface; a desktop or laptop keeps the
//! defaults.
//!
//! Detection reads the machine where it can: Linux's DMI names known
//! handhelds (Steam Deck, ROG Ally, Legion Go, …) and laptop chassis, and a
//! session inside gamescope is a handheld when it has a battery and a TV
//! otherwise. Windows keeps the same firmware names in the registry, where a
//! battery without a handheld's name is a laptop. Anything else is a desktop;
//! Settings → Devices can say otherwise.
//!
//! First defaults go only into sections never saved here (revision 0), so
//! they never replace a choice, and only once.

use std::path::Path;

use caskade_discovery::registry::{
    Registry, RegistryHive, RegistryKey, RegistryValue, RegistryView,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;

use crate::Result;
use crate::settings::SettingsService;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    #[default]
    Desktop,
    Laptop,
    Handheld,
    Tv,
}

/// Product names of handheld PCs, as their DMI reports them.
const HANDHELDS: &[&str] = &[
    "jupiter",   // Steam Deck LCD
    "galileo",   // Steam Deck OLED
    "rog ally",  // ASUS ROG Ally
    "rc71l",     // ROG Ally
    "rc72la",    // ROG Ally X
    "83e1",      // Lenovo Legion Go
    "legion go", // Lenovo Legion Go (S)
    "83l3",      // Legion Go S
    "83n6",      // Legion Go S
    "ayaneo",
    "aya neo",
    "onexplayer",
    "gpd win",
    "aokzoe",
    "claw a1m", // MSI Claw
    "msi claw",
];

/// SMBIOS chassis types of portable computers.
const LAPTOP_CHASSIS: &[u32] = &[8, 9, 10, 14, 31, 32];
/// A tablet chassis.
const TABLET_CHASSIS: u32 = 30;

/// Where Windows keeps the firmware's names for this machine.
const WINDOWS_BIOS: &str = r"HARDWARE\DESCRIPTION\System\BIOS";
/// The firmware names that can say a handheld. A Legion Go's product is only
/// a code, and a new model's only its family names. `BaseBoardProduct` stays
/// out: HP's are four hex digits, and `83E1` would make one a Legion Go.
const WINDOWS_NAMES: &[&str] = &["SystemProductName", "SystemFamily"];
/// ACPI batteries, one subkey per battery Windows has ever seen here. Its
/// volatile `Control` subkey exists only while the battery is present, so a
/// drive moved out of a laptop doesn't make a desktop one.
const WINDOWS_BATTERIES: &str = r"SYSTEM\CurrentControlSet\Enum\ACPI\PNP0C0A";

/// This device, detected once per run.
pub fn detected() -> DeviceKind {
    static DETECTED: std::sync::OnceLock<DeviceKind> = std::sync::OnceLock::new();
    *DETECTED.get_or_init(detect)
}

/// This device, as far as it can be told.
#[cfg(windows)]
pub fn detect() -> DeviceKind {
    detect_windows(&caskade_discovery::registry::NativeRegistry::default())
}

/// This device, as far as it can be told.
#[cfg(not(windows))]
pub fn detect() -> DeviceKind {
    detect_in(
        Path::new("/"),
        in_gamescope(|name| std::env::var(name).ok()),
    )
}

/// Detection against a filesystem rooted at `root` (tests use a folder).
pub fn detect_in(root: &Path, gamescope: bool) -> DeviceKind {
    let read = |path: &str| {
        std::fs::read_to_string(root.join(path))
            .map(|text| text.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };
    let names = [
        read("sys/class/dmi/id/product_name"),
        read("sys/class/dmi/id/product_family"),
        read("sys/class/dmi/id/board_name"),
    ];
    if names.iter().any(|name| names_a_handheld(name)) {
        return DeviceKind::Handheld;
    }
    let battery = std::fs::read_dir(root.join("sys/class/power_supply")).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with("BAT"))
    });
    if gamescope {
        return if battery {
            DeviceKind::Handheld
        } else {
            DeviceKind::Tv
        };
    }
    match read("sys/class/dmi/id/chassis_type").parse::<u32>() {
        Ok(TABLET_CHASSIS) if battery => DeviceKind::Handheld,
        Ok(chassis) if LAPTOP_CHASSIS.contains(&chassis) => DeviceKind::Laptop,
        _ => DeviceKind::Desktop,
    }
}

/// Detection against a Windows registry (tests use one in memory). Windows
/// has no gamescope, and a battery alone makes a laptop: a Windows tablet
/// keeps the desktop shell until the user says otherwise.
pub fn detect_windows(registry: &dyn Registry) -> DeviceKind {
    if windows_names(registry)
        .iter()
        .any(|name| names_a_handheld(name))
    {
        DeviceKind::Handheld
    } else if windows_battery(registry) {
        DeviceKind::Laptop
    } else {
        DeviceKind::Desktop
    }
}

/// The firmware's names for this machine, as far as they can be read.
pub fn windows_names(registry: &dyn Registry) -> Vec<String> {
    let bios = RegistryKey::new(
        RegistryHive::LocalMachine,
        RegistryView::Native,
        WINDOWS_BIOS,
    );
    WINDOWS_NAMES
        .iter()
        .filter_map(|name| match registry.value(&bios, name) {
            Ok(Some(RegistryValue::String(text))) if !text.trim().is_empty() => Some(text),
            _ => None,
        })
        .collect()
}

/// Whether a battery is present now.
pub fn windows_battery(registry: &dyn Registry) -> bool {
    let key =
        |path: String| RegistryKey::new(RegistryHive::LocalMachine, RegistryView::Native, path);
    registry
        .subkeys(&key(WINDOWS_BATTERIES.to_owned()))
        .unwrap_or_default()
        .iter()
        .any(|battery| {
            registry
                .key_exists(&key(format!(r"{WINDOWS_BATTERIES}\{battery}\Control")))
                .unwrap_or(false)
        })
}

fn names_a_handheld(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    HANDHELDS.iter().any(|known| name.contains(known))
}

/// gamescope sets `GAMESCOPE_WAYLAND_DISPLAY` for its clients; Steam's game
/// mode also sets `SteamGamepadUI`.
pub fn in_gamescope(var: impl Fn(&str) -> Option<String>) -> bool {
    var("GAMESCOPE_WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
        || var("XDG_CURRENT_DESKTOP").is_some_and(|value| value.eq_ignore_ascii_case("gamescope"))
        || var("SteamGamepadUI").as_deref() == Some("1")
}

/// The fields a kind of device starts with, by section.
pub fn first_defaults(kind: DeviceKind) -> Vec<(&'static str, Value)> {
    match kind {
        DeviceKind::Desktop | DeviceKind::Laptop => Vec::new(),
        DeviceKind::Handheld => vec![
            ("core.console", json!({ "active": true })),
            ("core.emulation", json!({ "desktopFullscreen": true })),
            ("core.appearance", json!({ "uiScale": 115 })),
        ],
        DeviceKind::Tv => vec![
            ("core.console", json!({ "active": true })),
            ("core.emulation", json!({ "desktopFullscreen": true })),
            ("core.appearance", json!({ "uiScale": 125 })),
        ],
    }
}

fn invalid(error: impl std::fmt::Display) -> crate::Error {
    crate::Error::Invalid(error.to_string())
}

/// Writes `kind`'s first defaults into the sections never saved here.
/// Returns the sections written.
pub async fn apply_first_defaults(
    settings: &SettingsService,
    kind: DeviceKind,
) -> Result<Vec<&'static str>> {
    let mut written = Vec::new();
    for (namespace, fields) in first_defaults(kind) {
        let snapshot = settings.get(namespace).await.map_err(invalid)?;
        if snapshot.revision > 0 {
            continue;
        }
        let mut value = snapshot.value;
        if let (Some(object), Some(fields)) = (value.as_object_mut(), fields.as_object()) {
            for (field, field_value) in fields {
                object.insert(field.clone(), field_value.clone());
            }
        }
        settings
            .set(namespace, value, Some(0))
            .await
            .map_err(invalid)?;
        written.push(namespace);
    }
    Ok(written)
}

/// On a new install only (first-run setup never saved, no games), writes
/// `kind`'s first defaults; an existing library keeps what it has.
pub async fn settle_new_device(
    settings: &SettingsService,
    store: &crate::db::LibraryStore,
    kind: DeviceKind,
) -> Result<Vec<&'static str>> {
    let setup = settings.get("core.onboarding").await.map_err(invalid)?;
    if setup.revision > 0 {
        return Ok(Vec::new());
    }
    let games = store
        .list_games(&crate::model::ListGames {
            limit: 1,
            ..crate::model::ListGames::default()
        })
        .await?
        .total;
    if games > 0 {
        return Ok(Vec::new());
    }
    apply_first_defaults(settings, kind).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(files: &[(&str, &str)], battery: bool) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let supply = root.path().join("sys/class/power_supply");
        std::fs::create_dir_all(supply.join("AC")).unwrap();
        if battery {
            std::fs::create_dir_all(supply.join("BAT0")).unwrap();
        }
        root
    }

    #[test]
    fn handhelds_laptops_tvs_and_desktops_are_told_apart() {
        let deck = machine(&[("sys/class/dmi/id/product_name", "Jupiter\n")], true);
        assert_eq!(detect_in(deck.path(), false), DeviceKind::Handheld);
        let ally = machine(
            &[("sys/class/dmi/id/product_name", "ROG Ally RC71L_RC71L")],
            true,
        );
        assert_eq!(detect_in(ally.path(), false), DeviceKind::Handheld);
        let laptop = machine(
            &[
                ("sys/class/dmi/id/product_name", "ThinkPad X1"),
                ("sys/class/dmi/id/chassis_type", "10"),
            ],
            true,
        );
        assert_eq!(detect_in(laptop.path(), false), DeviceKind::Laptop);
        let desktop = machine(&[("sys/class/dmi/id/chassis_type", "3")], false);
        assert_eq!(detect_in(desktop.path(), false), DeviceKind::Desktop);
        // gamescope: a handheld with a battery, a TV box without.
        assert_eq!(detect_in(desktop.path(), true), DeviceKind::Tv);
        let unknown_handheld = machine(&[], true);
        assert_eq!(
            detect_in(unknown_handheld.path(), true),
            DeviceKind::Handheld
        );
        // Nothing readable at all: a desktop.
        assert_eq!(
            detect_in(Path::new("/nonexistent-root"), false),
            DeviceKind::Desktop
        );
        assert!(in_gamescope(
            |name| (name == "SteamGamepadUI").then(|| "1".into())
        ));
        assert!(!in_gamescope(|_| None));
    }
}
