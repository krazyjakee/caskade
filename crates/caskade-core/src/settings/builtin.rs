//! Core preference schemas. Installer/discovery adapters register later
//! sections through the same registry; storage roots themselves stay in DB.
use super::{
    InstallerSettings, Result, SectionDefinition, SettingsError, SettingsService, SettingsSnapshot,
};
use crate::AppPaths;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneralSettings {
    pub launch_on_login: bool,
    pub start_minimized: bool,
    #[schemars(regex(pattern = "^(home|library|collections|last)$"))]
    pub start_view: String,
    #[schemars(regex(pattern = "^(stay|minimize|tray)$"))]
    pub on_launch: String,
    #[schemars(regex(pattern = "^(quit|tray)$"))]
    pub on_close: String,
    pub confirm_quit: bool,
    #[schemars(length(min = 1, max = 4096))]
    pub cache_dir: String,
    /// `system` (the OS's languages) or a BCP 47 tag such as `sv` or
    /// `pt-BR`. The core resolves it to a language it has a catalog for,
    /// else English (`i18n::follow_setting`).
    #[schemars(
        regex(pattern = "^(system|[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*)$"),
        length(max = 64)
    )]
    pub language: String,
    /// The clock's hours: `system` (the language's own 12 or 24), `12h`
    /// or `24h` (#218).
    #[schemars(regex(pattern = "^(system|12h|24h)$"))]
    pub time_format: String,
}
impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            launch_on_login: false,
            start_minimized: false,
            start_view: "home".into(),
            on_launch: "minimize".into(),
            on_close: "tray".into(),
            confirm_quit: true,
            cache_dir: "cache".into(),
            language: "system".into(),
            time_format: "system".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppearanceSettings {
    /// The active theme's id (#140), reverse-DNS like `caskade.default`.
    /// The Appearance picker only offers installed, valid themes; one that
    /// went missing later falls back to `caskade.default` with a notice
    /// (`ThemeService::resolve`).
    #[schemars(
        regex(pattern = r"^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$"),
        length(max = 128)
    )]
    pub theme: String,
    #[schemars(regex(pattern = "^(compact|comfortable)$"))]
    pub density: String,
    #[schemars(range(min = 128, max = 288))]
    pub card_size: u32,
    #[schemars(range(min = 80, max = 150))]
    pub ui_scale: u32,
    pub show_art_titles: bool,
    #[schemars(regex(pattern = "^(system|full|reduced)$"))]
    pub motion: String,
    pub reduce_effects: bool,
    /// A custom accent colour; `None` uses the theme's (#141).
    #[serde(default)]
    #[schemars(regex(pattern = "^#[0-9a-fA-F]{6}$"))]
    pub accent: Option<String>,
    /// The user's CSS (#142), applied in `@layer cs.user`, so it beats
    /// themes and components without `!important`. At most 32 Ki
    /// characters (the app's editor holds the same limit).
    #[serde(default)]
    #[schemars(length(max = 32768))]
    pub custom_css: String,
    /// Off: the custom CSS is kept but not applied, to recover from a
    /// broken look.
    #[serde(default = "enabled")]
    pub custom_css_enabled: bool,
}
fn enabled() -> bool {
    true
}
impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: caskade_theme::DEFAULT_THEME.into(),
            density: "comfortable".into(),
            card_size: 176,
            ui_scale: 100,
            show_art_titles: true,
            motion: "system".into(),
            reduce_effects: false,
            accent: None,
            custom_css: String::new(),
            custom_css_enabled: true,
        }
    }
}

/// `core.appearance` 1 → 2: the closed `nexus|terminal` theme choice becomes
/// a theme id. Nexus is `caskade.default`; Terminal never shipped, so it
/// (and anything else) starts on the default too. The custom CSS fields
/// take their defaults.
fn appearance_v2(mut previous: serde_json::Value) -> Result<serde_json::Value> {
    let object = previous
        .as_object_mut()
        .ok_or(super::SettingsError::Stored)?;
    object.insert("theme".into(), caskade_theme::DEFAULT_THEME.into());
    Ok(previous)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LauncherSettings {
    pub enabled: BTreeMap<String, bool>,
}
impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            enabled: [
                "steam",
                "heroic",
                "lutris",
                "epic",
                "gog",
                "ea",
                "ubisoft",
                "battlenet",
                "itch",
                "amazon",
                "riot",
                "rockstar",
            ]
            .into_iter()
            .map(|id| (id.into(), true))
            .collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeepScanSettings {
    pub custom_folders: bool,
    pub external_drives: bool,
    pub rom_directories: bool,
    pub portable_executables: bool,
    pub wine_prefixes: bool,
    pub full_disk: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscoverySettings {
    pub enabled: bool,
    pub scan_on_startup: bool,
    pub background_refresh: bool,
    #[schemars(regex(pattern = "^(1h|6h|12h|24h|7d)$"))]
    pub refresh_interval: String,
    pub rescan_after_launcher_install: bool,
    pub watch_folders: bool,
    pub merge_duplicates: bool,
    pub prefer_launcher_metadata: bool,
    pub deep_scans: DeepScanSettings,
    pub os_checks: BTreeMap<String, BTreeMap<String, bool>>,
    /// Absolute custom scan roots, used only with deepScans.customFolders.
    #[serde(default)]
    #[schemars(length(max = 64))]
    pub scan_paths: Vec<String>,
    /// Source/key pairs ignored from the discovery review list.
    #[serde(default)]
    #[schemars(length(max = 1024))]
    pub ignored_entries: Vec<DiscoveryExclusion>,
    /// Explicit review approvals, preserving classifier confidence and evidence.
    #[serde(default)]
    #[schemars(length(max = 1024))]
    pub accepted_entries: Vec<DiscoveryExclusion>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscoveryExclusion {
    #[schemars(length(min = 1, max = 128))]
    pub source: String,
    #[schemars(length(min = 1, max = 4096))]
    pub key: String,
    /// The game's title when the decision was made, so Settings can name
    /// it (#53). Decisions from before it have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = 512))]
    pub title: Option<String>,
}
impl Default for DiscoverySettings {
    fn default() -> Self {
        let os_checks = [
            (
                "windows",
                &["registry", "manifests", "start-menu", "uwp"][..],
            ),
            (
                "macos",
                &["applications", "user-apps", "manifests", "bundles"][..],
            ),
            ("linux", &["desktop", "xdg", "launcher-config", "wine"][..]),
        ]
        .into_iter()
        .map(|(os, checks)| {
            (
                os.into(),
                checks.iter().map(|check| ((*check).into(), true)).collect(),
            )
        })
        .collect();
        Self {
            enabled: true,
            scan_on_startup: true,
            background_refresh: true,
            refresh_interval: "24h".into(),
            rescan_after_launcher_install: true,
            watch_folders: true,
            merge_duplicates: true,
            prefer_launcher_metadata: true,
            deep_scans: DeepScanSettings::default(),
            os_checks,
            scan_paths: Vec::new(),
            ignored_entries: Vec::new(),
            accepted_entries: Vec::new(),
        }
    }
}

/// Console mode (#148): how Caskade starts, and the mode it was left in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConsoleSettings {
    /// `auto`: console inside gamescope (Steam Deck game mode), otherwise
    /// the mode Caskade was last left in. `desktop` and `console` always
    /// start in that mode.
    #[schemars(regex(pattern = "^(auto|desktop|console)$"))]
    pub start: String,
    /// The mode last chosen with the toggle: `true` for console.
    pub active: bool,
}
impl Default for ConsoleSettings {
    fn default() -> Self {
        Self {
            start: "auto".into(),
            active: false,
        }
    }
}

/// Emulated games. Console mode always plays them
/// fullscreen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmulationSettings {
    #[schemars(title = "Desktop full screen")]
    /// Play emulated games fullscreen in desktop mode too.
    pub desktop_fullscreen: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageSettings {
    pub always_use_default: bool,
}

/// Game updates (`crate::updates`): newer releases of games installed from
/// GitHub, found without an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSettings {
    #[schemars(title = "Check for game updates")]
    /// Look for new releases of games installed from GitHub in the background.
    pub check: bool,
    #[schemars(title = "Check every (hours)", range(min = 1, max = 168))]
    /// How long to wait between checks of a game.
    pub interval_hours: u32,
    #[schemars(title = "Offer updates before playing")]
    /// When a newer release is out, ask before starting the game whether to update it first.
    pub ask_before_play: bool,
}
impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check: true,
            interval_hours: 6,
            ask_before_play: true,
        }
    }
}

/// Caskade's own updates: the app checks its release feed and offers
/// new versions; nothing installs without being asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppUpdateSettings {
    #[schemars(title = "Check for Caskade updates")]
    /// Look for a new version of Caskade at startup and every few hours.
    pub check: bool,
}
impl Default for AppUpdateSettings {
    fn default() -> Self {
        Self { check: true }
    }
}

/// Game streaming: whether other devices may stream
/// games from this one. Hosting is switched on and off through the
/// streaming commands, which install and set up the host first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StreamingSettings {
    #[schemars(title = "Let other devices stream games from this one")]
    /// Other devices on the local network can play this device's games.
    pub hosting: bool,
    #[serde(default)]
    #[schemars(title = "Devices added by address")]
    /// Devices to stream from that aren't found on the network by themselves.
    pub addresses: Vec<String>,
    #[serde(default)]
    #[schemars(title = "Use the streaming already set up on this device")]
    /// Host through the streaming program someone already installed here, changing only Caskade's games in it.
    pub existing: bool,
    #[serde(default)]
    #[schemars(title = "Stream quality")]
    /// How games streamed from other devices look; each part left empty is chosen automatically.
    pub quality: StreamQuality,
}

/// Stream quality (#264): each part `None` is the client's automatic
/// choice.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, specta::Type,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StreamQuality {
    #[serde(default)]
    #[schemars(
        title = "Resolution",
        regex(pattern = r"^[1-9][0-9]{2,4}x[1-9][0-9]{2,4}$")
    )]
    /// Width and height in pixels, like 1920x1080.
    pub resolution: Option<String>,
    #[serde(default)]
    #[schemars(title = "Frame rate", range(min = 10, max = 240))]
    /// Frames per second.
    pub fps: Option<u32>,
    #[serde(default)]
    #[schemars(title = "Bitrate", range(min = 500, max = 500000))]
    /// Kilobits per second.
    pub bitrate_kbps: Option<u32>,
}

/// First-run setup: whether it has been done, and its sounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnboardingSettings {
    /// Setup has been finished or skipped. Until then the library's first
    /// scan, and everything that goes online, waits for it.
    pub completed: bool,
    /// Play setup's sounds.
    pub sounds: bool,
}
impl Default for OnboardingSettings {
    fn default() -> Self {
        Self {
            completed: false,
            sounds: true,
        }
    }
}

/// What Caskade fetches from the internet without being asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetadataSettings {
    #[schemars(title = "Fetch artwork and game info online")]
    /// Download Hasheous' game databases and match games to IGDB in the
    /// background, for covers, descriptions and genres.
    pub online: bool,
}
impl Default for MetadataSettings {
    fn default() -> Self {
        Self { online: true }
    }
}

/// The downloaded save-location manifest (`crate::ludusavi`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavesSettings {
    #[schemars(
        title = "Save manifest URL",
        regex(pattern = r"^https?://[^\s]+$"),
        length(max = 4096)
    )]
    /// Download save-location definitions from this URL.
    pub manifest_url: String,
    #[schemars(title = "Save versions to keep", range(min = 1, max = 1000))]
    /// Versions kept per game; the newest stay.
    #[serde(default = "default_keep")]
    pub keep: u32,
}
fn default_keep() -> u32 {
    crate::saves::versions::KEEP
}
impl Default for SavesSettings {
    fn default() -> Self {
        Self {
            manifest_url: crate::ludusavi::MANIFEST_URL.into(),
            keep: default_keep(),
        }
    }
}

/// Choices for every account (#208), which only an admin changes: no
/// Settings section owns `core.accounts` (`accounts::Permission`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountsSettings {
    #[schemars(title = "Rating board")]
    /// The board whose ratings games show and age limits are picked in;
    /// one of `igdb::age::BOARDS`.
    #[schemars(regex(pattern = "^(pegi|esrb|usk|cero|acb|grac|classind)$"))]
    pub rating_board: String,
}
impl Default for AccountsSettings {
    fn default() -> Self {
        Self {
            rating_board: crate::igdb::age::DEFAULT_BOARD.into(),
        }
    }
}

impl SettingsService {
    /// Sets the stream quality in `core.streaming`, leaving the rest of the
    /// section alone. It has its own call because it decides how games look
    /// on this device, so any account may change it (#66), while the
    /// section's hosting stays an admin's.
    pub async fn set_stream_quality(&self, quality: &StreamQuality) -> Result<SettingsSnapshot> {
        for _ in 0..8 {
            let current = self.get("core.streaming").await?;
            let mut value = current.value;
            value["quality"] = serde_json::to_value(quality)?;
            match self
                .set("core.streaming", value, Some(current.revision))
                .await
            {
                Err(SettingsError::Conflict) => continue,
                result => return result,
            }
        }
        Err(SettingsError::Conflict)
    }

    pub fn register_core(&self, paths: &AppPaths) -> Result<()> {
        let general = GeneralSettings {
            cache_dir: paths.cache.to_string_lossy().into_owned(),
            ..GeneralSettings::default()
        };
        self.register(SectionDefinition::new(
            "core.general",
            1,
            serde_json::to_value(schemars::schema_for!(GeneralSettings))?,
            serde_json::to_value(general)?,
        )?)?;
        self.register(
            SectionDefinition::typed::<AppearanceSettings>("core.appearance", 2)?
                .migration(1, appearance_v2)?,
        )?;
        self.register(SectionDefinition::typed::<ConsoleSettings>(
            "core.console",
            1,
        )?)?;
        self.register(SectionDefinition::typed::<EmulationSettings>(
            "core.emulation",
            1,
        )?)?;
        self.register(SectionDefinition::typed::<LauncherSettings>(
            "core.launchers",
            1,
        )?)?;
        self.register(SectionDefinition::typed::<DiscoverySettings>(
            "core.discovery",
            1,
        )?)?;
        self.register(SectionDefinition::typed::<StorageSettings>(
            "core.storage",
            1,
        )?)?;
        self.register(
            SectionDefinition::typed::<InstallerSettings>("core.installer", 3)?
                .migration(1, |previous| {
                    if previous != serde_json::json!({}) {
                        return Err(super::SettingsError::Stored);
                    }
                    Ok(serde_json::to_value(InstallerSettings::default())?)
                })?
                // Downloads from a link ask before running an installer (CSK-02).
                .migration(2, |mut previous| {
                    if let Some(value) = previous.as_object_mut() {
                        value
                            .entry("downloadExecution")
                            .or_insert_with(|| serde_json::json!("ask"));
                    }
                    Ok(previous)
                })?,
        )?;
        self.register(SectionDefinition::typed::<UpdateSettings>(
            "core.updates",
            1,
        )?)?;
        self.register(SectionDefinition::typed::<AppUpdateSettings>(
            "core.appUpdates",
            1,
        )?)?;
        self.register(SectionDefinition::typed::<StreamingSettings>(
            "core.streaming",
            1,
        )?)?;
        self.register(SectionDefinition::typed::<OnboardingSettings>(
            "core.onboarding",
            1,
        )?)?;
        self.register(SectionDefinition::typed::<MetadataSettings>(
            "core.metadata",
            1,
        )?)?;
        self.register(SectionDefinition::typed::<SavesSettings>("core.saves", 1)?)?;
        self.register(SectionDefinition::typed::<crate::ports::PortsSettings>(
            "core.ports",
            1,
        )?)?;
        self.register(
            SectionDefinition::typed::<crate::sync::policy::SyncSettings>("core.sync", 1)?,
        )?;
        self.register(SectionDefinition::typed::<AccountsSettings>(
            "core.accounts",
            1,
        )?)?;
        Ok(())
    }
}
