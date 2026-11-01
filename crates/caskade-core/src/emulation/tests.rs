use super::*;
use caskade_installer::OperatingSystem;
use std::{fmt::Write, path::PathBuf};

/// The examples from EMULATION-PLAN §3.1–§3.2.
const PLATFORMS: &str = r#"
[[platform]]
id = "nintendo_snes"
name = "Super Nintendo Entertainment System"
short = "SNES"
extensions = ["sfc", "smc", "fig", "bs", "st", "zip", "7z"]
ids = { igdb = 19, libretro = "Nintendo - Super Nintendo Entertainment System", playnite = "nintendo_super_nes", esde = "snes" }

[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue", "chd", "pbp", "m3u", "iso", "bin"]
ids = { igdb = 7, libretro = "Sony - PlayStation", playnite = "sony_playstation", esde = "psx" }
bios = [
  { file = "scph5501.bin", md5 = "490f666e1afb15b7362b406ed1cea246", region = "usa", required = true },
  { file = "scph5502.bin", md5 = "32736f17079d0b2b7024407c39bd3050", region = "europe", required = false },
]

[[region]]
id = "usa"
name = "USA"
codes = ["USA", "U"]
"#;

const RETROARCH: &str = r#"
id = "retroarch"
name = "RetroArch"
website = "https://www.retroarch.com/"

[detect.windows]
executable = '^retroarch\.exe$'
locations = ["{ProgramFiles}/RetroArch", "{AppData}/RetroArch", "{UserHome}/scoop/apps/retroarch/current"]

[detect.linux]
executable = '^retroarch$'
flatpak = "org.libretro.RetroArch"
path = ["retroarch"]
appimage = '^RetroArch.*\.AppImage$'

[detect.macos]
bundle = "com.libretro.RetroArch"

[vars]
cores = "{EmulatorDir}/cores"
lib = ".dll"
[vars.linux]
cores = "{XdgConfig}/retroarch/cores"
lib = ".so"
[vars.linux.flatpak]
cores = "{UserHome}/.var/app/org.libretro.RetroArch/config/retroarch/cores"
[vars.macos]
cores = "{AppSupport}/RetroArch/cores"
lib = ".dylib"

[[profile]]
name = "Snes9x"
platforms = ["nintendo_snes"]
extensions = ["sfc", "smc", "fig", "swc", "bs", "zip", "7z"]
args = ["-L", "{cores}/snes9x_libretro{lib}", "{RomPath}"]
requires = ["{cores}/snes9x_libretro{lib}"]

[[profile]]
name = "Beetle PSX HW"
platforms = ["sony_playstation"]
extensions = ["cue", "chd", "pbp", "m3u", "toc", "exe"]
args = ["-L", "{cores}/mednafen_psx_hw_libretro{lib}", "{ImagePath}"]
requires = ["{cores}/mednafen_psx_hw_libretro{lib}"]
"#;

const DUCKSTATION: &str = r#"
id = "duckstation"
name = "DuckStation"
tracking = { type = "process_name", name = "duckstation-qt" }

[detect.linux]
flatpak = "org.duckstation.DuckStation"
appimage = '^DuckStation.*\.AppImage$'

[[profile]]
name = "Default"
platforms = ["sony_playstation"]
extensions = ["cue", "chd", "m3u"]
args = ["-batch", "-fullscreen", "--", "{RomPath}"]
working_dir = "{RomDir}"
"#;

fn catalog() -> Catalog {
    Catalog::load([
        ("platforms.toml", PLATFORMS),
        ("retroarch.toml", RETROARCH),
        ("duckstation.toml", DUCKSTATION),
    ])
    .unwrap()
}

/// A plausible machine per OS; paths are written for that OS.
fn context(os: OperatingSystem) -> VarContext {
    let path = PathBuf::from;
    match os {
        OperatingSystem::Windows => VarContext {
            rom: Some(path(r"D:\ROMs\SNES\Chrono Trigger (USA).sfc")),
            emulator_dir: Some(path(r"C:\Program Files\RetroArch")),
            folders: OsFolders {
                user_home: Some(path(r"C:\Users\Ada")),
                app_data: Some(path(r"C:\Users\Ada\AppData\Roaming")),
                program_files: Some(path(r"C:\Program Files")),
                ..OsFolders::default()
            },
            ..VarContext::default()
        },
        OperatingSystem::Linux => VarContext {
            rom: Some(path("/home/ada/ROMs/SNES/Chrono Trigger (USA).sfc")),
            emulator_dir: Some(path("/opt/retroarch")),
            folders: OsFolders {
                user_home: Some(path("/home/ada")),
                xdg_config: Some(path("/home/ada/.config")),
                ..OsFolders::default()
            },
            ..VarContext::default()
        },
        OperatingSystem::Macos => VarContext {
            rom: Some(path("/Users/ada/ROMs/SNES/Chrono Trigger (USA).sfc")),
            emulator_dir: Some(path("/Applications/RetroArch.app")),
            folders: OsFolders {
                user_home: Some(path("/Users/ada")),
                app_support: Some(path("/Users/ada/Library/Application Support")),
                ..OsFolders::default()
            },
            ..VarContext::default()
        },
    }
}

/// Every shipped profile resolved for every install kind on every OS.
fn resolved_table(catalog: &Catalog) -> String {
    let mut out = String::new();
    for definition in catalog.emulators() {
        for profile in &definition.profiles {
            writeln!(out, "{} / {}", definition.id, profile.name).unwrap();
            for os in OSES {
                for kind in InstallKind::on(os) {
                    let resolved = definition
                        .resolve(&profile.name, os, *kind, &context(os))
                        .unwrap();
                    let requires: Vec<_> = resolved
                        .requires
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect();
                    writeln!(
                        out,
                        "  {:<7} {:<9} argv: {}",
                        os_key(os),
                        kind.key(),
                        resolved.args.join(" ⏐ ")
                    )
                    .unwrap();
                    if !requires.is_empty() {
                        writeln!(out, "  {:<17} requires: {}", "", requires.join(" ⏐ ")).unwrap();
                    }
                    if let Some(dir) = resolved.working_dir {
                        writeln!(out, "  {:<17} cwd: {dir}", "").unwrap();
                    }
                }
            }
        }
    }
    out
}

#[test]
fn definitions_load_and_resolve_argv_per_os_and_install_kind() {
    let catalog = catalog();
    assert_eq!(catalog.platforms().count(), 2);
    assert_eq!(catalog.region("usa").unwrap().codes, ["USA", "U"]);
    let psx = catalog.platform("sony_playstation").unwrap();
    assert!(psx.bios[0].required);
    assert_eq!(psx.ids.esde.as_deref(), Some("psx"));
    let duckstation = catalog.emulator("duckstation").unwrap();
    assert_eq!(
        duckstation.tracking,
        Some(Tracking::ProcessName {
            name: "duckstation-qt".into()
        })
    );
    let mut settings = insta::Settings::clone_current();
    // The cargo target dir may be outside the workspace; pin the path.
    settings.set_snapshot_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/emulation/snapshots"),
    );
    settings.bind(|| insta::assert_snapshot!(resolved_table(&catalog)));
}

#[test]
fn a_rom_path_with_spaces_stays_one_argument() {
    let catalog = catalog();
    let retroarch = catalog.emulator("retroarch").unwrap();
    let context = context(OperatingSystem::Linux);
    let resolved = retroarch
        .resolve(
            "Snes9x",
            OperatingSystem::Linux,
            InstallKind::Path,
            &context,
        )
        .unwrap();
    assert_eq!(resolved.args.len(), 3);
    assert_eq!(
        resolved.args[2],
        "/home/ada/ROMs/SNES/Chrono Trigger (USA).sfc"
    );
}

#[test]
fn rom_variables_and_aliases_use_the_host_path() {
    let rom = std::env::temp_dir().join("Final Fantasy VII (Disc 1).chd");
    let context = VarContext {
        rom: Some(rom.clone()),
        name: Some("Final Fantasy VII".into()),
        platform: Some("sony_playstation".into()),
        ..VarContext::default()
    };
    let resolved = resolve(
        &Vars::default(),
        &[
            "{RomName}".into(),
            "{ImageNameNoExt}".into(),
            "{RomDir}".into(),
            "--title={Name} ({Platform})".into(),
        ],
        None,
        &[],
        OperatingSystem::Linux,
        InstallKind::Folder,
        &context,
    )
    .unwrap();
    assert_eq!(
        resolved.args,
        [
            "Final Fantasy VII (Disc 1).chd".to_owned(),
            "Final Fantasy VII (Disc 1)".to_owned(),
            rom.parent().unwrap().display().to_string(),
            "--title=Final Fantasy VII (sony_playstation)".to_owned(),
        ]
    );
}

#[test]
fn a_rom_name_never_becomes_an_option() {
    // CSK-10: MAME takes `{RomNameNoExt}` as an argument of its own.
    let resolve_rom = |file: &str, args: &[&str]| {
        let context = VarContext {
            rom: Some(std::env::temp_dir().join(file)),
            ..VarContext::default()
        };
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        resolve(
            &Vars::default(),
            &args,
            None,
            &[],
            OperatingSystem::Linux,
            InstallKind::Folder,
            &context,
        )
    };
    for bare in ["{RomNameNoExt}", "{RomName}", "{ImageName}"] {
        let error = resolve_rom("-autoboot_script x.zip", &["-skip_gameinfo", bare]).unwrap_err();
        assert!(error.contains("starts with `-`"), "{bare}: {error}");
    }
    // Inside an option or a path, and for other names, it stays as it is.
    assert_eq!(
        resolve_rom("-odd.zip", &["-cart={RomName}", "{RomPath}"])
            .unwrap()
            .args[0],
        "-cart=-odd.zip"
    );
    assert_eq!(
        resolve_rom("pacman.zip", &["{RomNameNoExt}"]).unwrap().args,
        ["pacman"]
    );
}

#[test]
fn a_later_file_replaces_an_earlier_definition() {
    let fixed = RETROARCH.replace("name = \"RetroArch\"", "name = \"RetroArch (fixed)\"");
    let catalog = Catalog::load([
        ("platforms.toml", PLATFORMS),
        ("retroarch.toml", RETROARCH),
        ("plugin/retroarch.toml", fixed.as_str()),
    ])
    .unwrap();
    assert_eq!(catalog.emulators().count(), 1);
    assert_eq!(
        catalog.emulator("retroarch").unwrap().name,
        "RetroArch (fixed)"
    );
}

fn errors(files: &[(&str, &str)]) -> Vec<String> {
    match Catalog::load(files.iter().copied()) {
        Ok(_) => Vec::new(),
        Err(errors) => errors.0.iter().map(ToString::to_string).collect(),
    }
}

#[test]
fn validation_names_the_file_and_key() {
    let broken = r#"
id = "broken"
name = "Broken"

[detect.windows]
executable = '^broken(\.exe$'

[vars.linux.flatpak]
only_flatpak = "{UserHome}/x"

[vars]
loop = "{loop}/x"

[[profile]]
name = "One"
platforms = ["nintendo_snes", "sega_nowhere"]
extensions = [".SFC"]
args = ["{corez}/x", "{RomPath}"]

[[profile]]
name = "Two"
platforms = ["nintendo_snes"]
args = ["{only_flatpak}", "{loop}", "{unclosed"]
"#;
    let found = errors(&[("platforms.toml", PLATFORMS), ("broken.toml", broken)]);
    let expected = [
        "broken.toml: detect.windows.executable: invalid regex",
        "broken.toml: vars.loop: variable `{loop}` refers to itself",
        r#"broken.toml: profile "One".platforms: unknown platform "sega_nowhere""#,
        r#"broken.toml: profile "One".extensions: extension ".SFC" must be lowercase without the dot"#,
        r#"broken.toml: profile "One".args[0]: unknown variable `{corez}` on windows (folder)"#,
        r#"broken.toml: profile "Two".args[0]: unknown variable `{only_flatpak}` on windows (folder)"#,
        r#"broken.toml: profile "Two".args[0]: unknown variable `{only_flatpak}` on linux (folder)"#,
        r#"broken.toml: profile "Two".args[1]: variable `{loop}` refers to itself"#,
        r#"broken.toml: profile "Two".args[2]: unclosed `{` in "{unclosed""#,
    ];
    for want in expected {
        assert!(
            found.iter().any(|error| error.starts_with(want)),
            "missing {want:?} in {found:#?}"
        );
    }
    // A Flatpak-only variable is fine for the Flatpak install itself.
    assert!(
        !found
            .iter()
            .any(|error| error.contains("only_flatpak") && error.contains("(flatpak)"))
    );
}

#[test]
fn bios_files_may_sit_in_a_subfolder_of_the_bios_folder() {
    let platform = |file: &str| {
        format!(
            "[[platform]]\nid = \"x\"\nname = \"X\"\nshort = \"X\"\nbios = [{{ file = {file:?} }}]\n"
        )
    };
    for good in ["dc_boot.bin", "dc/dc_boot.bin"] {
        assert!(errors(&[("p.toml", &platform(good))]).is_empty(), "{good}");
    }
    for bad in [
        "/etc/passwd",
        "../x.bin",
        "dc//x.bin",
        "dc/./x.bin",
        r"dc\x.bin",
        "C:x.bin",
        "",
    ] {
        assert!(!errors(&[("p.toml", &platform(bad))]).is_empty(), "{bad}");
    }
}

#[test]
fn syntax_and_duplicates_fail_with_the_file_named() {
    let duplicate = r#"
[[platform]]
id = "x"
name = "X"
short = "X"
[[platform]]
id = "x"
name = "X again"
short = "X"
"#;
    let found = errors(&[
        ("dup.toml", duplicate),
        ("bad.toml", "id = \"bad\"\nname = \nbroken"),
        ("unknown.toml", "id = \"u\"\nname = \"U\"\nsurprise = 1\n"),
    ]);
    assert!(
        found[0].starts_with(r#"dup.toml: platform "x": defined twice"#),
        "{found:#?}"
    );
    assert!(
        found[1].starts_with("bad.toml: TOML parse error at line 2"),
        "{found:#?}"
    );
    assert!(found[2].contains("unknown field `surprise`"), "{found:#?}");
}

#[test]
fn custom_templates_know_only_the_builtins() {
    let vars = catalog().emulator("retroarch").unwrap().vars.clone();
    assert!(
        check_templates(
            &Vars::default(),
            InstallKind::Folder,
            ["{RomPath}", "{ImageDir}"]
        )
        .is_ok()
    );
    assert!(check_templates(&Vars::default(), InstallKind::Folder, ["{cores}"]).is_err());
    assert!(check_templates(&vars, InstallKind::Folder, ["{cores}/x{lib}"]).is_ok());
    assert!(check_templates(&vars, InstallKind::Folder, ["{RomPath"]).is_err());
}

#[test]
fn args_overrides_apply_to_the_definition_args() {
    let base = vec!["-L".to_owned(), "{RomPath}".to_owned()];
    assert_eq!(ArgsOverride::Default.apply(&base), base);
    let append = ArgsOverride::Append {
        args: vec!["--fullscreen".into()],
    };
    assert_eq!(append.apply(&base), ["-L", "{RomPath}", "--fullscreen"]);
    let replace = ArgsOverride::Replace {
        args: vec!["{RomPath}".into()],
        replaced: vec![],
    };
    assert_eq!(replace.apply(&base), ["{RomPath}"]);
}

#[test]
fn availability_needs_every_required_file() {
    let catalog = catalog();
    let retroarch = catalog.emulator("retroarch").unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("cores")).unwrap();
    let context = VarContext {
        emulator_dir: Some(temp.path().to_path_buf()),
        ..VarContext::default()
    };
    let snes = |os| {
        retroarch
            .resolve("Snes9x", os, InstallKind::Folder, &context)
            .unwrap()
    };
    assert!(!snes(OperatingSystem::Windows).available());
    std::fs::write(temp.path().join("cores/snes9x_libretro.dll"), b"").unwrap();
    assert!(snes(OperatingSystem::Windows).available());
}

#[test]
fn emulators_are_checked_against_their_definition() {
    let catalog = catalog();
    let mut emulator = Emulator {
        id: "ra".into(),
        name: "RetroArch".into(),
        definition: Some("retroarch".into()),
        install: EmulatorInstall::Flatpak {
            app_id: "org.libretro.RetroArch".into(),
        },
        profiles: vec![EmulatorProfile::BuiltIn {
            id: "snes".into(),
            definition_profile: "Snes9x".into(),
            args: ArgsOverride::Append {
                args: vec!["--config".into(), "{cores}/../snes.cfg".into()],
            },
            hooks: ProfileHooks::default(),
        }],
    };
    emulator.validate(Some(&catalog)).unwrap();
    let EmulatorProfile::BuiltIn { args, .. } = &mut emulator.profiles[0] else {
        unreachable!()
    };
    *args = ArgsOverride::Replace {
        args: vec!["{corez}".into()],
        replaced: vec![],
    };
    assert!(emulator.validate(Some(&catalog)).is_err());
    // Without the catalog only the syntax is checked.
    emulator.validate(None).unwrap();
    let EmulatorProfile::BuiltIn {
        definition_profile, ..
    } = &mut emulator.profiles[0]
    else {
        unreachable!()
    };
    *definition_profile = "Nope".into();
    assert!(emulator.validate(Some(&catalog)).is_err());

    let custom = Emulator {
        id: "custom".into(),
        name: "Custom".into(),
        definition: None,
        install: EmulatorInstall::Path {
            binary: "mednafen".into(),
        },
        profiles: vec![EmulatorProfile::Custom {
            id: "p".into(),
            name: "Mednafen".into(),
            executable: None,
            args: vec!["{cores}".into()],
            working_dir: None,
            platforms: vec!["sony_playstation".into()],
            extensions: vec!["cue".into()],
            env: vec![],
            tracking: Tracking::Default,
            hooks: ProfileHooks::default(),
        }],
    };
    assert!(
        custom.validate(None).is_err(),
        "custom profiles have no definition vars"
    );
}
