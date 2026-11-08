use caskade_core::emulation::{
    Catalog, InstallKind, OSES, OperatingSystem, OsFolders, Tracking, VarContext, os_key,
};
use caskade_sources::emulation::{catalog, definition_files};
use std::{collections::BTreeSet, fmt::Write, path::PathBuf};

fn context(os: OperatingSystem) -> VarContext {
    let (home, emulator, rom, config, appdata, programs, support) = match os {
        OperatingSystem::Windows => (
            r"C:\Users\fixture",
            r"C:\Emulators\Test Emulator",
            r"D:\ROMs\Sample Game (USA).rom",
            "",
            r"C:\Users\fixture\AppData\Roaming",
            r"C:\Program Files",
            "",
        ),
        OperatingSystem::Linux => (
            "/home/fixture",
            "/opt/Test Emulator",
            "/media/ROMs/Sample Game (USA).rom",
            "/home/fixture/.config",
            "",
            "",
            "",
        ),
        OperatingSystem::Macos => (
            "/Users/fixture",
            "/Applications/Test Emulator.app",
            "/Volumes/ROMs/Sample Game (USA).rom",
            "",
            "",
            "",
            "/Users/fixture/Library/Application Support",
        ),
    };
    let path = |value: &str| (!value.is_empty()).then(|| PathBuf::from(value));
    VarContext {
        rom: path(rom),
        emulator_dir: path(emulator),
        name: Some("Sample Game".into()),
        platform: Some("nintendo_snes".into()),
        game_id: Some("fixture-game".into()),
        caskade_dir: path("/fixture/Caskade"),
        storage_root: path("/fixture/Games"),
        folders: OsFolders {
            user_home: path(home),
            xdg_config: path(config),
            app_data: path(appdata),
            program_files: path(programs),
            app_support: path(support),
        },
    }
}

#[test]
fn embedded_catalog_validates_every_shipped_definition() {
    let catalog = catalog().unwrap();
    assert_eq!(definition_files().count(), 23);
    assert!(catalog.platforms().count() >= 463);
    assert_eq!(catalog.regions().count(), 20);
    assert_eq!(catalog.emulators().count(), 21);
    let expected = [
        "retroarch",
        "pcsx2",
        "duckstation",
        "dolphin",
        "ppsspp",
        "melonds",
        "mgba",
        "cemu",
        "ryujinx",
        "rpcs3",
        "mesen",
        "rmg",
        "gopher64",
        "flycast",
        "ymir",
        "ares",
        "mame",
        "azahar",
        "xemu",
        "xenia",
        "shadps4",
    ];
    for id in expected {
        let emulator = catalog.emulator(id).unwrap();
        assert!(!emulator.profiles.is_empty(), "{id}");
        assert!(
            emulator.tracking.is_some(),
            "{id} must declare its default tracking behavior"
        );
        for os in OSES {
            assert!(emulator.detect.on(os).is_some(), "{id}/{}", os_key(os));
        }
    }
    let covered: BTreeSet<_> = catalog
        .emulators()
        .flat_map(|e| &e.profiles)
        .flat_map(|p| &p.platforms)
        .collect();
    for platform in catalog.platforms() {
        // Recognition is independent of emulator support. Known extensions
        // remain required for the systems whose profiles we ship.
        if covered.contains(&platform.id) {
            assert!(!platform.extensions.is_empty(), "{}", platform.id);
        }
        for bios in &platform.bios {
            if let Some(region) = &bios.region {
                assert!(catalog.region(region).is_some(), "{region}");
            }
        }
    }
    assert!(matches!(
        catalog.emulator("ryujinx").unwrap().tracking,
        Some(Tracking::ProcessName { .. })
    ));
}

/// Check every source platform, including systems with no emulator profile
/// and no IGDB id. Adding a source row must never silently leave it out.
#[test]
fn every_romm_and_playnite_platform_has_a_definition() {
    let source: serde_json::Value =
        serde_json::from_str(include_str!("../emulation/platform-sources.json")).unwrap();
    let catalog = catalog().unwrap();
    for row in source["romm"].as_array().unwrap() {
        let slug = row["slug"].as_str().unwrap();
        let matches: Vec<_> = catalog
            .platforms()
            .filter(|p| p.ids.romm.iter().any(|alias| alias == slug))
            .collect();
        assert_eq!(matches.len(), 1, "RomM slug {slug}");
        if let Some(igdb) = row["igdb"].as_u64() {
            assert_eq!(matches[0].ids.igdb, Some(igdb as u32), "{slug}");
        }
    }
    for row in source["playnite"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        assert_eq!(
            catalog
                .platforms()
                .filter(|p| p.ids.playnite.as_deref() == Some(id))
                .count(),
            1,
            "Playnite id {id}"
        );
    }
    for (slug, id) in [
        ("atari2600", "atari_2600"),
        ("atari5200", "atari_5200"),
        ("atari7800", "atari_7800"),
        ("jaguar", "atari_jaguar"),
        ("neogeoaes", "snk_neogeo_aes"),
        ("gamegear", "sega_gamegear"),
        ("sg1000", "sega_sg1000"),
        ("tg16", "nec_turbografx_16"),
    ] {
        assert!(
            catalog
                .platform(id)
                .unwrap_or_else(|| panic!("missing {id}"))
                .ids
                .romm
                .iter()
                .any(|alias| alias == slug)
        );
    }
    // The pinned Playnite Vectrex id is wrong; never fold it into Intellivision.
    assert_eq!(catalog.platform("vectrex").unwrap().ids.igdb, Some(70));
    assert_eq!(
        catalog.platform("mattel_intellivision").unwrap().ids.igdb,
        Some(67)
    );
    let msx2 = catalog
        .platforms()
        .find(|p| p.ids.romm.iter().any(|s| s == "msx2"))
        .unwrap();
    let plus = catalog
        .platforms()
        .find(|p| p.ids.romm.iter().any(|s| s == "msx2plus"))
        .unwrap();
    assert_ne!(msx2.id, plus.id);
}

#[test]
fn shipped_profiles_resolve_on_every_os_and_install_kind() {
    let catalog = catalog().unwrap();
    for os in OSES {
        let mut table = String::new();
        let mut count = 0;
        for definition in catalog.emulators() {
            for profile in &definition.profiles {
                for kind in InstallKind::on(os) {
                    let context = context(os);
                    let resolved = definition
                        .resolve(&profile.name, os, *kind, &context)
                        .unwrap();
                    assert!(
                        resolved
                            .args
                            .iter()
                            // MAME takes the set's name and a ROM path.
                            .any(|arg| arg.contains("Sample Game (USA)")),
                        "{} / {} drops ROM",
                        definition.id,
                        profile.name
                    );
                    assert!(
                        resolved.args.iter().all(|arg| !arg.contains('{')),
                        "unexpanded variable"
                    );
                    writeln!(
                        table,
                        "{} / {} / {}",
                        definition.id,
                        profile.name,
                        kind.key()
                    )
                    .unwrap();
                    writeln!(table, "  argv: {:?}", resolved.args).unwrap();
                    if !resolved.requires.is_empty() {
                        // Host PathBuf separators must not affect virtual-OS snapshots.
                        let paths: Vec<_> = resolved
                            .requires
                            .iter()
                            .map(|p| p.to_string_lossy().replace('\\', "/"))
                            .collect();
                        writeln!(table, "  requires: {paths:?}").unwrap();
                    }
                    count += 1;
                }
            }
        }
        assert!(count >= 50, "validation must cover the complete catalogue");
        insta::assert_snapshot!(format!("shipped_argv_{}", os_key(os)), table);
    }
}

#[test]
fn flatpak_retroarch_cores_and_argv_boundaries_are_explicit() {
    let catalog = catalog().unwrap();
    let definition = catalog.emulator("retroarch").unwrap();
    let context = context(OperatingSystem::Linux);
    let profile = definition.profile("Snes9x").unwrap();
    let resolved = definition
        .resolve(
            &profile.name,
            OperatingSystem::Linux,
            InstallKind::Flatpak,
            &context,
        )
        .unwrap();
    assert_eq!(
        resolved.args,
        [
            "-L",
            "/home/fixture/.var/app/org.libretro.RetroArch/config/retroarch/cores/snes9x_libretro.so",
            "/media/ROMs/Sample Game (USA).rom"
        ]
    );
    assert_eq!(resolved.requires, [PathBuf::from(&resolved.args[1])]);
    assert!(
        catalog.emulator("rpcs3").unwrap().import.is_none(),
        "RPCS3 importer is a later feature"
    );
}

#[test]
fn save_defaults_cover_shipped_emulators_and_expand_for_every_os_and_install_kind() {
    let catalog = catalog().unwrap();
    for definition in catalog.emulators() {
        if definition.id == "xemu" {
            // xemu has no default HDD image: it is chosen by the user.
            assert!(definition.saves.files.is_empty());
            continue;
        }
        assert!(!definition.saves.files.is_empty(), "{}", definition.id);
        for os in OSES {
            for kind in InstallKind::on(os) {
                let paths = definition.resolve_saves(os, *kind, &context(os)).unwrap();
                for pattern in paths.files.iter().chain(&paths.states) {
                    assert!(!pattern.contains('{'), "{}/{pattern}", definition.id);
                    assert!(
                        pattern.starts_with('/') || pattern.as_bytes().get(1) == Some(&b':'),
                        "{pattern}"
                    );
                }
            }
        }
    }
    for (file, text) in definition_files().filter(|(_, text)| text.contains("[saves]")) {
        assert!(
            text.contains("# Save defaults: https://"),
            "{file} has no upstream source"
        );
    }
}

/// #275: RetroArch's configuration that syncs is its config_directory on
/// every OS and install kind, under the `{config}` folder its files are
/// named by on other devices.
#[test]
fn retroarch_config_is_its_config_directory_everywhere() {
    let catalog = catalog().unwrap();
    let retroarch = catalog.emulator("retroarch").unwrap();
    let resolve = |os, kind, emulator_dir: Option<&str>| {
        let mut context = context(os);
        context.rom = None;
        context.emulator_dir = emulator_dir.map(Into::into);
        let files = retroarch.resolve_config(os, kind, &context).unwrap();
        let roots = retroarch.save_roots(os, kind, &context);
        let config = roots
            .into_iter()
            .find(|(name, _)| name == "{config}")
            .map(|(_, path)| path);
        (files, config)
    };
    assert_eq!(
        resolve(OperatingSystem::Linux, InstallKind::AppImage, None),
        (
            vec!["/home/fixture/.config/retroarch/config/**".to_owned()],
            Some("/home/fixture/.config/retroarch/config".to_owned())
        )
    );
    assert_eq!(
        resolve(
            OperatingSystem::Linux,
            InstallKind::Flatpak,
            Some("/home/fixture/.var/app/org.libretro.RetroArch")
        )
        .1
        .as_deref(),
        Some("/home/fixture/.var/app/org.libretro.RetroArch/config/retroarch/config")
    );
    assert_eq!(
        resolve(
            OperatingSystem::Windows,
            InstallKind::Folder,
            Some(r"C:\Emulators\RetroArch")
        )
        .1
        .as_deref(),
        Some("C:/Emulators/RetroArch/config")
    );
    let macos = resolve(OperatingSystem::Macos, InstallKind::AppBundle, None).1;
    assert!(
        macos
            .as_deref()
            .is_some_and(|path| path.ends_with("/RetroArch/config")),
        "{macos:?}"
    );
    // Only RetroArch declares configuration so far.
    for definition in catalog
        .emulators()
        .filter(|definition| definition.id != "retroarch")
    {
        assert!(definition.config.files.is_empty(), "{}", definition.id);
    }
}

#[test]
fn retroarch_and_rom_named_saves_stay_per_rom_and_literals_are_not_globs() {
    let catalog = catalog().unwrap();
    let mut context = context(OperatingSystem::Linux);
    context.rom = Some("/ROMs[one]/Game[1].gba".into());
    context.emulator_dir = Some("/home/fixture/.var/app/org.libretro.RetroArch".into());
    let paths = catalog
        .emulator("retroarch")
        .unwrap()
        .resolve_saves(OperatingSystem::Linux, InstallKind::Flatpak, &context)
        .unwrap();
    assert_eq!(
        paths.files[0],
        "/home/fixture/.var/app/org.libretro.RetroArch/config/retroarch/saves/**/Game[[]1[]].srm"
    );
    assert!(paths.states[0].ends_with("/states/**/Game[[]1[]].state*"));
    let paths = catalog
        .emulator("mgba")
        .unwrap()
        .resolve_saves(OperatingSystem::Linux, InstallKind::Folder, &context)
        .unwrap();
    assert_eq!(paths.files[0], "/ROMs[[]one[]]/Game[[]1[]].sav");
    context.rom = None;
    assert!(
        catalog
            .emulator("mgba")
            .unwrap()
            .resolve_saves(OperatingSystem::Linux, InstallKind::Folder, &context,)
            .is_err()
    );
    let windows = catalog
        .emulator("mgba")
        .unwrap()
        .resolve_saves(
            OperatingSystem::Windows,
            InstallKind::Folder,
            &self::context(OperatingSystem::Windows),
        )
        .unwrap();
    assert_eq!(windows.files[0], "D:/ROMs/Sample Game (USA).sav");
}

#[test]
fn invalid_save_definitions_fail_at_catalog_load() {
    for body in [
        "files = ['{Missing}/saves']",
        "files = ['']",
        "states = ['relative/save']",
        "files = ['{RomDir}/[']",
        "unknown = []",
    ] {
        let text = format!("id='bad'\nname='Bad'\n[saves]\n{body}\n");
        assert!(
            Catalog::load([("bad.toml", text.as_str())]).is_err(),
            "{body}"
        );
    }
    assert!(Catalog::load([("old.toml", "id='old'\nname='Old'\n")]).is_ok());
}

#[test]
fn user_definition_overrides_preserve_the_shipped_catalogue() {
    let mut files: Vec<_> = definition_files().collect();
    files.push(("user/retroarch.toml", "id='retroarch'\nname='User RetroArch'\n[[profile]]\nname='Custom'\nplatforms=['nintendo_snes']\nargs=['{RomPath}']\n"));
    let catalog = Catalog::load(files).unwrap();
    assert_eq!(
        catalog.emulator("retroarch").unwrap().name,
        "User RetroArch"
    );
    assert_eq!(
        catalog.platforms().count(),
        self::catalog().unwrap().platforms().count()
    );
    assert_eq!(catalog.emulators().count(), 21);
}

/// The files each GitHub source picks, as released when the source was
/// written (`emulation/SOURCES.md`). A rename upstream fails here.
#[test]
fn install_sources_match_released_files() {
    use caskade_core::emulation::InstallSource;
    let released = [
        ("pcsx2", "windows", "pcsx2-v2.8.2-windows-x64-Qt.7z"),
        (
            "pcsx2",
            "linux",
            "pcsx2-v2.8.2-linux-appimage-x64-Qt.AppImage",
        ),
        ("pcsx2", "macos", "pcsx2-v2.8.2-macos-Qt.tar.xz"),
        (
            "duckstation",
            "windows",
            "duckstation-windows-x64-release.zip",
        ),
        ("duckstation", "linux", "DuckStation-x64.AppImage"),
        ("duckstation", "macos", "duckstation-mac-release.zip"),
        ("ppsspp", "windows", "PPSSPP-v1.20.4-Windows-x64.zip"),
        ("ppsspp", "linux", "PPSSPP-v1.20.4-anylinux-x86_64.AppImage"),
        ("ppsspp", "macos", "PPSSPPSDL-macOS-v1.20.4.zip"),
        ("melonds", "windows", "melonDS-1.1-windows-x86_64.zip"),
        ("melonds", "linux", "melonDS-1.1-appimage-x86_64.zip"),
        ("melonds", "macos", "melonDS-1.1-macOS-universal.zip"),
        ("mgba", "windows", "mGBA-0.10.5-win64.7z"),
        ("mgba", "linux", "mGBA-0.10.5-appimage-x64.appimage"),
        ("mgba", "macos", "mGBA-0.10.5-macos.dmg"),
        ("cemu", "windows", "cemu-2.6-windows-x64.zip"),
        ("cemu", "linux", "Cemu-2.6-x86_64.AppImage"),
        ("cemu", "macos", "cemu-2.6-macos-12-x64.dmg"),
        (
            "rpcs3",
            "windows",
            "rpcs3-v0.0.43-20415-6c44fff7_win64_msvc.7z",
        ),
        (
            "rpcs3",
            "linux",
            "rpcs3-v0.0.43-20415-6c44fff7_linux64.AppImage",
        ),
        (
            "rpcs3",
            "macos",
            "rpcs3-v0.0.43-20415-6c44fff7_macos_aarch64.7z",
        ),
        ("mesen", "windows", "Mesen_2.2.1_Windows.zip"),
        ("mesen", "linux", "Mesen_2.2.1_Linux_x64.zip"),
        ("mesen", "macos", "Mesen_2.2.1_macOS_ARM64_AppleSilicon.zip"),
        ("rmg", "windows", "RMG-Portable-Windows64-v0.9.0.zip"),
        ("rmg", "linux", "RMG-Portable-Linux64-v0.9.0.AppImage"),
        ("gopher64", "windows", "gopher64-windows-x86_64.exe"),
        ("gopher64", "linux", "gopher64-linux-x86_64"),
        ("gopher64", "macos", "gopher64-macos-aarch64.zip"),
        ("flycast", "windows", "flycast-win64-2.7.zip"),
        ("flycast", "linux", "flycast-x86_64-2.7.AppImage"),
        ("flycast", "macos", "flycast-macOS-2.7.zip"),
        ("ymir", "windows", "ymir-windows-x86_64-AVX2-v0.4.1.zip"),
        ("ymir", "linux", "ymir-linux-x86_64-AVX2-v0.4.1.tar.xz"),
        ("ymir", "macos", "ymir-macos-arm64-v0.4.1.zip"),
        ("ares", "windows", "ares-windows-x64.zip"),
        ("ares", "macos", "ares-macos-universal.zip"),
        ("mame", "windows", "mame0289b_x64.exe"),
        ("azahar", "windows", "azahar-windows-msvc-2126.2.zip"),
        ("azahar", "linux", "azahar.AppImage"),
        ("azahar", "macos", "azahar-macos-universal-2126.2.zip"),
        ("xemu", "windows", "xemu-0.8.139-windows-x86_64.zip"),
        ("xemu", "linux", "xemu-0.8.139-x86_64.AppImage"),
        ("xemu", "macos", "xemu-0.8.139-macos-universal.zip"),
        ("xenia", "windows", "xenia_canary_windows.7z"),
        ("xenia", "linux", "xenia_canary_linux.AppImage"),
        ("shadps4", "windows", "shadps4-win64-sdl-2026-0.20.0.zip"),
        ("shadps4", "linux", "shadps4-linux-sdl-0.20.0.zip"),
        ("shadps4", "macos", "shadps4-macos-sdl-2026-0.20.0.zip"),
    ];
    let catalog = catalog().unwrap();
    for (id, os, file) in released {
        let os = OSES.into_iter().find(|known| os_key(*known) == os).unwrap();
        let Some(InstallSource::Github {
            asset: Some(asset), ..
        }) = catalog.emulator(id).unwrap().install.on(os)
        else {
            panic!("{id} has no GitHub asset on {}", os_key(os));
        };
        assert!(
            regex::Regex::new(asset).unwrap().is_match(file),
            "{id} on {}: {asset} doesn't match {file}",
            os_key(os)
        );
    }
    // Siblings that must not be picked.
    let pcsx2 = catalog.emulator("pcsx2").unwrap();
    let Some(InstallSource::Github {
        asset: Some(asset), ..
    }) = pcsx2.install.on(OperatingSystem::Windows)
    else {
        panic!()
    };
    assert!(
        !regex::Regex::new(asset)
            .unwrap()
            .is_match("pcsx2-v2.8.2-windows-x64-installer.exe")
    );
    // `mame0289lx.zip` is MAME's game list, not a Linux build; xemu's debug zip.
    let asset = |id: &str, os: OperatingSystem| match catalog.emulator(id).unwrap().install.on(os) {
        Some(InstallSource::Github {
            asset: Some(asset), ..
        }) => regex::Regex::new(asset).unwrap(),
        _ => panic!("{id}"),
    };
    assert!(!asset("mame", OperatingSystem::Windows).is_match("mame0289lx.zip"));
    assert!(
        !asset("xemu", OperatingSystem::Windows).is_match("xemu-0.8.139-windows-x86_64-dbg.zip")
    );
}

#[test]
fn every_recommended_platform_has_an_installable_recommendation() {
    let catalog = catalog().unwrap();
    for platform in catalog.platforms() {
        if platform.recommended.is_empty() {
            continue;
        }
        let recommended = catalog.recommended(&platform.id);
        assert_eq!(
            recommended.len(),
            platform.recommended.len(),
            "{}: every recommendation names a profile for it",
            platform.id
        );
        for os in OSES {
            // Xenia has no macOS build of its own (XeniOS is a separate port).
            if (platform.id.as_str(), os) == ("microsoft_xbox360", OperatingSystem::Macos) {
                continue;
            }
            let installable = recommended
                .iter()
                .any(|(emulator, _)| emulator.install.on(os).is_some());
            assert!(installable, "{} on {}", platform.id, os_key(os));
        }
    }
}

#[test]
fn shipped_downloads_are_https() {
    use caskade_core::emulation::InstallSource;
    let catalog = catalog().unwrap();
    for emulator in catalog.emulators() {
        for os in OSES {
            let urls = [
                match emulator.install.on(os) {
                    Some(InstallSource::Github { url, .. }) => url.clone(),
                    Some(InstallSource::Url { url }) => Some(url.clone()),
                    _ => None,
                },
                emulator.requires_from.on(os).map(str::to_owned),
            ];
            for url in urls.into_iter().flatten() {
                assert!(url.starts_with("https://"), "{}: {url}", emulator.id);
            }
        }
    }
    let retroarch = catalog.emulator("retroarch").unwrap();
    assert!(
        OSES.iter()
            .all(|os| retroarch.requires_from.on(*os).is_some())
    );
}

/// Every shipped install source resolves to a file on the real hosts
/// today (GitHub's website, Dolphin's endpoint). Nothing is downloaded.
/// `cargo test -p caskade-sources --features live-tests --test it live_install_sources -- --ignored --nocapture`
#[cfg(feature = "live-tests")]
#[tokio::test]
#[ignore = "hits github.com and dolphin-emu.org"]
async fn live_install_sources_resolve() {
    use caskade_core::emulation::{InstallSource, setup};
    let catalog = catalog().unwrap();
    let hosts = setup::Hosts::default();
    let mut failed = Vec::new();
    for emulator in catalog.emulators() {
        for os in OSES {
            let Some(source) = emulator.install.on(os) else {
                continue;
            };
            if matches!(source, InstallSource::Flatpak { .. }) {
                continue;
            }
            match setup::resolve(&hosts, source).await {
                Ok(resolved) => println!(
                    "{:<12} {:<8} {:<28} {}",
                    emulator.id,
                    os_key(os),
                    resolved.version.unwrap_or_default(),
                    resolved.file
                ),
                Err(error) => failed.push(format!("{} {}: {error}", emulator.id, os_key(os))),
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Installs real releases into a temp folder, with a temp home, and checks
/// Caskade finds each emulator in what it unpacked. Downloads ~300 MB.
/// `CASKADE_LIVE_EMULATORS=mgba,pcsx2` picks others.
#[cfg(feature = "live-tests")]
#[tokio::test]
#[ignore = "downloads emulators from their projects"]
async fn live_installs_find_the_emulator() {
    use caskade_core::{
        AppPaths,
        bus::Bus,
        db::LibraryStore,
        emulation::{EmulationService, EmulatorSetup, OsFolders, SetupStatus, host_os},
    };
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let emulation = EmulationService::new(store.clone(), bus.clone(), catalog().unwrap())
        .with_host(
            host_os(),
            OsFolders {
                xdg_config: Some(home.join(".config")),
                user_home: Some(home.clone()),
                ..OsFolders::default()
            },
        );
    let setup = EmulatorSetup::new(store.clone(), bus, emulation, &AppPaths::under(temp.path()));
    let wanted = std::env::var("CASKADE_LIVE_EMULATORS")
        .unwrap_or_else(|_| "mgba,melonds,pcsx2,retroarch".into());
    let mut failed = Vec::new();
    for id in wanted.split(',') {
        let job = setup.install(id).await.unwrap();
        let job = loop {
            let job = setup.job(&job.id).unwrap();
            if job.status.finished() {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        };
        if job.status != SetupStatus::Done {
            failed.push(format!("{id}: {:?}", job.error));
            continue;
        }
        let configured = store
            .emulators()
            .await
            .unwrap()
            .into_iter()
            .find(|row| Some(&row.id) == job.emulator_id.as_ref())
            .unwrap();
        println!(
            "{id:<10} {:<24} {:?} {:?}",
            job.version.unwrap_or_default(),
            configured.install,
            job.warnings
        );
    }
    for path in ["home/.config/PCSX2/inis/PCSX2.ini"] {
        if let Ok(text) = std::fs::read_to_string(temp.path().join(path)) {
            println!("{path}:\n{text}");
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}
