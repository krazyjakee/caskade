//! Settings-to-scan mapping. The injected host decides the platform; no live
//! machine checks or launcher subprocesses occur at this boundary.
use caskade_core::settings::{DiscoverySettings, LauncherSettings, SettingsService};
use caskade_discovery::{
    Config, OperatingSystem, SourceId,
    orchestrator::{SourceKey, SourceSelection},
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{path::PathBuf, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OsCheck {
    pub id: String,
    pub label: String,
    pub sources: Vec<String>,
}
pub fn os_checks(os: OperatingSystem) -> Vec<OsCheck> {
    let rows: &[(&str, &str, &[&str])] = match os {
        OperatingSystem::Windows => &[
            (
                "registry",
                "Installed games registry and Game Explorer",
                &["uninstall", "gameux"],
            ),
            ("start-menu", "Start Menu shortcuts", &["startmenu"]),
            ("uwp", "Xbox and packaged apps", &["xbox"]),
        ],
        OperatingSystem::Macos => &[(
            "applications",
            "System and user app bundles",
            &["macos.apps"],
        )],
        OperatingSystem::Linux => &[
            ("desktop", "Desktop entries", &["desktop"]),
            (
                "xdg",
                "Flatpak and Snap application records",
                &["flatpak", "snap"],
            ),
            ("wine", "Wine and Proton prefixes", &["prefixes"]),
        ],
    };
    rows.iter()
        .map(|(id, label, sources)| OsCheck {
            id: (*id).into(),
            label: os_check_label(id).unwrap_or_else(|| (*label).into()),
            sources: sources.iter().map(|s| (*s).into()).collect(),
        })
        .collect()
}
/// An OS check's name in the core's language.
fn os_check_label(id: &str) -> Option<String> {
    Some(match id {
        "registry" => caskade_core::tr!("Installed games registry and Game Explorer"),
        "start-menu" => caskade_core::tr!("Start Menu shortcuts"),
        "uwp" => caskade_core::tr!("Xbox and packaged apps"),
        "applications" => caskade_core::tr!("System and user app bundles"),
        "desktop" => caskade_core::tr!("Desktop entries"),
        "xdg" => caskade_core::tr!("Flatpak and Snap application records"),
        "wine" => caskade_core::tr!("Wine and Proton prefixes"),
        _ => return None,
    })
}
pub const fn os_name(os: OperatingSystem) -> &'static str {
    match os {
        OperatingSystem::Windows => "windows",
        OperatingSystem::Linux => "linux",
        OperatingSystem::Macos => "macos",
    }
}
pub async fn preferences(
    settings: &SettingsService,
) -> Result<(DiscoverySettings, LauncherSettings), String> {
    let discovery = settings
        .get("core.discovery")
        .await
        .map_err(|e| e.to_string())?;
    let launchers = settings
        .get("core.launchers")
        .await
        .map_err(|e| e.to_string())?;
    Ok((
        serde_json::from_value(discovery.value).map_err(|e| e.to_string())?,
        serde_json::from_value(launchers.value).map_err(|e| e.to_string())?,
    ))
}
pub fn scan_config(
    discovery: &DiscoverySettings,
    launchers: &LauncherSettings,
    os: OperatingSystem,
) -> Result<Config, String> {
    let mut config = Config::default();
    let mut disabled: Vec<_> = launchers
        .enabled
        .iter()
        .filter(|(_, enabled)| !**enabled)
        .map(|(id, _)| SourceId::from(id.to_owned()))
        .collect();
    if disabled.iter().any(|s| s.as_str() == "steam") {
        disabled.push(SourceId::new("steam.shortcuts"));
    }
    for check in os_checks(os) {
        if discovery
            .os_checks
            .get(os_name(os))
            .and_then(|s| s.get(&check.id))
            == Some(&false)
        {
            disabled.extend(check.sources.iter().map(|id| SourceId::from(id.to_owned())));
        }
    }
    config.sources = SourceSelection::AllExcept(disabled.into_iter().collect());
    if discovery.deep_scans.custom_folders {
        for path in &discovery.scan_paths {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(caskade_core::tr!(
                    "Custom scan folders must be absolute paths."
                ));
            }
            config.deep.folders.push(path);
        }
    }
    config.deep.external_drives = discovery.deep_scans.external_drives;
    config.deep.portable = discovery.deep_scans.portable_executables;
    config.deep.wine_prefixes = discovery.deep_scans.wine_prefixes;
    config.deep.full_disk = discovery.deep_scans.full_disk;
    config.exclusions.source_keys = discovery
        .ignored_entries
        .iter()
        .map(|entry| SourceKey {
            source: SourceId::from(entry.source.clone()),
            key: entry.key.clone(),
        })
        .collect();
    Ok(config)
}
pub fn refresh_interval(preferences: &DiscoverySettings) -> Duration {
    Duration::from_secs(match preferences.refresh_interval.as_str() {
        "1h" => 3600,
        "6h" => 6 * 3600,
        "12h" => 12 * 3600,
        "7d" => 7 * 24 * 3600,
        _ => 24 * 3600,
    })
}
