//! An emulated game's emulator settings and launch readiness (§4, #122).

use super::{
    Availability, BiosEntry, CompatibleProfile, EmulationService, PlatformInfo,
    emulators::bios_entry, platform_info, platforms::compatible,
};
use crate::{
    Error, Result,
    bus::Event,
    emulation::{ArgsOverride, BiosState, EmulatorTarget, InstallKind, Vars, check_templates},
    model::{GameEntry, LaunchSpec},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Whether an emulated game can launch, or what to fix first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Readiness {
    Ready,
    /// The game asks for an emulator or profile at launch; one is available.
    Ask,
    /// The platform has no default, or nothing compatible to ask about.
    NoEmulator,
    /// The pinned emulator or profile was deleted.
    ProfileGone,
    /// The emulator is no longer installed.
    NotInstalled,
    /// The profile's required files (cores, …) are missing.
    MissingFiles {
        missing: Vec<String>,
    },
    /// The profile can't be resolved.
    Invalid {
        reason: String,
    },
    /// Required BIOS files are missing or are the wrong dump.
    MissingBios {
        files: Vec<String>,
    },
    /// A ROM file isn't where the library says.
    RomMissing {
        path: String,
    },
    /// Not an emulated game.
    NotEmulated,
}

/// One disc (ROM) of a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscView {
    pub name: Option<String>,
    pub path: String,
    pub exists: bool,
    pub size: Option<u64>,
}

/// An emulated game's emulator settings and launch readiness (§4, #122).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameEmulation {
    pub entry_id: String,
    pub platform_id: String,
    pub platform: Option<PlatformInfo>,
    pub target: EmulatorTarget,
    pub args: ArgsOverride,
    pub discs: Vec<DiscView>,
    /// What the target resolves to now, unless it asks at launch.
    pub profile: Option<CompatibleProfile>,
    pub compatible: Vec<CompatibleProfile>,
    pub bios: Vec<BiosEntry>,
    pub readiness: Readiness,
    /// What to show instead of Play, e.g. "No emulator for SNES — configure".
    pub message: Option<String>,
}

impl EmulationService {
    /// An emulated library entry's settings and readiness.
    pub async fn game(&self, entry_id: &str) -> Result<GameEmulation> {
        let entry = self.inner.store.get_entry(entry_id).await?.ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "unknown library entry {entry}",
                entry = entry_id
            ))
        })?;
        self.game_of(&entry).await
    }

    /// Whether `entry` can launch now.
    pub async fn readiness(&self, entry: &GameEntry) -> Result<Readiness> {
        Ok(self.game_of(entry).await?.readiness)
    }

    async fn game_of(&self, entry: &GameEntry) -> Result<GameEmulation> {
        let Some(LaunchSpec::Emulator {
            platform_id,
            target,
            roms,
            args,
        }) = &entry.launch
        else {
            return Ok(GameEmulation {
                entry_id: entry.id.clone(),
                platform_id: String::new(),
                platform: None,
                target: Default::default(),
                args: Default::default(),
                discs: Vec::new(),
                profile: None,
                compatible: Vec::new(),
                bios: Vec::new(),
                readiness: Readiness::NotEmulated,
                message: None,
            });
        };
        let catalog = self.catalog();
        let platform = catalog.platform(platform_id).map(platform_info);
        let short = platform
            .as_ref()
            .map_or(platform_id.clone(), |platform| platform.short.clone());
        let mut discs = Vec::new();
        for rom in roms {
            let path = rom.path(&self.inner.store).await?;
            discs.push(DiscView {
                name: rom.name.clone(),
                exists: path.is_file(),
                path: path.to_string_lossy().into_owned(),
                size: rom.size,
            });
        }
        let emulators = self.emulators().await?;
        let compatible = compatible(&emulators, platform_id);
        let default = self
            .inner
            .store
            .get_platform_settings(platform_id)
            .await?
            .and_then(|settings| settings.default);
        let chosen = match target {
            EmulatorTarget::PlatformDefault => {
                default.map(|default| (default.emulator_id, default.profile_id))
            }
            EmulatorTarget::Profile {
                emulator_id,
                profile_id,
            } => Some((emulator_id.clone(), profile_id.clone())),
            EmulatorTarget::AskProfile { .. } | EmulatorTarget::AskEmulator => None,
        };
        let profile_view = chosen.as_ref().and_then(|(emulator_id, profile_id)| {
            emulators
                .iter()
                .find(|view| view.emulator.id == *emulator_id)
                .and_then(|view| {
                    view.profiles
                        .iter()
                        .find(|profile| profile.id == *profile_id)
                        .map(|profile| (view, profile))
                })
        });
        let profile = profile_view.map(|(view, profile)| CompatibleProfile {
            emulator_id: view.emulator.id.clone(),
            profile_id: profile.id.clone(),
            emulator_name: view.emulator.name.clone(),
            profile_name: profile.name.clone(),
            available: profile.availability == Availability::Available,
        });
        let bios = profile_view
            .map(|(_, profile)| profile.bios.clone())
            .unwrap_or_else(|| {
                self.bios(&catalog, std::slice::from_ref(platform_id), &[])
                    .iter()
                    .map(|(platform, bios)| BiosEntry {
                        platform: platform.clone(),
                        ..bios_entry(bios)
                    })
                    .collect()
            });

        // A RomM game's ROM downloads at launch (#1).
        let readiness = if let Some(missing) = discs.iter().find(|disc| !disc.exists)
            && !crate::romm::is_romm(entry)
        {
            Readiness::RomMissing {
                path: missing.path.clone(),
            }
        } else {
            match (target, profile_view) {
                (EmulatorTarget::AskEmulator, _) => {
                    if compatible.iter().any(|profile| profile.available) {
                        Readiness::Ask
                    } else {
                        Readiness::NoEmulator
                    }
                }
                (EmulatorTarget::AskProfile { emulator_id }, _) => {
                    if compatible
                        .iter()
                        .any(|profile| profile.available && profile.emulator_id == *emulator_id)
                    {
                        Readiness::Ask
                    } else if emulators
                        .iter()
                        .any(|view| view.emulator.id == *emulator_id)
                    {
                        Readiness::NoEmulator
                    } else {
                        Readiness::ProfileGone
                    }
                }
                (EmulatorTarget::PlatformDefault, None) if chosen.is_none() => {
                    Readiness::NoEmulator
                }
                (_, None) => Readiness::ProfileGone,
                (_, Some((_, profile))) => match &profile.availability {
                    Availability::NotInstalled => Readiness::NotInstalled,
                    Availability::MissingFiles { missing } => Readiness::MissingFiles {
                        missing: missing.clone(),
                    },
                    Availability::Invalid { reason } => Readiness::Invalid {
                        reason: reason.clone(),
                    },
                    Availability::Available => {
                        let files: Vec<String> = profile
                            .bios
                            .iter()
                            .filter(|bios| {
                                bios.required
                                    && bios.state.is_some_and(|state| state != BiosState::Found)
                            })
                            .map(|bios| bios.file.clone())
                            .collect();
                        if files.is_empty() {
                            Readiness::Ready
                        } else {
                            Readiness::MissingBios { files }
                        }
                    }
                },
            }
        };
        let emulator_name = profile
            .as_ref()
            .map(|profile| profile.emulator_name.clone());
        let message = match &readiness {
            Readiness::Ready | Readiness::Ask | Readiness::NotEmulated => None,
            Readiness::NoEmulator => Some(crate::tr!(
                "No emulator for {platform} — configure",
                platform = short
            )),
            Readiness::ProfileGone => Some(crate::tr!(
                "The emulator this game used is gone — choose another for {platform}",
                platform = short
            )),
            Readiness::NotInstalled => Some(match emulator_name {
                Some(emulator) => crate::tr!(
                    "{emulator} isn't installed any more — configure",
                    emulator = emulator
                ),
                None => crate::tr!("The emulator isn't installed any more — configure"),
            }),
            Readiness::MissingFiles { .. } => Some(match emulator_name {
                Some(emulator) => crate::tr!(
                    "{emulator} is missing files it needs — configure",
                    emulator = emulator
                ),
                None => crate::tr!("The emulator is missing files it needs — configure"),
            }),
            Readiness::Invalid { reason } => Some(reason.clone()),
            Readiness::MissingBios { files } => Some(crate::tr!(
                "Missing BIOS for {platform}: {files}",
                platform = short,
                files = files.join(", ")
            )),
            Readiness::RomMissing { path } => {
                Some(crate::tr!("The ROM is missing: {path}", path = path))
            }
        };
        Ok(GameEmulation {
            entry_id: entry.id.clone(),
            platform_id: platform_id.clone(),
            platform,
            target: target.clone(),
            args: args.clone(),
            discs,
            profile,
            compatible,
            bios,
            readiness,
            message,
        })
    }

    /// Changes what an emulated game launches with and its extra arguments.
    /// A pinned profile must run the game's platform.
    pub async fn set_game(
        &self,
        entry_id: &str,
        target: EmulatorTarget,
        args: ArgsOverride,
    ) -> Result<GameEmulation> {
        let mut entry = self.inner.store.get_entry(entry_id).await?.ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "unknown library entry {entry}",
                entry = entry_id
            ))
        })?;
        let Some(LaunchSpec::Emulator {
            platform_id,
            target: current,
            args: current_args,
            ..
        }) = &mut entry.launch
        else {
            return Err(Error::Invalid(crate::tr!("not an emulated game")));
        };
        let emulators = self.emulators().await?;
        let fits = compatible(&emulators, platform_id);
        match &target {
            EmulatorTarget::Profile {
                emulator_id,
                profile_id,
            } if !fits.iter().any(|profile| {
                profile.emulator_id == *emulator_id && profile.profile_id == *profile_id
            }) =>
            {
                return Err(Error::Invalid(crate::tr!(
                    "that profile doesn't run {platform}",
                    platform = platform_id
                )));
            }
            EmulatorTarget::AskProfile { emulator_id }
                if !fits
                    .iter()
                    .any(|profile| profile.emulator_id == *emulator_id) =>
            {
                return Err(Error::Invalid(crate::tr!(
                    "that emulator has no profile for {platform}",
                    platform = platform_id
                )));
            }
            _ => {}
        }
        // Per-game arguments know only the built-in variables.
        check_templates(
            Vars::NONE,
            InstallKind::Folder,
            args.args().iter().map(String::as_str),
        )?;
        *current = target;
        *current_args = args;
        let entry = self.inner.store.upsert_entry(&entry).await?;
        self.inner.bus.emit(Event::LibraryGameUpdated {
            game_id: entry.game_id.clone(),
        });
        self.game_of(&entry).await
    }
}
