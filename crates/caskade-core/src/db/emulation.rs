//! Emulator, platform settings and ROM scanner repositories (#112,
//! EMULATION-PLAN §4). Built-in profile checks that need definitions belong
//! to the emulation service; the store checks everything else.

use super::{LibraryStore, decode};
use crate::{
    Error, Result,
    emulation::{
        Emulator, EmulatorInstall, EmulatorProfile, PlatformSettings, ProfileRef, RomScanner,
    },
    model::validation as v,
};

fn install_kind(install: &EmulatorInstall) -> &'static str {
    match install {
        EmulatorInstall::Folder { .. } => "folder",
        EmulatorInstall::Path { .. } => "path",
        EmulatorInstall::Flatpak { .. } => "flatpak",
        EmulatorInstall::AppImage { .. } => "app_image",
        EmulatorInstall::AppBundle { .. } => "app_bundle",
    }
}

impl LibraryStore {
    /// Saves an emulator and its profiles in order. Profiles missing from
    /// `emulator` are deleted, unless a ROM scanner still uses one.
    pub async fn upsert_emulator(&self, emulator: &Emulator) -> Result<Emulator> {
        emulator.validate(None)?;
        let ids: Vec<&str> = emulator.profiles.iter().map(EmulatorProfile::id).collect();
        let ids_json = v::json(&ids)?;
        let mut tx = self.pool.begin().await?;
        let used: Option<String> = sqlx::query_scalar(
            "SELECT s.name FROM rom_scanners s WHERE s.emulator_id=? AND s.profile_id NOT IN (SELECT value FROM json_each(?)) LIMIT 1",
        )
        .bind(&emulator.id)
        .bind(&ids_json)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(scanner) = used {
            return Err(Error::Invalid(crate::tr!(
                "the ROM scanner {scanner} uses a profile being removed",
                scanner = format!("{scanner:?}")
            )));
        }
        let record = Emulator {
            profiles: Vec::new(),
            ..emulator.clone()
        };
        sqlx::query("INSERT INTO emulators(id,name,definition_id,install_kind,record_json) VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,definition_id=excluded.definition_id,install_kind=excluded.install_kind,record_json=excluded.record_json")
            .bind(&emulator.id)
            .bind(&emulator.name)
            .bind(&emulator.definition)
            .bind(install_kind(&emulator.install))
            .bind(v::json(&record)?)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM emulator_profiles WHERE emulator_id=? AND id NOT IN (SELECT value FROM json_each(?))",
        )
        .bind(&emulator.id)
        .bind(&ids_json)
        .execute(&mut *tx)
        .await?;
        for (position, profile) in emulator.profiles.iter().enumerate() {
            let (kind, definition_profile) = match profile {
                EmulatorProfile::BuiltIn {
                    definition_profile, ..
                } => ("built_in", Some(definition_profile.as_str())),
                EmulatorProfile::Custom { .. } => ("custom", None),
            };
            // A profile id never moves to another emulator.
            let written = sqlx::query("INSERT INTO emulator_profiles(id,emulator_id,position,kind,definition_profile,record_json) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET position=excluded.position,kind=excluded.kind,definition_profile=excluded.definition_profile,record_json=excluded.record_json WHERE emulator_profiles.emulator_id=excluded.emulator_id")
                .bind(profile.id())
                .bind(&emulator.id)
                .bind(position as i64)
                .bind(kind)
                .bind(definition_profile)
                .bind(v::json(profile)?)
                .execute(&mut *tx)
                .await?;
            if written.rows_affected() != 1 {
                return Err(Error::Invalid(crate::tr!(
                    "profile {profile} belongs to another emulator",
                    profile = profile.id()
                )));
            }
        }
        tx.commit().await?;
        Ok(emulator.clone())
    }

    pub async fn get_emulator(&self, id: &str) -> Result<Option<Emulator>> {
        let Some(record) =
            sqlx::query_scalar::<_, String>("SELECT record_json FROM emulators WHERE id=?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
        else {
            return Ok(None);
        };
        let mut emulator: Emulator = decode(record)?;
        emulator.profiles = self.profiles_of(id).await?;
        Ok(Some(emulator))
    }

    /// Every emulator with its profiles, by name.
    pub async fn emulators(&self) -> Result<Vec<Emulator>> {
        let records = sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM emulators ORDER BY name COLLATE NOCASE,id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut emulators = Vec::with_capacity(records.len());
        for record in records {
            let mut emulator: Emulator = decode(record)?;
            emulator.profiles = self.profiles_of(&emulator.id).await?;
            emulators.push(emulator);
        }
        Ok(emulators)
    }

    async fn profiles_of(&self, emulator_id: &str) -> Result<Vec<EmulatorProfile>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM emulator_profiles WHERE emulator_id=? ORDER BY position,id",
        )
        .bind(emulator_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }

    /// Deletes an emulator and its profiles; platforms using one of them
    /// lose their default. Refused while a ROM scanner uses the emulator.
    pub async fn delete_emulator(&self, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let used: Option<String> =
            sqlx::query_scalar("SELECT name FROM rom_scanners WHERE emulator_id=? LIMIT 1")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(scanner) = used {
            return Err(Error::Invalid(crate::tr!(
                "the ROM scanner {scanner} uses this emulator",
                scanner = format!("{scanner:?}")
            )));
        }
        let deleted = sqlx::query("DELETE FROM emulators WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
        tx.commit().await?;
        Ok(deleted)
    }

    pub async fn set_platform_settings(
        &self,
        settings: &PlatformSettings,
    ) -> Result<PlatformSettings> {
        settings.validate()?;
        let (emulator_id, profile_id) = settings
            .default
            .as_ref()
            .map(|default| (default.emulator_id.as_str(), default.profile_id.as_str()))
            .unzip();
        sqlx::query("INSERT INTO platform_settings(platform_id,emulator_id,profile_id,record_json) VALUES(?,?,?,?) ON CONFLICT(platform_id) DO UPDATE SET emulator_id=excluded.emulator_id,profile_id=excluded.profile_id,record_json=excluded.record_json")
            .bind(&settings.platform)
            .bind(emulator_id)
            .bind(profile_id)
            .bind(v::json(settings)?)
            .execute(&self.pool)
            .await
            .map_err(|error| match &error {
                sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
                    Error::Invalid(crate::tr!("the default profile does not exist"))
                }
                _ => error.into(),
            })?;
        Ok(settings.clone())
    }

    pub async fn get_platform_settings(&self, platform: &str) -> Result<Option<PlatformSettings>> {
        sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT record_json,emulator_id,profile_id FROM platform_settings WHERE platform_id=?",
        )
        .bind(platform)
        .fetch_optional(&self.pool)
        .await?
        .map(platform_settings)
        .transpose()
    }

    pub async fn platform_settings(&self) -> Result<Vec<PlatformSettings>> {
        sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT record_json,emulator_id,profile_id FROM platform_settings ORDER BY platform_id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(platform_settings)
        .collect()
    }

    pub async fn upsert_scanner(&self, scanner: &RomScanner) -> Result<RomScanner> {
        scanner.validate()?;
        sqlx::query("INSERT INTO rom_scanners(id,name,folder,platform_id,emulator_id,profile_id,in_library_update,record_json) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,folder=excluded.folder,platform_id=excluded.platform_id,emulator_id=excluded.emulator_id,profile_id=excluded.profile_id,in_library_update=excluded.in_library_update,record_json=excluded.record_json")
            .bind(&scanner.id)
            .bind(&scanner.name)
            .bind(v::path_text(&scanner.folder)?)
            .bind(&scanner.platform)
            .bind(&scanner.profile.emulator_id)
            .bind(&scanner.profile.profile_id)
            .bind(scanner.in_library_update)
            .bind(v::json(scanner)?)
            .execute(&self.pool)
            .await
            .map_err(|error| match &error {
                sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
                    Error::Invalid(crate::tr!("the scanner's profile does not exist"))
                }
                _ => error.into(),
            })?;
        Ok(scanner.clone())
    }

    pub async fn get_scanner(&self, id: &str) -> Result<Option<RomScanner>> {
        sqlx::query_scalar::<_, String>("SELECT record_json FROM rom_scanners WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(decode)
            .transpose()
    }

    pub async fn scanners(&self) -> Result<Vec<RomScanner>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM rom_scanners ORDER BY name COLLATE NOCASE,id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }

    /// Emulated library entries per platform id.
    pub async fn emulated_rom_counts(&self) -> Result<std::collections::BTreeMap<String, u32>> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT json_extract(record_json,'$.launch.platformId') AS platform,count(*) FROM game_entries WHERE launch_kind='emulator' AND platform IS NOT NULL GROUP BY platform",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(platform, count)| (platform, u32::try_from(count).unwrap_or(u32::MAX)))
            .collect())
    }

    pub async fn delete_scanner(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM rom_scanners WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }
}

/// The columns are authoritative for the default: deleting its profile
/// clears them, not the JSON.
fn platform_settings(
    (record, emulator_id, profile_id): (String, Option<String>, Option<String>),
) -> Result<PlatformSettings> {
    let mut settings: PlatformSettings = decode(record)?;
    settings.default = emulator_id
        .zip(profile_id)
        .map(|(emulator_id, profile_id)| ProfileRef {
            emulator_id,
            profile_id,
        });
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        emulation::{ArgsOverride, EmulatorTarget, GameRom, ProfileHooks, Tracking},
        model::{Game, GameEntry, InstallLocation, LaunchSpec},
    };
    use std::path::PathBuf;

    fn emulator() -> Emulator {
        Emulator {
            id: "ra".into(),
            name: "RetroArch".into(),
            definition: Some("retroarch".into()),
            install: EmulatorInstall::Flatpak {
                app_id: "org.libretro.RetroArch".into(),
            },
            profiles: vec![
                EmulatorProfile::BuiltIn {
                    id: "snes".into(),
                    definition_profile: "Snes9x".into(),
                    args: ArgsOverride::Append {
                        args: vec!["--verbose".into()],
                    },
                    hooks: ProfileHooks {
                        before: Some(vec!["/usr/bin/true".into()]),
                        after_exit: None,
                    },
                },
                EmulatorProfile::Custom {
                    id: "psx".into(),
                    name: "Beetle via core path".into(),
                    executable: None,
                    args: vec!["-L".into(), "/cores/psx.so".into(), "{RomPath}".into()],
                    working_dir: Some("{RomDir}".into()),
                    platforms: vec!["sony_playstation".into()],
                    extensions: vec!["cue".into(), "<none>".into()],
                    env: vec![("SDL_VIDEODRIVER".into(), "wayland".into())],
                    tracking: Tracking::ProcessName {
                        name: "retroarch".into(),
                    },
                    hooks: ProfileHooks::default(),
                },
            ],
        }
    }

    fn scanner() -> RomScanner {
        RomScanner {
            id: "snes-roms".into(),
            name: "SNES".into(),
            folder: PathBuf::from("/roms/snes"),
            platform: Some("nintendo_snes".into()),
            profile: ProfileRef {
                emulator_id: "ra".into(),
                profile_id: "snes".into(),
            },
            launch_with: EmulatorTarget::PlatformDefault,
            recursive: true,
            scan_archives: false,
            skip_cloud_placeholders: true,
            exclusions: vec!["bios/".into(), ">*.txt".into()],
            in_library_update: true,
        }
    }

    #[tokio::test]
    async fn emulators_settings_and_scanners_round_trip() {
        let store = LibraryStore::in_memory().await.unwrap();
        let ra = emulator();
        store.upsert_emulator(&ra).await.unwrap();
        assert_eq!(store.get_emulator("ra").await.unwrap(), Some(ra.clone()));

        // Reordering and editing keep the same rows.
        let mut edited = ra.clone();
        edited.profiles.reverse();
        edited.name = "RetroArch (Flatpak)".into();
        store.upsert_emulator(&edited).await.unwrap();
        assert_eq!(store.emulators().await.unwrap(), vec![edited.clone()]);

        let settings = PlatformSettings {
            platform: "nintendo_snes".into(),
            default: Some(ProfileRef {
                emulator_id: "ra".into(),
                profile_id: "snes".into(),
            }),
            bios_dir: Some(PathBuf::from("/bios")),
        };
        store.set_platform_settings(&settings).await.unwrap();
        assert_eq!(
            store.platform_settings().await.unwrap(),
            vec![settings.clone()]
        );
        store.upsert_scanner(&scanner()).await.unwrap();
        assert_eq!(store.scanners().await.unwrap(), vec![scanner()]);
        assert_eq!(
            store.get_scanner("snes-roms").await.unwrap(),
            Some(scanner())
        );
    }

    #[tokio::test]
    async fn references_to_profiles_are_kept_consistent() {
        let store = LibraryStore::in_memory().await.unwrap();
        store.upsert_emulator(&emulator()).await.unwrap();
        let missing = PlatformSettings {
            platform: "sony_playstation".into(),
            default: Some(ProfileRef {
                emulator_id: "ra".into(),
                profile_id: "nope".into(),
            }),
            bios_dir: None,
        };
        assert!(store.set_platform_settings(&missing).await.is_err());
        let psx = PlatformSettings {
            default: Some(ProfileRef {
                emulator_id: "ra".into(),
                profile_id: "psx".into(),
            }),
            ..missing
        };
        store.set_platform_settings(&psx).await.unwrap();
        store.upsert_scanner(&scanner()).await.unwrap();

        // A scanner's profile can't be removed, nor its emulator deleted.
        let mut without_snes = emulator();
        without_snes
            .profiles
            .retain(|profile| profile.id() != "snes");
        assert!(store.upsert_emulator(&without_snes).await.is_err());
        assert!(store.delete_emulator("ra").await.is_err());
        assert_eq!(store.get_emulator("ra").await.unwrap(), Some(emulator()));

        // A default's profile can; the platform loses its default.
        let mut without_psx = emulator();
        without_psx.profiles.retain(|profile| profile.id() != "psx");
        store.upsert_emulator(&without_psx).await.unwrap();
        let settings = store
            .get_platform_settings("sony_playstation")
            .await
            .unwrap();
        assert_eq!(settings.unwrap().default, None);

        // Profile ids belong to one emulator.
        let mut thief = emulator();
        thief.id = "other".into();
        assert!(store.upsert_emulator(&thief).await.is_err());

        store.delete_scanner("snes-roms").await.unwrap();
        assert!(store.delete_emulator("ra").await.unwrap());
        assert!(store.emulators().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn invalid_configuration_is_refused() {
        let store = LibraryStore::in_memory().await.unwrap();
        let mut bad = emulator();
        bad.install = EmulatorInstall::AppImage {
            file: PathBuf::from("relative.AppImage"),
        };
        assert!(store.upsert_emulator(&bad).await.is_err());
        let mut bad = emulator();
        if let EmulatorProfile::Custom { args, .. } = &mut bad.profiles[1] {
            args.push("{cores}".into());
        }
        assert!(
            store.upsert_emulator(&bad).await.is_err(),
            "unknown variable"
        );
        let mut bad = scanner();
        bad.folder = PathBuf::from("roms");
        assert!(store.upsert_scanner(&bad).await.is_err());
    }

    /// #51's "needs emulator setup" ROM entries become installed emulated
    /// games on Caskade's platform ids (#120).
    #[tokio::test]
    async fn rom_placeholders_from_the_installer_become_emulated_games() {
        let store = LibraryStore::in_memory().await.unwrap();
        let game = store
            .upsert_game(&Game::new("Mario Kart 64"))
            .await
            .unwrap();
        let mut entry = GameEntry::new(&game.id, crate::install::HANDOFF_SOURCE, "rom-64");
        entry.location = Some(InstallLocation::External {
            path: PathBuf::from("/roms"),
        });
        entry.launch = Some(LaunchSpec::Emulator {
            platform_id: "nintendo_64".into(),
            target: EmulatorTarget::PlatformDefault,
            roms: vec![GameRom::absolute("/roms/mk64.z64")],
            args: ArgsOverride::Default,
        });
        entry.state = caskade_discovery::InstallState::Broken {
            reason: crate::install::NEEDS_EMULATOR.into(),
        };
        let entry = store.upsert_entry(&entry).await.unwrap();
        super::super::MIGRATOR.undo(&store.pool, 2).await.unwrap();
        super::super::MIGRATOR.run(&store.pool).await.unwrap();
        let migrated = store.get_entry(&entry.id).await.unwrap().unwrap();
        assert_eq!(migrated.state, caskade_discovery::InstallState::Installed);
        assert!(matches!(
            &migrated.launch,
            Some(LaunchSpec::Emulator { platform_id, .. }) if platform_id == "nintendo_n64"
        ));
        super::super::MIGRATOR.undo(&store.pool, 2).await.unwrap();
        let state: String = sqlx::query_scalar("SELECT state_kind FROM game_entries WHERE id=?")
            .bind(&entry.id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(state, "broken");
    }

    /// Emulator launches stored before #112 move to the new shape and back.
    #[tokio::test]
    async fn emulator_launches_migrate_to_the_new_shape() {
        let store = LibraryStore::in_memory().await.unwrap();
        let game = store
            .upsert_game(&Game::new("Chrono Trigger"))
            .await
            .unwrap();
        let mut entry = GameEntry::new(&game.id, crate::install::HANDOFF_SOURCE, "rom-1");
        entry.location = Some(InstallLocation::External {
            path: PathBuf::from("/roms/snes"),
        });
        let entry = store.upsert_entry(&entry).await.unwrap();

        super::super::MIGRATOR.undo(&store.pool, 1).await.unwrap();
        let old = serde_json::json!({
            "type": "emulator",
            "platformId": "snes",
            "profile": {"type": "default"},
            "roms": ["Chrono Trigger (USA).sfc", "/elsewhere/disc2.sfc"],
            "args": ["--fullscreen"],
        });
        sqlx::query("UPDATE game_entries SET launch_kind='emulator',record_json=json_set(record_json,'$.launch',json(?)) WHERE id=?")
            .bind(old.to_string())
            .bind(&entry.id)
            .execute(&store.pool)
            .await
            .unwrap();
        super::super::MIGRATOR.run(&store.pool).await.unwrap();

        let migrated = store.get_entry(&entry.id).await.unwrap().unwrap();
        assert_eq!(
            migrated.launch,
            Some(LaunchSpec::Emulator {
                platform_id: "snes".into(),
                target: EmulatorTarget::PlatformDefault,
                roms: vec![
                    GameRom::absolute("/roms/snes/Chrono Trigger (USA).sfc"),
                    GameRom::absolute("/elsewhere/disc2.sfc"),
                ],
                args: ArgsOverride::Append {
                    args: vec!["--fullscreen".into()],
                },
            })
        );
        // The migrated entry saves again under the new validation.
        store.upsert_entry(&migrated).await.unwrap();

        // And the down migration restores the old shape.
        super::super::MIGRATOR.undo(&store.pool, 1).await.unwrap();
        let launch: String = sqlx::query_scalar(
            "SELECT json_extract(record_json,'$.launch') FROM game_entries WHERE id=?",
        )
        .bind(&entry.id)
        .fetch_one(&store.pool)
        .await
        .unwrap();
        let launch: serde_json::Value = serde_json::from_str(&launch).unwrap();
        assert_eq!(launch["profile"], serde_json::json!({"type": "default"}));
        assert_eq!(launch["args"], serde_json::json!(["--fullscreen"]));
        assert_eq!(
            launch["roms"],
            serde_json::json!([
                "/roms/snes/Chrono Trigger (USA).sfc",
                "/elsewhere/disc2.sfc"
            ])
        );
    }
}
