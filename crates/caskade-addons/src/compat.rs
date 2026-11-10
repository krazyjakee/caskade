//! What the catalogue offers this Caskade: each entry against what is
//! installed (by id and version) and what this build accepts (the plugin
//! API, the OS, the app mode), with the reasons a release doesn't fit,
//! worked out before anything is downloaded.

use super::{AddonKind, AddonOs, Catalogue, CatalogueEntry, CatalogueRelease};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppMode {
    Desktop,
    Console,
}

/// What this Caskade accepts.
pub struct HostFacts {
    /// Whether a plugin asking for this API range loads here (the plugin
    /// host's own rule).
    pub plugin_api: fn(&semver::VersionReq) -> bool,
    /// The plugin API this build provides, for the reasons shown.
    pub plugin_api_version: String,
    pub os: AddonOs,
    pub mode: AppMode,
}

/// A plugin or theme installed now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAddon {
    pub id: String,
    pub kind: AddonKind,
    /// None when its manifest can't be read.
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ReasonCode {
    /// Needs a plugin API this Caskade doesn't have.
    Api,
    /// Doesn't run on this OS.
    Os,
    /// Its UI doesn't support the app mode in use; it still installs.
    Mode,
    /// Its publisher deprecated it.
    Deprecated,
    /// Installed under the same id as the other kind (a theme for a plugin).
    Kind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityReason {
    pub code: ReasonCode,
    /// Stops installing (an API, OS or kind mismatch), or only limits it.
    pub blocking: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ItemState {
    /// Not installed, and a release fits.
    Available,
    /// Installed, at the newest release that fits or newer.
    Installed,
    /// Installed, and a newer release fits.
    UpdateAvailable,
    /// No release fits; see the reasons.
    Incompatible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Compatibility {
    pub state: ItemState,
    pub installed_version: Option<String>,
    /// The newest release that fits; the one Install or Update gets.
    pub release: Option<CatalogueRelease>,
    /// Why the newest release doesn't fit, or what limits the one that does.
    pub reasons: Vec<CompatibilityReason>,
}

/// An entry with what it means here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueItem {
    pub entry: CatalogueEntry,
    pub compatibility: Compatibility,
}

fn reasons(
    kind: AddonKind,
    release: &CatalogueRelease,
    host: &HostFacts,
) -> Vec<CompatibilityReason> {
    let mut reasons = Vec::new();
    if kind == AddonKind::Plugin
        && let Some(api) = &release.api
        && !semver::VersionReq::parse(api).is_ok_and(|api| (host.plugin_api)(&api))
    {
        reasons.push(CompatibilityReason {
            code: ReasonCode::Api,
            blocking: true,
            message: caskade_core::tr!(
                "{version} needs plugin API {api}; this Caskade has {host}",
                version = release.version,
                api = api,
                host = host.plugin_api_version
            ),
        });
    }
    if !release.os.is_empty() && !release.os.contains(&host.os) {
        reasons.push(CompatibilityReason {
            code: ReasonCode::Os,
            blocking: true,
            message: caskade_core::tr!(
                "{version} doesn't run on this operating system",
                version = release.version
            ),
        });
    }
    if !release.modes.is_empty() && !release.modes.contains(&host.mode) {
        reasons.push(CompatibilityReason {
            code: ReasonCode::Mode,
            blocking: false,
            message: match host.mode {
                AppMode::Console => caskade_core::tr!("Its pages work in desktop mode only."),
                AppMode::Desktop => caskade_core::tr!("Its pages work in console mode only."),
            },
        });
    }
    reasons
}

fn version(text: &str) -> Option<semver::Version> {
    semver::Version::parse(text).ok()
}

/// What `entry` means here, given what is installed.
pub fn evaluate_entry(
    entry: &CatalogueEntry,
    installed: &[InstalledAddon],
    host: &HostFacts,
) -> Compatibility {
    let mine = installed.iter().find(|addon| addon.id == entry.id);
    let installed_version = mine.and_then(|addon| addon.version.clone());
    let fits = entry.releases.iter().find(|release| {
        reasons(entry.kind, release, host)
            .iter()
            .all(|reason| !reason.blocking)
    });
    let mut found = match fits {
        Some(release) => reasons(entry.kind, release, host),
        None => entry
            .releases
            .first()
            .map(|newest| reasons(entry.kind, newest, host))
            .unwrap_or_default(),
    };
    if let Some(why) = &entry.deprecated {
        found.push(CompatibilityReason {
            code: ReasonCode::Deprecated,
            blocking: false,
            message: why.clone(),
        });
    }
    if let Some(addon) = mine
        && addon.kind != entry.kind
    {
        found.push(CompatibilityReason {
            code: ReasonCode::Kind,
            blocking: true,
            message: caskade_core::tr!("Something else is installed under the same id."),
        });
        return Compatibility {
            state: ItemState::Incompatible,
            installed_version,
            release: None,
            reasons: found,
        };
    }
    let state = match (mine, fits) {
        (_, None) => ItemState::Incompatible,
        (None, Some(_)) => ItemState::Available,
        (Some(addon), Some(release)) => {
            match (
                addon.version.as_deref().and_then(version),
                version(&release.version),
            ) {
                (Some(have), Some(offered)) if offered > have => ItemState::UpdateAvailable,
                // An unreadable installed version can be replaced.
                (None, Some(_)) => ItemState::UpdateAvailable,
                _ => ItemState::Installed,
            }
        }
    };
    // Installed and incompatible stays installed: it is there and works.
    let state = match (state, mine) {
        (ItemState::Incompatible, Some(_)) => ItemState::Installed,
        (state, _) => state,
    };
    Compatibility {
        state,
        installed_version,
        release: fits.cloned(),
        reasons: found,
    }
}

/// Every entry of `catalogue` with what it means here.
pub fn evaluate(
    catalogue: &Catalogue,
    installed: &[InstalledAddon],
    host: &HostFacts,
) -> Vec<CatalogueItem> {
    catalogue
        .entries
        .iter()
        .map(|entry| CatalogueItem {
            compatibility: evaluate_entry(entry, installed, host),
            entry: entry.clone(),
        })
        .collect()
}
