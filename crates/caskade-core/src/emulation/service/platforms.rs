//! Platform settings: defaults, BIOS folders and the profiles that run each
//! platform.

use super::{Availability, EmulationService, EmulatorView, PlatformInfo, platform_info};
use crate::{
    Error, Result,
    emulation::{BiosState, PlatformSettings},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

/// Settings → Emulators header stats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct EmulationOverview {
    pub emulators: u32,
    pub platforms_with_default: u32,
    pub roms: u32,
    /// `None` until BIOS status (#122) can tell.
    pub bios_issues: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct CompatibleProfile {
    pub emulator_id: String,
    pub profile_id: String,
    pub emulator_name: String,
    pub profile_name: String,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlatformView {
    pub platform: PlatformInfo,
    pub settings: PlatformSettings,
    pub rom_count: u32,
    pub compatible: Vec<CompatibleProfile>,
}

impl EmulationService {
    pub async fn platforms(&self) -> Result<Vec<PlatformView>> {
        let catalog = self.catalog();
        let emulators = self.emulators().await?;
        let mut settings: BTreeMap<String, PlatformSettings> = self
            .inner
            .store
            .platform_settings()
            .await?
            .into_iter()
            .map(|settings| (settings.platform.clone(), settings))
            .collect();
        let counts = self.inner.store.emulated_rom_counts().await?;
        Ok(catalog
            .platforms()
            .map(|platform| {
                let settings = settings.remove(&platform.id).unwrap_or(PlatformSettings {
                    platform: platform.id.clone(),
                    default: None,
                    bios_dir: None,
                });
                let mut info = platform_info(platform);
                // BIOS state as the platform's default emulator sees it.
                if let Some(profile) = settings.default.as_ref().and_then(|default| {
                    emulators
                        .iter()
                        .filter(|view| view.emulator.id == default.emulator_id)
                        .flat_map(|view| &view.profiles)
                        .find(|profile| profile.id == default.profile_id)
                }) {
                    for entry in &mut info.bios {
                        if let Some(checked) = profile
                            .bios
                            .iter()
                            .find(|checked| checked.file == entry.file)
                        {
                            *entry = checked.clone();
                        }
                    }
                }
                PlatformView {
                    platform: info,
                    settings,
                    rom_count: counts.get(&platform.id).copied().unwrap_or(0),
                    compatible: compatible(&emulators, &platform.id),
                }
            })
            .collect())
    }

    /// Sets a platform's default and BIOS folder. The default must be a
    /// profile that runs this platform.
    pub async fn set_platform_settings(&self, settings: PlatformSettings) -> Result<PlatformView> {
        if let Some(default) = &settings.default {
            let emulators = self.emulators().await?;
            let fits = compatible(&emulators, &settings.platform)
                .iter()
                .any(|profile| {
                    profile.emulator_id == default.emulator_id
                        && profile.profile_id == default.profile_id
                });
            if !fits {
                return Err(Error::Invalid(crate::tr!(
                    "that profile doesn't run {platform}",
                    platform = settings.platform
                )));
            }
        }
        self.inner.store.set_platform_settings(&settings).await?;
        self.changed("platforms");
        self.platforms()
            .await?
            .into_iter()
            .find(|view| view.platform.id == settings.platform)
            .ok_or_else(|| {
                Error::Invalid(crate::tr!(
                    "unknown platform {platform}",
                    platform = settings.platform
                ))
            })
    }

    pub async fn overview(&self) -> Result<EmulationOverview> {
        let platforms = self.platforms().await?;
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Ok(EmulationOverview {
            emulators: count(self.inner.store.emulators().await?.len()),
            platforms_with_default: count(
                platforms
                    .iter()
                    .filter(|view| {
                        view.settings.default.as_ref().is_some_and(|default| {
                            view.compatible.iter().any(|profile| {
                                profile.available
                                    && profile.emulator_id == default.emulator_id
                                    && profile.profile_id == default.profile_id
                            })
                        })
                    })
                    .count(),
            ),
            roms: platforms.iter().map(|view| view.rom_count).sum(),
            bios_issues: Some(count(
                platforms
                    .iter()
                    .filter(|view| {
                        view.platform.bios.iter().any(|bios| {
                            bios.required
                                && bios.state.is_some_and(|state| state != BiosState::Found)
                        })
                    })
                    .count(),
            )),
        })
    }
}

/// Configured profiles that run `platform`.
pub(super) fn compatible(emulators: &[EmulatorView], platform: &str) -> Vec<CompatibleProfile> {
    emulators
        .iter()
        .flat_map(|view| {
            view.profiles
                .iter()
                .filter(|profile| profile.platforms.iter().any(|id| id == platform))
                .map(|profile| CompatibleProfile {
                    emulator_id: view.emulator.id.clone(),
                    profile_id: profile.id.clone(),
                    emulator_name: view.emulator.name.clone(),
                    profile_name: profile.name.clone(),
                    available: profile.availability == Availability::Available,
                })
        })
        .collect()
}
