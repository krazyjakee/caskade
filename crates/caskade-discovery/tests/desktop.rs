#![cfg(feature = "linux-sources")]
use caskade_discovery::{
    sources::{
        desktop::{self, Desktop},
        flatpak::Flatpak,
        snap::Snap,
    },
    *,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

struct Machine {
    temp: tempfile::TempDir,
}
impl Machine {
    fn new() -> Self {
        let temp = crate::fixtures::tempdir();
        fs::create_dir(temp.path().join("root")).unwrap();
        fs::write(temp.path().join("machine.toml"),"schema_version=1\nos=\"linux\"\n[env]\nPATH=\"/usr/bin:/snap/bin\"\nLANG=\"fr_FR.UTF-8\"\n").unwrap();
        Self { temp }
    }
    fn file(&self, path: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let path = self
            .temp
            .path()
            .join("root")
            .join(path.trim_start_matches('/'));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }
    fn exe(&self, path: &str) -> PathBuf {
        let path = self.file(path, b"synthetic never executed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }
    fn desktop(&self, name: &str, body: &str) -> PathBuf {
        self.file(
            &format!("/home/fixture/.local/share/applications/{name}"),
            format!("[Desktop Entry]\nType=Application\n{body}\n"),
        )
    }
    fn env(&self) -> HostEnv {
        HostEnv::fixture(self.temp.path()).unwrap()
    }
    fn scan(&self, config: Config) -> ScanReport {
        let sources: Vec<Arc<dyn Source>> =
            vec![Arc::new(Desktop), Arc::new(Flatpak), Arc::new(Snap)];
        scan_with_sources(&self.env(), &config, &Cancel::new(), &sources).unwrap()
    }
    fn flatpak(&self, id: &str, category: &str) -> PathBuf {
        let root = format!("/var/lib/flatpak/app/{id}/current/active");
        self.file(
            &format!("{root}/metadata"),
            format!("[Application]\nname={id}\ncommand=fixture\n"),
        );
        self.file(&format!("{root}/files/share/metainfo/{id}.metainfo.xml"),format!("<component type=\"desktop-application\"><id>{id}</id><name>Default Game</name><name xml:lang=\"fr\">Jeu &amp; Test</name><categories><category>{category}</category></categories></component>"));
        self.exe(&format!("{root}/files/bin/fixture"));
        self.env().resolve_path(Path::new(&root)).unwrap()
    }
}
fn entries(report: &ScanReport) -> Vec<&DetectedGame> {
    report
        .groups
        .iter()
        .map(|g| g.primary_entry().unwrap())
        .collect()
}
fn titles(report: &ScanReport) -> Vec<&str> {
    let mut titles: Vec<_> = entries(report).iter().map(|g| g.title.as_str()).collect();
    titles.sort();
    titles
}

#[test]
fn game_category_localized_name_exec_field_codes_and_env_remain_literal() {
    let machine = Machine::new();
    let program = machine.exe("/home/fixture/Games/Space Quest/play");
    let icon = machine.file("/home/fixture/icons/game.png", b"fixture icon");
    let desktop=machine.desktop("space.desktop","Name=Space Quest\nName[fr]=Quête Spatiale\nCategories=Game;AdventureGame;\nPath=/home/fixture/Games/Space Quest\nIcon=/home/fixture/icons/game.png\nExec=env FIXTURE_FLAG=literal \"/home/fixture/Games/Space Quest/play\" --name %c %k %i %U %% \"$(never-run)\"");
    let report = machine.scan(Config::default());
    assert_eq!(titles(&report), ["Quête Spatiale"]);
    let game = entries(&report)[0];
    assert_eq!(game.source, SourceId::new("desktop"));
    assert_eq!(game.confidence, Confidence::Likely);
    assert_eq!(game.icon, Some(icon));
    let LaunchTarget::Exec {
        program: actual,
        args,
        cwd,
        env,
    } = &game.launch[0]
    else {
        panic!("exec target")
    };
    assert_eq!(actual, &program);
    assert_eq!(cwd.as_deref(), program.parent());
    assert_eq!(
        env,
        &BTreeMap::from([("FIXTURE_FLAG".into(), "literal".into())])
    );
    // Field replacements are data, not another shell/tokenization pass.
    assert_eq!(args[0], "--name");
    assert_eq!(args[1], "Quête Spatiale");
    assert_eq!(args[2], desktop.to_str().unwrap());
    assert_eq!(args[3], "--icon");
    assert_eq!(args[5], "%");
    assert_eq!(args[6], "$(never-run)");
    assert!(report.sources.iter().all(|s| s.error.is_none()));
}

#[test]
fn hidden_precedence_nodisplay_tryexec_and_types_are_honored() {
    let machine = Machine::new();
    machine.file(
        "/usr/share/applications/masked.desktop",
        b"[Desktop Entry]\nType=Application\nName=Masked\nCategories=Game;\nExec=masked\n",
    );
    machine.desktop("masked.desktop", "Hidden=true");
    machine.desktop(
        "nodisplay.desktop",
        "Name=NoDisplay\nCategories=Game;\nExec=nodisplay\nNoDisplay=true",
    );
    machine.desktop(
        "missing.desktop",
        "Name=Missing\nCategories=Game;\nExec=missing\nTryExec=unavailable",
    );
    machine.desktop(
        "available.desktop",
        "Name=Available\nCategories=Game;\nExec=fixture-game\nTryExec=fixture-game",
    );
    machine.exe("/usr/bin/fixture-game");
    machine.file("/usr/share/applications/link.desktop",b"[Desktop Entry]\nType=Link\nName=Website\nCategories=Game;\nURL=https://example.invalid\n");
    #[cfg(unix)]
    {
        machine.file("/usr/bin/nonexec", b"not executable");
        machine.desktop(
            "nonexec.desktop",
            "Name=Non executable\nCategories=Game;\nExec=nonexec\nTryExec=/usr/bin/nonexec",
        );
    }
    assert_eq!(titles(&machine.scan(Config::default())), ["Available"]);
}

#[test]
fn executable_names_sharing_system_bin_do_not_merge_different_games() {
    let machine = Machine::new();
    for name in ["alpha", "beta"] {
        machine.exe(&format!("/usr/bin/{name}"));
        machine.desktop(
            &format!("{name}.desktop"),
            &format!("Name={name}\nCategories=Game;\nExec={name}"),
        );
    }
    let report = machine.scan(Config::default());
    assert_eq!(titles(&report), ["alpha", "beta"]);
    assert!(
        entries(&report)
            .iter()
            .all(|entry| entry.install_dir.is_none())
    );
}

#[test]
fn emulator_only_entries_feed_the_reusable_probe_not_games() {
    let machine = Machine::new();
    machine.exe("/usr/bin/fixture-emu");
    machine.desktop("emu.desktop","Name=Fixture Emulator\nCategories=Game;Emulator;\nTryExec=fixture-emu\nExec=fixture-emu %f");
    assert!(machine.scan(Config::default()).groups.is_empty());
    let candidates =
        desktop::emulator_candidates(&machine.env(), &ScanCtx::new(Cancel::new())).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].title, "Fixture Emulator");
    assert!(candidates[0].emulator_only());
}

#[derive(Clone)]
struct Owners(Vec<DetectedGame>);
impl Source for Owners {
    fn id(&self) -> SourceId {
        SourceId::new("fixture-owners")
    }
    fn hosts(&self) -> OsSet {
        OsSet::ALL
    }
    fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn scan(&self, _: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        ctx.check()?;
        Ok(self.0.clone())
    }
    fn watch_paths(&self, _: &HostEnv) -> Vec<WatchPath> {
        vec![]
    }
}
#[test]
fn launcher_generated_shortcuts_only_enrich_their_owner_and_orphans_disappear() {
    let machine = Machine::new();
    let cases = [
        (
            "steam",
            "620",
            "steam steam://rungameid/620",
            "steam://rungameid/620",
        ),
        (
            "heroic",
            "fixture-app",
            "heroic heroic://launch/legendary/fixture-app",
            "heroic://launch/legendary/fixture-app",
        ),
        (
            "lutris",
            "fixture-slug",
            "lutris lutris:rungame/fixture-slug",
            "lutris:rungame/fixture-slug",
        ),
        (
            "itch",
            "12",
            "itch itch://caves/12/launch",
            "itch://caves/12/launch",
        ),
    ];
    let mut owners = vec![];
    for (source, key, exec, uri) in cases {
        machine.desktop(
            &format!("{source}.desktop"),
            &format!("Name=Shortcut {source}\nCategories=Game;\nExec={exec}"),
        );
        let mut game = DetectedGame::new(
            SourceId::from(source.to_owned()),
            key,
            format!("Owner {source}"),
        );
        game.launch.push(LaunchTarget::Uri { uri: uri.into() });
        owners.push(game);
    }
    assert!(machine.scan(Config::default()).groups.is_empty());
    let sources: Vec<Arc<dyn Source>> = vec![Arc::new(Owners(owners)), Arc::new(Desktop)];
    let report =
        scan_with_sources(&machine.env(), &Config::default(), &Cancel::new(), &sources).unwrap();
    assert_eq!(report.groups.len(), 4);
    for group in report.groups {
        assert_eq!(group.entries.len(), 2);
        let owner = group.primary_entry().unwrap();
        assert!(owner.title.starts_with("Owner "));
        assert_eq!(owner.confidence, Confidence::Certain);
        assert_eq!(owner.launch.len(), 1);
        assert!(
            !owner
                .evidence
                .iter()
                .any(|e| matches!(e,Evidence::Rule{rule,..} if rule==uri::PROXY_RULE))
        );
    }
}

#[test]
fn ordinary_apps_are_classified_only_for_opted_in_deep_scan() {
    let machine = Machine::new();
    machine.exe("/home/fixture/Games/Uncategorised/play");
    machine.file(
        "/home/fixture/Games/Uncategorised/fixture.pck",
        b"synthetic fingerprint",
    );
    machine.desktop(
        "uncategorised.desktop",
        "Name=Uncategorised Quest\nExec=/home/fixture/Games/Uncategorised/play",
    );
    assert!(machine.scan(Config::default()).groups.is_empty());
    let mut config = Config::default();
    config.deep.portable = true;
    let report = machine.scan(config);
    assert_eq!(titles(&report), ["Uncategorised Quest"]);
    assert!(
        entries(&report)[0]
            .evidence
            .iter()
            .any(|e| matches!(e,Evidence::Rule{rule,..} if rule=="fingerprint.godot"))
    );
}

#[test]
fn flatpak_metainfo_game_uses_flatpak_target_and_localized_name() {
    let machine = Machine::new();
    let root = machine.flatpak("org.example.FixtureGame", "Game");
    let report = machine.scan(Config::default());
    assert_eq!(titles(&report), ["Jeu & Test"]);
    let game = entries(&report)[0];
    assert_eq!(game.source, SourceId::new("flatpak"));
    assert_eq!(game.install_dir, Some(root.join("files")));
    assert_eq!(
        game.launch,
        [LaunchTarget::Flatpak {
            app_id: "org.example.FixtureGame".into(),
            args: vec![]
        }]
    );
    assert_eq!(game.confidence, Confidence::Likely);
}

#[test]
fn flatpak_exports_respect_masking_and_do_not_duplicate_metadata_games() {
    let machine = Machine::new();
    machine.flatpak("org.example.Visible", "Game");
    machine.flatpak("org.example.Hidden", "Game");
    machine.file("/var/lib/flatpak/exports/share/applications/org.example.Visible.Editor.desktop",b"[Desktop Entry]\nType=Application\nName=Hidden Level Editor\nNoDisplay=true\nX-Flatpak=org.example.Visible\nExec=flatpak run org.example.Visible\n");
    machine.file("/var/lib/flatpak/exports/share/applications/org.example.Visible.desktop",b"[Desktop Entry]\nType=Application\nName=Export Title\nCategories=Game;\nX-Flatpak=org.example.Visible\nExec=/usr/bin/flatpak run --branch=stable --command=fixture org.example.Visible --test %U\n");
    machine.file("/var/lib/flatpak/exports/share/applications/org.example.Hidden.desktop",b"[Desktop Entry]\nType=Application\nName=Hidden Export\nCategories=Game;\nX-Flatpak=org.example.Hidden\nExec=flatpak run org.example.Hidden\n");
    machine.desktop("org.example.Hidden.desktop", "Hidden=true");
    let report = machine.scan(Config::default());
    assert_eq!(titles(&report), ["Export Title"]);
    assert_eq!(
        entries(&report)[0].launch,
        [LaunchTarget::Flatpak {
            app_id: "org.example.Visible".into(),
            args: vec!["--test".into()]
        }]
    );
}

#[test]
fn flatpak_runtime_non_game_and_bad_metadata_never_become_games() {
    let machine = Machine::new();
    machine.flatpak("org.example.Office", "Office");
    machine.flatpak("org.example.Broken", "Game");
    machine.file(
        "/var/lib/flatpak/app/org.example.Broken/current/active/metadata",
        b"[Runtime]\nname=org.example.Broken\n",
    );
    machine.file(
        "/var/lib/flatpak/runtime/org.example.Runtime/current/active/metadata",
        b"[Application]\nname=org.example.Runtime\n",
    );
    machine.flatpak("org.example.Emulator", "Emulator");
    assert!(machine.scan(Config::default()).groups.is_empty());
}

#[test]
fn extra_flatpak_installation_is_read_without_a_flatpak_process() {
    let machine = Machine::new();
    machine.file(
        "/etc/flatpak/installations.d/games.conf",
        b"[Installation \"games\"]\nPath=/opt/flatpak-games\n",
    );
    machine.file(
        "/opt/flatpak-games/app/org.example.Custom/current/active/metadata",
        b"[Application]\nname=org.example.Custom\n",
    );
    machine.file("/opt/flatpak-games/app/org.example.Custom/current/active/files/share/metainfo/org.example.Custom.metainfo.xml",b"<component><id>org.example.Custom</id><name>Custom Game</name><categories><category>Game</category></categories></component>");
    assert_eq!(titles(&machine.scan(Config::default())), ["Custom Game"]);
}

#[test]
fn snap_exports_and_integrated_appimages_use_exact_exec_argv() {
    let machine = Machine::new();
    let snap = machine.exe("/snap/bin/fixture-game");
    machine.file("/var/lib/snapd/desktop/applications/fixture-game_game.desktop",b"[Desktop Entry]\nType=Application\nName=Snap Game\nX-SnapInstanceName=fixture-game\nCategories=Game;\nExec=env BAMF_DESKTOP_FILE_HINT=/var/lib/snapd/desktop/applications/fixture-game_game.desktop /snap/bin/fixture-game --safe %U\n");
    for name in ["One", "Two"] {
        machine.exe(&format!("/home/fixture/Applications/{name}.AppImage"));
        machine.desktop(&format!("{name}.desktop"),&format!("Name=AppImage {name}\nCategories=Game;\nExec=/home/fixture/Applications/{name}.AppImage --play"));
    }
    let report = machine.scan(Config::default());
    assert_eq!(
        titles(&report),
        ["AppImage One", "AppImage Two", "Snap Game"]
    );
    let game = entries(&report)
        .into_iter()
        .find(|e| e.title == "Snap Game")
        .unwrap();
    assert_eq!(game.source, SourceId::new("snap"));
    assert!(
        matches!(&game.launch[0],LaunchTarget::Exec{program,args,env,..} if program==&snap && args==&["--safe"] && env.get("BAMF_DESKTOP_FILE_HINT").is_some())
    );
}

#[test]
fn malformed_entries_and_xml_are_bounded_warnings_while_valid_siblings_survive() {
    let machine = Machine::new();
    machine.desktop(
        "valid.desktop",
        "Name=Valid Game\nCategories=Game;\nExec=valid",
    );
    machine.desktop("bad.desktop", "Name=Bad\nExec=\"unterminated");
    machine.desktop("large.desktop", &"x".repeat(1024 * 1024 + 1));
    let root = machine.flatpak("org.example.XmlBomb", "Game");
    fs::write(
        root.join("files/share/metainfo/org.example.XmlBomb.metainfo.xml"),
        format!(
            "<component>{}<name>Too deep</name>{}</component>",
            "<nested>".repeat(40),
            "</nested>".repeat(40)
        ),
    )
    .unwrap();
    let report = machine.scan(Config::default());
    assert_eq!(titles(&report), ["Valid Game"]);
    assert!(report.sources.iter().any(|s| !s.warnings.is_empty()));
    assert!(report.sources.iter().all(|s| s.error.is_none()));
}

#[test]
fn cancellation_stops_all_three_sources_and_watch_paths_are_fixture_confined() {
    let machine = Machine::new();
    let env = machine.env();
    let cancel = Cancel::new();
    cancel.cancel();
    let ctx = ScanCtx::new(cancel);
    for source in [&Desktop as &dyn Source, &Flatpak, &Snap] {
        assert!(matches!(
            source.scan(&env, &ctx),
            Err(SourceError::Cancelled)
        ));
        assert!(
            source
                .watch_paths(&env)
                .iter()
                .all(|w| w.path.starts_with(env.fs_root()))
        );
    }
}

#[cfg(unix)]
#[test]
fn flatpak_links_inside_fixture_work_but_export_links_cannot_read_host_files() {
    let machine = Machine::new();
    let root = machine.flatpak("org.example.Linked", "Game");
    let parent = root.parent().unwrap();
    fs::rename(&root, parent.join("commit")).unwrap();
    std::os::unix::fs::symlink("commit", parent.join("active")).unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(
        outside.path(),
        b"[Desktop Entry]\nType=Application\nName=Outside Game\nCategories=Game;\nExec=outside\n",
    )
    .unwrap();
    let dir = machine
        .temp
        .path()
        .join("root/home/fixture/.local/share/applications");
    fs::create_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.join("escape.desktop")).unwrap();
    let report = machine.scan(Config::default());
    assert_eq!(titles(&report), ["Jeu & Test"]);
    assert!(
        report
            .sources
            .iter()
            .any(|s| s.warnings.iter().any(|w| w.contains("escape.desktop")))
    );
}
