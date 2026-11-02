use caskade_core::{
    ludusavi::Entry,
    model::{GameEntry, GameRuntime, SourceId},
    saves::{Context, LocationKind, expand, expand_glob, scan_path},
};
use caskade_discovery::{HostEnv, OperatingSystem, Prefix};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn write(path: impl AsRef<Path>, value: &str) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, value).unwrap();
}

fn host(dir: &Path) -> HostEnv {
    fs::create_dir_all(dir.join("root/home/player")).unwrap();
    write(
        dir.join("machine.toml"),
        "os = 'linux'\n[env]\nHOME = '/home/player'\nUSER = 'player'\n",
    );
    HostEnv::fixture(dir).unwrap()
}

fn prefix(root: &Path) -> PathBuf {
    let prefix = root.join("steam/steamapps/compatdata/620/pfx");
    fs::create_dir_all(prefix.join("drive_c/users/steamuser/AppData")).unwrap();
    fs::create_dir_all(prefix.join("drive_c/ProgramData")).unwrap();
    write(
        prefix.join("user.reg"),
        "WINE REGISTRY Version 2\n#arch=win64\n\n[Software\\\\Fixture] 1\n\"Progress\"=dword:0000002a\n\n[Software\\\\Fixture\\\\Slots] 1\n\"Name\"=\"Slot one\"\n\n[Software\\\\Other] 1\n\"Password\"=\"not-a-game-save\"\n",
    );
    write(
        prefix.join("system.reg"),
        "WINE REGISTRY Version 2\n#arch=win64\n\n[Software\\\\Fixture] 1\n\"Setting\"=\"Enabled\"\n",
    );
    prefix
}

fn pure_context(profile: &str) -> Context {
    let windows = profile == "windows";
    let mut context = Context::new(if windows {
        OperatingSystem::Windows
    } else {
        OperatingSystem::Linux
    });
    let home = if windows {
        "C:/Users/player"
    } else {
        "/home/player"
    };
    context.values = BTreeMap::from([
        (
            "base".into(),
            if windows {
                "D:/Steam/steamapps/common/FixtureGame"
            } else {
                "/games/FixtureGame"
            }
            .into(),
        ),
        (
            "root".into(),
            if windows { "D:/Steam" } else { "/games" }.into(),
        ),
        ("game".into(), "FixtureGame".into()),
        ("home".into(), home.into()),
        ("osUserName".into(), "player".into()),
        ("storeUserId".into(), "*".into()),
        ("storeGameId".into(), "620".into()),
    ]);
    if windows {
        for (name, value) in [
            ("winAppData", "C:/Users/player/AppData/Roaming"),
            ("winLocalAppData", "C:/Users/player/AppData/Local"),
            ("winLocalAppDataLow", "C:/Users/player/AppData/LocalLow"),
            ("winDocuments", "C:/Users/player/Documents"),
            ("winPublic", "C:/Users/Public"),
            ("winProgramData", "C:/ProgramData"),
            ("winDir", "C:/Windows"),
        ] {
            context.values.insert(name.into(), value.into());
        }
    } else {
        context
            .values
            .insert("xdgData".into(), format!("{home}/.local/share"));
        context
            .values
            .insert("xdgConfig".into(), format!("{home}/.config"));
    }
    context
}

#[derive(Deserialize)]
struct Parity {
    cases: Vec<ParityCase>,
}
#[derive(Deserialize)]
struct ParityCase {
    profile: String,
    pattern: String,
    expected: Vec<String>,
}

#[test]
fn windows_linux_and_proton_patterns_match_ludusavi_reference_fixtures() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path());
    let root = host.fs_root();
    let prefix = prefix(root);
    let base = root.join("steam/steamapps/common/FixtureGame");
    fs::create_dir_all(&base).unwrap();
    let mut entry = GameEntry::new("g", SourceId::new("steam"), "steam:620");
    entry.runtime = GameRuntime::Windows {
        prefix: Some(Prefix {
            path: prefix.clone(),
            arch: None,
            owner: None,
        }),
        runner: None,
    };
    let mut proton = Context::for_release(&host, &entry, &base, &root.join("steam"));
    proton.values.insert("storeGameId".into(), "620".into());
    assert!(proton.warnings.is_empty(), "{:?}", proton.warnings);
    let cases: Parity =
        serde_json::from_str(include_str!("fixtures/saves/ludusavi-paths.json")).unwrap();
    for case in cases.cases {
        let context = if case.profile == "proton" {
            proton.clone()
        } else {
            pure_context(&case.profile)
        };
        let expected: Vec<_> = case
            .expected
            .iter()
            .map(|value| value.replace("@PREFIX@", &prefix.to_string_lossy()))
            .collect();
        assert_eq!(
            expand(&case.pattern, &context).unwrap(),
            expected,
            "{}: {}",
            case.profile,
            case.pattern
        );
    }
}

#[test]
fn unavailable_placeholders_never_turn_into_guessed_or_root_paths() {
    let context = pure_context("linux");
    for pattern in ["<winAppData>/Fixture", "<unknown>/save", "<home/incomplete"] {
        assert!(expand(pattern, &context).is_err());
    }
    assert!(expand_glob("relative/save/*", true).is_err());
}

#[test]
fn globbing_is_case_aware_recursive_and_escapes_literal_home_components() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home[one]");
    write(home.join("Data/save1.dat"), "one");
    write(home.join("Data/nested/save2.dat"), "two");
    write(home.join("Data/slot-a.sav"), "a");
    write(home.join("Data/slot-b.sav"), "b");
    write(home.join("Data/skip.txt"), "text");
    let mut context = pure_context("linux");
    context
        .values
        .insert("home".into(), home.to_string_lossy().into_owned());
    let pattern = expand("<home>/Data/**/save?.dat", &context)
        .unwrap()
        .remove(0);
    let found = expand_glob(&pattern, true).unwrap();
    assert_eq!(
        found,
        [
            home.join("Data/nested/save2.dat"),
            home.join("Data/save1.dat")
        ]
    );
    let pattern = expand("<home>/data/SAVE?.DAT", &context).unwrap().remove(0);
    assert_eq!(
        expand_glob(&pattern, false).unwrap(),
        [home.join("Data/save1.dat")]
    );
    assert!(expand_glob(&pattern, true).unwrap().is_empty());
    let pattern = expand("<home>/Data/slot-{a,b}.sav", &context)
        .unwrap()
        .remove(0);
    assert_eq!(expand_glob(&pattern, true).unwrap().len(), 2);
    let folder = scan_path(&home.join("Data"), vec!["save".into()])
        .unwrap()
        .unwrap();
    assert_eq!(folder.spec.kind, LocationKind::Folder);
    assert_eq!(folder.files.len(), 5);
}

#[cfg(unix)]
#[test]
fn recursive_folders_follow_wine_document_links_without_following_cycles_forever() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path().join("Documents/save.dat"), "save");
    std::os::unix::fs::symlink(dir.path().join("Documents"), dir.path().join("linked")).unwrap();
    std::os::unix::fs::symlink(dir.path(), dir.path().join("Documents/loop")).unwrap();
    let found = scan_path(&dir.path().join("linked"), Vec::new())
        .unwrap()
        .unwrap();
    assert_eq!(found.files.len(), 1);
}

#[test]
fn os_and_store_constraints_are_conjunctive_with_alternatives() {
    let mut context = pure_context("windows");
    context.store = Some("steam".into());
    let entry: Entry = serde_json::from_value(
        serde_json::json!({"when":[{"os":"linux","store":"steam"},{"os":"windows","store":"gog"}]}),
    )
    .unwrap();
    assert!(!context.applies(&entry));
    context.store = Some("gog".into());
    assert!(context.applies(&entry));
    let unknown: Entry =
        serde_json::from_value(serde_json::json!({"when":[{"os":"future-os"}]})).unwrap();
    assert!(!context.applies(&unknown));
}

#[test]
fn prefix_shell_folders_honor_redirected_documents_and_local_low() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path());
    let prefix = prefix(host.fs_root());
    let mut registry = fs::read_to_string(prefix.join("user.reg")).unwrap();
    registry.push_str(
        r#"
[Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders] 1
"Personal"="C:\\Redirected Documents"
"{A520A1A4-1780-4FF6-BD18-167343C5AF16}"="C:\\Redirected Low"
"#,
    );
    write(prefix.join("user.reg"), &registry);
    let mut entry = GameEntry::new("g", SourceId::new("steam"), "steam:620");
    entry.runtime = GameRuntime::Windows {
        prefix: Some(Prefix {
            path: prefix.clone(),
            arch: None,
            owner: None,
        }),
        runner: None,
    };
    let base = host.fs_root().join("games/Fixture");
    let context = Context::for_release(&host, &entry, &base, host.fs_root());
    assert_eq!(
        context.values["winDocuments"],
        prefix
            .join("drive_c/Redirected Documents")
            .to_string_lossy()
    );
    assert_eq!(
        context.values["winLocalAppDataLow"],
        prefix.join("drive_c/Redirected Low").to_string_lossy()
    );
}

#[cfg(feature = "net")]
mod integration {
    use super::*;
    use caskade_core::{
        db::LibraryStore,
        ludusavi::Ludusavi,
        model::{Game, InstallLocation},
        saves::{LocationSpec, MatchMethod, SaveMatch, SavePreferences, Saves},
    };
    use caskade_discovery::registry::RegistryValue;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

    async fn manifest(dir: &Path) -> Ludusavi {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(include_str!("fixtures/saves/manifest.yaml")),
            )
            .mount(&server)
            .await;
        let manifest = Ludusavi::with_url(dir.join("cache"), server.uri());
        manifest.refresh(true).await.unwrap();
        manifest
    }

    async fn install(
        store: &LibraryStore,
        base: &Path,
        title: &str,
        source: &'static str,
        external: &str,
    ) -> GameEntry {
        fs::create_dir_all(base).unwrap();
        let game = store.upsert_game(&Game::new(title)).await.unwrap();
        let mut entry = GameEntry::new(&game.id, SourceId::new(source), external);
        entry.state = caskade_core::model::InstallState::Installed;
        entry.location = Some(InstallLocation::External { path: base.into() });
        store.upsert_entry(&entry).await.unwrap()
    }

    #[tokio::test]
    async fn emulator_saves_follow_pinned_and_default_profiles_and_never_guess_for_ask() {
        use caskade_core::emulation::{
            Catalog, Emulator, EmulatorInstall, EmulatorProfile, EmulatorTarget, GameRom,
            PlatformSettings, ProfileRef,
        };
        let dir = tempfile::tempdir().unwrap();
        let env = host(dir.path());
        let store = LibraryStore::in_memory().await.unwrap();
        let catalog = Catalog::load([
            ("platforms.toml", "[[platform]]\nid='snes'\nname='SNES'\nshort='SNES'\n"),
            ("emulator.toml", "id='retroarch'\nname='RetroArch'\n[saves]\nfiles=['{XdgConfig}/retroarch/saves/**/{RomNameNoExt}.srm']\nstates=['{XdgConfig}/retroarch/states/**/{RomNameNoExt}.state*']\n[[profile]]\nname='SNES'\nplatforms=['snes']\nargs=['{RomPath}']\n"),
        ]).unwrap();
        let emulator = store
            .upsert_emulator(&Emulator {
                id: "emulator".into(),
                name: "RetroArch".into(),
                definition: Some("retroarch".into()),
                install: EmulatorInstall::Folder {
                    dir: env.home().join("tools/RetroArch"),
                    executable: "retroarch".into(),
                },
                profiles: vec![EmulatorProfile::BuiltIn {
                    id: "snes-profile".into(),
                    definition_profile: "SNES".into(),
                    args: Default::default(),
                    hooks: Default::default(),
                }],
            })
            .await
            .unwrap();
        let chosen = ProfileRef {
            emulator_id: emulator.id.clone(),
            profile_id: emulator.profiles[0].id().into(),
        };
        let game = store
            .upsert_game(&Game::new("Steam Fixture"))
            .await
            .unwrap();
        let rom = env.home().join("ROMs/Game[1].sfc");
        write(&rom, "rom");
        write(
            env.home()
                .join(".config/retroarch/saves/Snes9x/Game[1].srm"),
            "save",
        );
        write(
            env.home().join(".config/retroarch/saves/Snes9x/Other.srm"),
            "other",
        );
        write(
            env.home()
                .join(".config/retroarch/states/Snes9x/Game[1].state1"),
            "state",
        );
        let mut entry = GameEntry::new(&game.id, SourceId::new("manual"), "console");
        entry.launch = Some(caskade_core::model::LaunchSpec::Emulator {
            platform_id: "snes".into(),
            target: EmulatorTarget::Profile {
                emulator_id: chosen.emulator_id.clone(),
                profile_id: chosen.profile_id.clone(),
            },
            roms: vec![GameRom::absolute(&rom)],
            args: Default::default(),
        });
        entry = store.upsert_entry(&entry).await.unwrap();
        let saves = Saves::with_environment(store.clone(), manifest(dir.path()).await, env)
            .with_catalog(std::sync::Arc::new(catalog));
        let state = saves.state(&entry.id).await.unwrap();
        assert!(state.warnings.is_empty(), "{:?}", state.warnings);
        assert!(matches!(
            state.matched,
            SaveMatch::Matched {
                method: MatchMethod::Emulator,
                ..
            }
        ));
        assert_eq!(state.locations.len(), 2);
        assert!(
            state
                .locations
                .iter()
                .any(|location| location.tags == ["state"])
        );
        assert!(
            !serde_json::to_string(&state.locations)
                .unwrap()
                .contains("Other.srm")
        );
        store
            .set_platform_settings(&PlatformSettings {
                platform: "snes".into(),
                default: Some(chosen),
                bios_dir: None,
            })
            .await
            .unwrap();
        if let Some(caskade_core::model::LaunchSpec::Emulator { target, .. }) = &mut entry.launch {
            *target = EmulatorTarget::PlatformDefault;
        }
        store.upsert_entry(&entry).await.unwrap();
        assert_eq!(
            saves.state(&entry.id).await.unwrap().locations,
            state.locations
        );
        if let Some(caskade_core::model::LaunchSpec::Emulator { target, .. }) = &mut entry.launch {
            *target = EmulatorTarget::AskEmulator;
        }
        store.upsert_entry(&entry).await.unwrap();
        let state = saves.state(&entry.id).await.unwrap();
        assert_eq!(state.matched, SaveMatch::Unknown);
        assert!(state.locations.is_empty());
    }

    #[tokio::test]
    async fn matching_prefers_steam_then_gog_then_title_and_library_or_manifest_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let env = host(dir.path());
        let store = LibraryStore::in_memory().await.unwrap();
        let manifest = manifest(dir.path()).await;
        let saves = Saves::with_environment(store.clone(), manifest, env.clone());
        let base = env.fs_root().join("games/fixture");
        let mut entry = install(&store, &base, "Title Fixture", "steam", "steam:620").await;
        entry.ids = vec![caskade_core::model::ExternalId::Gog {
            product_id: 1207658924,
        }];
        store.upsert_entry(&entry).await.unwrap();
        assert!(matches!(
            saves.state(&entry.id).await.unwrap().matched,
            SaveMatch::Matched {
                method: MatchMethod::Steam,
                ..
            }
        ));
        let entry = install(&store, &base, "Title Fixture", "gog", "gog:1207658924").await;
        assert!(matches!(
            saves.state(&entry.id).await.unwrap().matched,
            SaveMatch::Matched {
                method: MatchMethod::Gog,
                ..
            }
        ));
        let entry = install(
            &store,
            &base,
            "Older Fixture Title",
            "manual",
            "title-alias",
        )
        .await;
        let state = saves.state(&entry.id).await.unwrap();
        assert!(
            matches!(state.matched, SaveMatch::Matched { ref title, .. } if title == "Title Fixture")
        );
        let entry = install(
            &store,
            &base,
            "Unknown display title",
            "manual",
            "library-alias",
        )
        .await;
        let mut game = store.game(&entry.game_id).await.unwrap().unwrap();
        game.aliases = vec!["Title Fixture".into()];
        store.upsert_game(&game).await.unwrap();
        assert!(matches!(
            saves.state(&entry.id).await.unwrap().matched,
            SaveMatch::Matched {
                method: MatchMethod::Alias,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn recorded_proton_prefix_resolves_files_case_insensitively_and_reads_only_selected_registry_keys()
     {
        let dir = tempfile::tempdir().unwrap();
        let env = host(dir.path());
        let prefix = prefix(env.fs_root());
        write(
            prefix.join("drive_c/users/steamuser/Documents/Fixture/SLOT.SAV"),
            "slot",
        );
        write(
            prefix.join("drive_c/users/steamuser/AppData/Roaming/Fixture/nested/progress.dat"),
            "progress",
        );
        let store = LibraryStore::in_memory().await.unwrap();
        let base = env.fs_root().join("steam/steamapps/common/FixtureGame");
        let mut entry = install(&store, &base, "Unrelated title", "steam", "steam:620").await;
        entry.runtime = GameRuntime::Windows {
            prefix: Some(Prefix {
                path: prefix.clone(),
                arch: None,
                owner: None,
            }),
            runner: None,
        };
        store.upsert_entry(&entry).await.unwrap();
        let saves = Saves::with_environment(store, manifest(dir.path()).await, env);
        let state = saves.state(&entry.id).await.unwrap();
        assert!(state.warnings.is_empty(), "{:?}", state.warnings);
        let files: Vec<_> = state
            .locations
            .iter()
            .flat_map(|location| location.files.iter())
            .collect();
        assert!(files.iter().any(|file| file.ends_with("SLOT.SAV")));
        assert!(files.iter().any(|file| file.ends_with("progress.dat")));
        let registry: Vec<_> = state
            .locations
            .iter()
            .flat_map(|location| &location.registry)
            .collect();
        assert_eq!(registry.len(), 3);
        assert!(
            registry
                .iter()
                .flat_map(|key| &key.values)
                .any(|value| value.value == RegistryValue::Dword(42))
        );
        assert!(
            !serde_json::to_string(&state.locations)
                .unwrap()
                .contains("not-a-game-save")
        );
    }

    #[tokio::test]
    async fn unknown_games_stay_empty_until_user_locations_or_a_manual_match_are_set_and_reset() {
        let dir = tempfile::tempdir().unwrap();
        let env = host(dir.path());
        let path = dir.path().join("library.sqlite");
        let store = LibraryStore::open(&path).await.unwrap();
        let base = env.fs_root().join("games/unknown");
        write(base.join("hand-saves/slot.dat"), "hand");
        let entry = install(&store, &base, "Never in the manifest", "manual", "unknown").await;
        let cached = manifest(dir.path()).await;
        let saves = Saves::with_environment(store.clone(), cached.clone(), env.clone());
        let state = saves.state(&entry.id).await.unwrap();
        assert_eq!(state.matched, SaveMatch::Unknown);
        assert!(state.locations.is_empty());
        let manual = SavePreferences {
            manifest_title: Some("Hand Matched Fixture".into()),
            locations: None,
        };
        let state = saves.edit(&entry.id, manual.clone(), 0).await.unwrap();
        assert!(matches!(
            state.matched,
            SaveMatch::Matched {
                method: MatchMethod::Manual,
                ..
            }
        ));
        assert_eq!(state.locations[0].files.len(), 1);
        assert!(
            saves
                .edit(&entry.id, SavePreferences::default(), 0)
                .await
                .is_err()
        );
        drop(saves);
        store.close().await;
        let store = LibraryStore::open(&path).await.unwrap();
        let saves = Saves::with_environment(store.clone(), cached, env);
        let state = saves.state(&entry.id).await.unwrap();
        assert_eq!(state.preferences, manual);
        assert_eq!(state.revision, 1);
        let explicit = SavePreferences {
            manifest_title: None,
            locations: Some(vec![LocationSpec {
                kind: LocationKind::Folder,
                path: base.join("hand-saves").to_string_lossy().into_owned(),
            }]),
        };
        let state = saves.edit(&entry.id, explicit, 1).await.unwrap();
        assert_eq!(state.matched, SaveMatch::Unknown);
        assert_eq!(state.locations[0].files.len(), 1);
        let state = saves
            .edit(
                &entry.id,
                SavePreferences {
                    manifest_title: None,
                    locations: Some(Vec::new()),
                },
                2,
            )
            .await
            .unwrap();
        assert!(state.locations.is_empty());
        let state = saves
            .edit(&entry.id, SavePreferences::default(), 3)
            .await
            .unwrap();
        assert_eq!(state.revision, 4);
        assert!(state.preferences.locations.is_none());
        assert!(state.locations.is_empty());
        let state = saves
            .edit(
                &entry.id,
                SavePreferences {
                    manifest_title: Some("No cached game with this name".into()),
                    locations: None,
                },
                4,
            )
            .await
            .unwrap();
        assert_eq!(state.matched, SaveMatch::Unknown);
        assert!(state.locations.is_empty());
        store.delete_entry(&entry.id).await.unwrap();
        assert_eq!(store.save_preferences(&entry.id).await.unwrap().0, 0);
    }

    #[tokio::test]
    async fn secondary_manifests_add_locations_and_missing_prefixes_never_fall_back_to_linux_home()
    {
        let dir = tempfile::tempdir().unwrap();
        let env = host(dir.path());
        let store = LibraryStore::in_memory().await.unwrap();
        let base = env.fs_root().join("games/secondary");
        let mut entry = install(&store, &base, "Steam Fixture", "steam", "steam:620").await;
        entry.runtime = GameRuntime::Windows {
            prefix: None,
            runner: None,
        };
        store.upsert_entry(&entry).await.unwrap();
        write(env.home().join("Documents/Fixture/wrong.sav"), "wrong");
        let saves = Saves::with_environment(store.clone(), manifest(dir.path()).await, env);
        let state = saves.state(&entry.id).await.unwrap();
        assert!(state.locations.is_empty());
        assert!(!state.warnings.is_empty());
        write(
            base.join(".ludusavi.yaml"),
            "Steam Fixture:\n  files:\n    ./custom: {tags: [save]}\n",
        );
        write(base.join("custom/slot.sav"), "right");
        let state = saves.state(&entry.id).await.unwrap();
        assert!(
            state
                .locations
                .iter()
                .flat_map(|location| &location.files)
                .any(|path| path.ends_with("slot.sav"))
        );
        let bad = SavePreferences {
            manifest_title: None,
            locations: Some(vec![LocationSpec {
                kind: LocationKind::File,
                path: "relative.sav".into(),
            }]),
        };
        assert!(saves.edit(&entry.id, bad, 0).await.is_err());
    }
}
