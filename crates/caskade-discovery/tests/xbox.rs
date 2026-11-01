#![cfg(feature = "windows-sources")]
//! Original synthetic metadata only. No installed games or Store accounts.
use caskade_discovery::{
    sources::xbox::{ID, Xbox},
    *,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
const PUBLISHER: &str =
    "CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US";
const NAME: &str = "Fixture.GamePass";
const PFN: &str = "Fixture.GamePass_8wekyb3d8bbwe";
const GAME: &str = r"D:\XboxGames\Fixture Game\Content";
struct Machine {
    temp: tempfile::TempDir,
}
impl Machine {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        for drive in ["c", "d"] {
            fs::create_dir_all(temp.path().join(format!("root/drives/{drive}"))).unwrap();
        }
        fs::write(
            temp.path().join("machine.toml"),
            r#"schema_version=1
os="windows"
[[volumes]]
path='C:\'
kind="fixed"
[[volumes]]
path='D:\'
kind="fixed"
"#,
        )
        .unwrap();
        Self { temp }
    }
    fn env(&self) -> HostEnv {
        HostEnv::fixture(self.temp.path()).unwrap()
    }
    fn path(&self, path: &str) -> PathBuf {
        self.env().resolve_path(Path::new(path)).unwrap()
    }
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn marker(&self, drive: &str, paths: &[&str]) {
        let mut bytes = b"RGBX".to_vec();
        bytes.extend_from_slice(&(paths.len() as u32).to_le_bytes());
        for path in paths {
            for u in path.encode_utf16().chain([0]) {
                bytes.extend_from_slice(&u.to_le_bytes());
            }
        }
        self.write(&format!(r"{drive}:\.GamingRoot"), bytes);
    }
    fn game(&self) {
        self.marker("D", &[r"\XboxGames"]);
        self.write(
            &format!(r"{GAME}\MicrosoftGame.config"),
            config(
                PUBLISHER,
                "Fixture Game &amp; Friends",
                r#"<Executable Name="bin\play.exe" Id="MainGame" TargetDeviceFamily="PC"/>"#,
            ),
        );
        self.write(
            &format!(r"{GAME}\AppxManifest.xml"),
            appx(PUBLISHER, "MainGame", r"bin\play.exe"),
        );
        self.write(
            &format!(r"{GAME}\bin\play.exe"),
            b"inert synthetic fixture, never executed",
        );
        self.write(&format!(r"{GAME}\logo.png"), b"synthetic image placeholder");
    }
    fn scan(&self) -> (Vec<DetectedGame>, Vec<String>) {
        let ctx = ScanCtx::new(Cancel::new());
        let games = Xbox.scan(&self.env(), &ctx).unwrap();
        (games, ctx.take_warnings())
    }
}
fn config(publisher: &str, title: &str, executables: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><Game configVersion="1">
<Identity Name="{NAME}" Publisher="{publisher}" Version="1.0.0.0"/>
<ExecutableList>{executables}</ExecutableList>
<ShellVisuals DefaultDisplayName="{title}" Square150x150Logo="logo.png"/>
<StoreId>9N000FIXTURE1</StoreId></Game>"#
    )
}
fn appx(publisher: &str, id: &str, exe: &str) -> String {
    format!(
        r#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10">
<Identity Name="{NAME}" Publisher="{publisher}" Version="1.0.0.0"/>
<Applications><Application Id="{id}" Executable="{exe}"><uap:VisualElements DisplayName="AppX fallback title"/></Application></Applications></Package>"#
    )
}
#[test]
fn game_pass_on_second_drive_and_non_game_store_app_excluded() {
    let m = Machine::new();
    m.game();
    m.marker("C", &["XboxGames"]);
    m.write(
        r"C:\XboxGames\Store Utility\Content\AppxManifest.xml",
        appx(PUBLISHER, "Utility", "utility.exe"),
    );
    let before = fs::read(m.path(&format!(r"{GAME}\MicrosoftGame.config"))).unwrap();
    let (games, warnings) = m.scan();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(games.len(), 1);
    let g = &games[0];
    assert_eq!(g.source, ID);
    assert_eq!(g.title, "Fixture Game & Friends");
    assert_eq!(g.key, format!("{PFN}!MainGame"));
    assert_eq!(
        g.ids,
        [
            ExternalId::Xbox { pfn: PFN.into() },
            ExternalId::MicrosoftStore {
                store_id: "9N000FIXTURE1".into()
            }
        ]
    );
    assert_eq!(g.install_dir, Some(m.path(GAME)));
    assert_eq!(g.state, InstallState::Installed);
    assert_eq!(g.confidence, Confidence::Certain);
    assert_eq!(
        g.launch,
        [LaunchTarget::Uri {
            uri: format!(r"shell:AppsFolder\{PFN}!MainGame")
        }]
    );
    assert_eq!(g.tracking.as_ref().unwrap().exe_names, ["play.exe"]);
    assert_eq!(g.icon, Some(m.path(&format!(r"{GAME}\logo.png"))));
    assert_eq!(
        before,
        fs::read(m.path(&format!(r"{GAME}\MicrosoftGame.config"))).unwrap()
    );
}

#[test]
fn public_scan_routes_the_builtin_xbox_source_and_preserves_package_identity() {
    let m = Machine::new();
    m.game();
    let config = Config {
        sources: SourceSelection::only([ID]),
        ..Default::default()
    };
    let report = scan(&m.env(), &config, &Cancel::new()).unwrap();
    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.sources[0].source, ID);
    assert!(report.sources[0].error.is_none());
    assert_eq!(report.sources[0].count, 1);
    assert_eq!(report.groups.len(), 1);
    let game = report.groups[0].primary_entry().unwrap();
    assert_eq!(game.key, format!("{PFN}!MainGame"));
    assert!(game.ids.contains(&ExternalId::Xbox { pfn: PFN.into() }));
    assert!(game.ids.contains(&ExternalId::MicrosoftStore {
        store_id: "9N000FIXTURE1".into()
    }));
}
#[test]
fn publisher_hash_matches_independent_windows_package_vectors() {
    let m = Machine::new();
    m.game();
    for (publisher, suffix) in [
        (PUBLISHER, "8wekyb3d8bbwe"),
        (
            "CN=Microsoft Windows, O=Microsoft Corporation, L=Redmond, S=Washington, C=US",
            "cw5n1h2txyewy",
        ),
    ] {
        m.write(
            &format!(r"{GAME}\MicrosoftGame.config"),
            config(
                publisher,
                "Fixture",
                r#"<Executable Name="bin/play.exe" Id="MainGame"/>"#,
            ),
        );
        m.write(
            &format!(r"{GAME}\AppxManifest.xml"),
            appx(publisher, "MainGame", "bin/play.exe"),
        );
        assert_eq!(m.scan().0[0].key, format!("{NAME}_{suffix}!MainGame"));
    }
}
#[test]
fn utf16_xml_and_escaped_identity_are_decoded_without_loss() {
    let m = Machine::new();
    m.game();
    let publisher = "CN=Fixture &amp; Friends";
    let xml = config(
        publisher,
        "Caf&#233; &amp; Friends",
        r#"<Executable Name="bin/play.exe" Id="MainGame"/>"#,
    )
    .replace("utf-8", "utf-16");
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    m.write(&format!(r"{GAME}\MicrosoftGame.config"), bytes);
    m.write(
        &format!(r"{GAME}\AppxManifest.xml"),
        appx(publisher, "MainGame", "bin/play.exe"),
    );
    let (g, w) = m.scan();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(g[0].title, "Café & Friends");
    assert!(g[0].key.starts_with(NAME));
}
#[test]
fn selects_non_development_pc_executable_and_matching_app() {
    let m = Machine::new();
    m.game();
    m.write(
        &format!(r"{GAME}\MicrosoftGame.config"),
        config(
            PUBLISHER,
            "Fixture",
            r#"
<Executable Name="dev.exe" Id="Dev" IsDevOnly="true"/>
<Executable Name="console.exe" Id="Console" TargetDeviceFamily="Scarlett"/>
<Executable Name="bin/play.exe" Id="MainGame" TargetDeviceFamily="PC"/>"#,
        ),
    );
    assert_eq!(m.scan().0[0].key, format!("{PFN}!MainGame"));
}
#[test]
fn console_or_development_only_config_is_not_a_pc_game() {
    let m = Machine::new();
    m.game();
    for exe in [
        r#"<Executable Name="dev.exe" Id="Dev" IsDevOnly="true"/>"#,
        r#"<Executable Name="console.exe" Id="Console" TargetDeviceFamily="XboxOne"/>"#,
    ] {
        m.write(
            &format!(r"{GAME}\MicrosoftGame.config"),
            config(PUBLISHER, "Fixture", exe),
        );
        assert!(m.scan().0.is_empty());
    }
}
#[test]
fn conflicting_appx_identity_or_application_is_rejected() {
    let m = Machine::new();
    m.game();
    for xml in [
        appx("CN=Someone Else", "MainGame", r"bin\play.exe"),
        appx(PUBLISHER, "WrongApp", r"bin\play.exe"),
    ] {
        m.write(&format!(r"{GAME}\AppxManifest.xml"), xml);
        let (g, w) = m.scan();
        assert!(g.is_empty());
        assert!(!w.is_empty());
    }
}

#[test]
fn registered_launch_helper_keeps_real_game_tracking_and_shell_activation() {
    let m = Machine::new();
    m.game();
    m.write(
        &format!(r"{GAME}\AppxManifest.xml"),
        appx(PUBLISHER, "MainGame", "gamelaunchhelper.exe"),
    );
    let (g, w) = m.scan();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(g[0].tracking.as_ref().unwrap().exe_names, ["play.exe"]);
    assert_eq!(
        g[0].launch,
        [LaunchTarget::Uri {
            uri: format!(r"shell:AppsFolder\{PFN}!MainGame")
        }]
    );
}
#[test]
fn missing_executable_is_broken_and_missing_appx_still_uses_config_identity() {
    let m = Machine::new();
    m.game();
    fs::remove_file(m.path(&format!(r"{GAME}\bin\play.exe"))).unwrap();
    fs::remove_file(m.path(&format!(r"{GAME}\AppxManifest.xml"))).unwrap();
    let (g, w) = m.scan();
    assert!(w.is_empty());
    assert_eq!(g.len(), 1);
    assert!(matches!(g[0].state, InstallState::Broken { .. }));
    assert_eq!(
        g[0].launch,
        [LaunchTarget::Uri {
            uri: format!(r"shell:AppsFolder\{PFN}!MainGame")
        }]
    );
}
#[test]
fn no_guessed_app_id_or_resource_key_title() {
    let m = Machine::new();
    m.game();
    m.write(
        &format!(r"{GAME}\MicrosoftGame.config"),
        config(PUBLISHER, "Fixture", r#"<Executable Name="bin/play.exe"/>"#),
    );
    assert!(m.scan().0.is_empty());
    m.write(
        &format!(r"{GAME}\MicrosoftGame.config"),
        config(
            PUBLISHER,
            "ms-resource:GameName",
            r#"<Executable Name="bin/play.exe" Id="MainGame"/>"#,
        ),
    );
    assert_eq!(m.scan().0[0].title, "AppX fallback title");
    m.write(
        &format!(r"{GAME}\AppxManifest.xml"),
        appx(PUBLISHER, "MainGame", r"bin\play.exe")
            .replace("AppX fallback title", "ms-resource:Other"),
    );
    assert!(m.scan().0.is_empty());
}
#[test]
fn corrupt_markers_and_drive_parent_paths_do_not_find_games() {
    let m = Machine::new();
    m.game();
    for bytes in [
        b"not RGBX".to_vec(),
        b"RGBX\xff\xff\xff\xff".to_vec(),
        b"RGBX\x01\0\0\0X\0".to_vec(),
    ] {
        m.write(r"D:\.GamingRoot", bytes);
        let (g, w) = m.scan();
        assert!(g.is_empty());
        assert!(!w.is_empty());
    }
    for path in [
        r"..\XboxGames",
        r"C:\XboxGames",
        r"\..\XboxGames",
        r"\\server\XboxGames",
    ] {
        m.marker("D", &[path]);
        let (g, w) = m.scan();
        assert!(g.is_empty());
        assert!(!w.is_empty());
    }
}
#[test]
fn xml_limits_dtd_and_package_path_traversal_are_rejected() {
    let m = Machine::new();
    m.game();
    for xml in [
        config(
            PUBLISHER,
            "Fixture",
            r#"<Executable Name="../outside.exe" Id="MainGame"/>"#,
        ),
        config(
            PUBLISHER,
            "Fixture",
            r#"<Executable Name="C:\outside.exe" Id="MainGame"/>"#,
        ),
        format!(
            "<!DOCTYPE Game [<!ENTITY title 'unexpected'>]>{}",
            config(
                PUBLISHER,
                "Fixture",
                r#"<Executable Name="bin/play.exe" Id="MainGame"/>"#
            )
        ),
        "<Game>".repeat(40),
        "x".repeat(1024 * 1024 + 1),
    ] {
        m.write(&format!(r"{GAME}\MicrosoftGame.config"), xml);
        let (g, w) = m.scan();
        assert!(g.is_empty());
        assert!(!w.is_empty());
    }
}
#[test]
fn multiple_marker_libraries_deduplicate_and_watch_absent_drive_markers() {
    let m = Machine::new();
    m.game();
    m.marker("D", &["XboxGames", r"\XboxGames"]);
    assert_eq!(m.scan().0.len(), 1);
    let env = m.env();
    let watch = Xbox.watch_paths(&env);
    assert!(
        watch
            .iter()
            .any(|p| p.path == m.path(r"C:\.GamingRoot") && !p.recursive)
    );
    assert!(
        watch
            .iter()
            .any(|p| p.path == m.path(r"D:\XboxGames") && p.recursive)
    );
    assert!(
        Xbox.launcher_locations(&env)
            .contains(&m.path(r"C:\.GamingRoot"))
    );
    let cancel = Cancel::new();
    cancel.cancel();
    assert_eq!(
        Xbox.scan(&env, &ScanCtx::new(cancel)),
        Err(SourceError::Cancelled)
    );
}
#[cfg(unix)]
#[test]
fn library_and_executable_links_cannot_escape_their_roots() {
    use std::os::unix::fs::symlink;
    let m = Machine::new();
    m.game();
    fs::remove_file(m.path(&format!(r"{GAME}\bin\play.exe"))).unwrap();
    let outside = m.write(r"D:\outside.exe", b"inert");
    symlink(outside, m.path(&format!(r"{GAME}\bin\play.exe"))).unwrap();
    assert!(m.scan().0.is_empty());
    let m = Machine::new();
    m.game();
    let library = m.path(r"D:\XboxGames");
    fs::rename(&library, m.path(r"C:\XboxGames")).unwrap();
    symlink(m.path(r"C:\XboxGames"), library).unwrap();
    let (g, w) = m.scan();
    assert!(g.is_empty());
    assert!(!w.is_empty());
}
