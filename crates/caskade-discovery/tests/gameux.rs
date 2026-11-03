#![cfg(feature = "windows-sources")]
use caskade_discovery::{
    registry::{RegistryValue as Value, *},
    sources::gameux::{GameUx, ID, REGISTRY_PATH},
    *,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
const GUID: &str = "{11111111-2222-3333-4444-555555555555}";
const SID: &str = "S-1-5-21-424242-424242-424242-1000";
const INSTALL: &str = r"C:\Games\Fixture2009";
struct Machine {
    temp: tempfile::TempDir,
}
impl Machine {
    fn new() -> Self {
        let temp = crate::fixtures::tempdir();
        fs::create_dir_all(temp.path().join("root/drives/c")).unwrap();
        fs::write(
            temp.path().join("machine.toml"),
            "schema_version=1\nos=\"windows\"\n",
        )
        .unwrap();
        Self { temp }
    }
    fn file(&self, path: &str) -> PathBuf {
        let p = self.env().resolve_path(Path::new(path)).unwrap();
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"original synthetic metadata/payload; never executed").unwrap();
        p
    }
    fn env(&self) -> HostEnv {
        HostEnv::fixture(self.temp.path()).unwrap()
    }
    fn payload(&self) {
        self.file(&format!(r"{INSTALL}\play.exe"));
        self.file(&format!(r"{INSTALL}\GameDefinition.dll"));
    }
    fn key(&self, hive: RegistryHive, view: RegistryView, suffix: &str) -> RegistryKey {
        RegistryKey::new(hive, view, format!(r"{REGISTRY_PATH}\{suffix}\{GUID}"))
    }
    fn registry(&self, hive: RegistryHive, view: RegistryView, suffix: &str) -> MemoryRegistry {
        let key = self.key(hive, view, suffix);
        let mut r = MemoryRegistry::new();
        for (name, value) in [
            ("Title", Value::String("Hidden & Dangerous 2".into())),
            ("ConfigApplicationPath", Value::String(INSTALL.into())),
            ("AppExePath", Value::String(format!(r"{INSTALL}\play.exe"))),
            (
                "ConfigGDFBinaryPath",
                Value::String(format!(r"{INSTALL}\GameDefinition.dll")),
            ),
        ] {
            r.insert(key.clone(), name, value)
        }
        r
    }
    fn scan(&self, r: MemoryRegistry) -> Vec<DetectedGame> {
        GameUx
            .scan(
                &self.env().with_registry(Arc::new(r)),
                &ScanCtx::new(Cancel::new()),
            )
            .unwrap()
    }
}

#[test]
fn original_retail_era_fixture_has_title_directory_exe_and_gdf_reference() {
    let m = Machine::new();
    m.payload();
    let games = m.scan(m.registry(
        RegistryHive::LocalMachine,
        RegistryView::Registry64,
        "Games",
    ));
    assert_eq!(games.len(), 1);
    let game = &games[0];
    assert_eq!(game.title, "Hidden & Dangerous 2");
    assert_eq!(game.source, ID);
    assert_eq!(game.confidence, Confidence::Certain);
    assert_eq!(game.state, InstallState::Installed);
    assert_eq!(
        game.runtime,
        GameRuntime::Windows {
            prefix: None,
            runner: None
        }
    );
    let env = m.env();
    let dir = env.resolve_path(Path::new(INSTALL)).unwrap();
    assert_eq!(game.install_dir, Some(dir.clone()));
    assert_eq!(
        game.launch,
        [LaunchTarget::Exec {
            program: dir.join("play.exe"),
            args: vec![],
            cwd: Some(dir.clone()),
            env: BTreeMap::new()
        }]
    );
    assert!(game.evidence.contains(&Evidence::File {
        path: dir.join("GameDefinition.dll")
    }));
    assert!(game.evidence.iter().any(|e|matches!(e,Evidence::Registry{key,value} if key.contains("GameUX") && value.as_deref()==Some("Title"))));
    assert_eq!(game.tracking.as_ref().unwrap().exe_names, ["play.exe"]);
}

#[test]
fn machine_sid_current_user_and_loaded_user_hives_support_both_views() {
    let m = Machine::new();
    m.payload();
    for (hive, view, suffix) in [
        (
            RegistryHive::LocalMachine,
            RegistryView::Registry32,
            "Games",
        ),
        (RegistryHive::LocalMachine, RegistryView::Registry64, SID),
        (RegistryHive::CurrentUser, RegistryView::Registry64, "Games"),
    ] {
        assert_eq!(
            m.scan(m.registry(hive, view, suffix)).len(),
            1,
            "{hive:?} {view:?} {suffix}"
        );
    }
    let mut r = MemoryRegistry::new();
    let source = m.registry(
        RegistryHive::LocalMachine,
        RegistryView::Registry64,
        "Games",
    );
    let values = source
        .values(&m.key(
            RegistryHive::LocalMachine,
            RegistryView::Registry64,
            "Games",
        ))
        .unwrap();
    let key = RegistryKey::new(
        RegistryHive::Users,
        RegistryView::Registry64,
        format!(r"{SID}\{REGISTRY_PATH}\Games\{GUID}"),
    );
    for entry in values {
        r.insert(key.clone(), entry.name, entry.value)
    }
    assert_eq!(m.scan(r).len(), 1);
}

#[test]
fn wine_registry_fixture_is_consumed_by_the_same_source() {
    let m = Machine::new();
    m.payload();
    let registry = m.temp.path().join("registry");
    fs::create_dir(&registry).unwrap();
    let path = format!(r"{REGISTRY_PATH}\Games\{GUID}").replace('\\', "\\\\");
    let text = format!(
        "WINE REGISTRY Version 2\n#arch=win64\n[{path}] 1700000000\n\"Title\"=\"Hidden & Dangerous 2\"\n\"ConfigApplicationPath\"=\"C:\\\\Games\\\\Fixture2009\"\n\"AppExePath\"=\"C:\\\\Games\\\\Fixture2009\\\\play.exe\"\n\"ConfigGDFBinaryPath\"=\"C:\\\\Games\\\\Fixture2009\\\\GameDefinition.dll\"\n"
    );
    let file = registry.join("hklm64.reg");
    fs::write(&file, text.as_bytes()).unwrap();
    let env = m.env();
    let games = GameUx.scan(&env, &ScanCtx::new(Cancel::new())).unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].state, InstallState::Installed);
    assert_eq!(fs::read(file).unwrap(), text.as_bytes());
}

#[test]
fn expandable_and_relative_paths_are_resolved_in_the_fixture_namespace() {
    let m = Machine::new();
    m.file(r"C:\Users\fixture\Games\Fixture2009\play.exe");
    let key = m.key(
        RegistryHive::LocalMachine,
        RegistryView::Registry64,
        "Games",
    );
    let mut r = m.registry(
        RegistryHive::LocalMachine,
        RegistryView::Registry64,
        "Games",
    );
    r.insert(
        key.clone(),
        "ConfigApplicationPath",
        Value::ExpandString(r"%USERPROFILE%\Games\Fixture2009".into()),
    );
    r.insert(key, "AppExePath", Value::String("play.exe".into()));
    let games = m.scan(r);
    assert_eq!(games[0].state, InstallState::Installed);
    assert!(
        games[0]
            .install_dir
            .as_ref()
            .unwrap()
            .ends_with("Users/fixture/Games/Fixture2009")
    );
    assert!(
        matches!(&games[0].launch[0],LaunchTarget::Exec{program,..} if program.starts_with(m.env().fs_root()) && program.ends_with("play.exe"))
    );
}

#[test]
fn stale_registrations_are_broken_and_a_gdf_dll_is_never_a_launch_target() {
    let m = Machine::new();
    let r = m.registry(
        RegistryHive::LocalMachine,
        RegistryView::Registry64,
        "Games",
    );
    let games = m.scan(r);
    assert!(matches!(games[0].state, InstallState::Broken { .. }));
    m.payload();
    let mut r = MemoryRegistry::new();
    let key = m.key(
        RegistryHive::LocalMachine,
        RegistryView::Registry64,
        "Games",
    );
    r.insert(
        key.clone(),
        "Title",
        Value::String("DLL-only registration".into()),
    );
    r.insert(
        key.clone(),
        "ConfigApplicationPath",
        Value::String(INSTALL.into()),
    );
    r.insert(
        key,
        "ConfigGDFBinaryPath",
        Value::String(format!(r"{INSTALL}\GameDefinition.dll")),
    );
    let games = m.scan(r);
    assert!(games[0].launch.is_empty());
    assert!(matches!(games[0].state, InstallState::Broken { .. }));
}

fn link(target: &str, args: &str, cwd: &str) -> Vec<u8> {
    // Original minimal Unicode MS-SHLLINK writer, with StringData only.
    let mut bytes = vec![0; 76];
    bytes[..4].copy_from_slice(&76u32.to_le_bytes());
    bytes[4..20].copy_from_slice(&[1, 20, 2, 0, 0, 0, 0, 0, 192, 0, 0, 0, 0, 0, 0, 70]);
    bytes[20..24].copy_from_slice(&(0x80u32 | 0x08 | 0x10 | 0x20).to_le_bytes());
    bytes[60..64].copy_from_slice(&1u32.to_le_bytes());
    for text in [target, cwd, args] {
        let units: Vec<_> = text.encode_utf16().collect();
        bytes.extend((units.len() as u16).to_le_bytes());
        bytes.extend(units.into_iter().flat_map(u16::to_le_bytes));
    }
    bytes.extend(0u32.to_le_bytes());
    bytes
}
#[test]
fn cached_primary_play_task_supplies_exe_arguments_and_working_directory() {
    let m = Machine::new();
    m.payload();
    let key = m.key(
        RegistryHive::LocalMachine,
        RegistryView::Registry64,
        "Games",
    );
    let mut r = MemoryRegistry::new();
    for (name, value) in [
        ("Title", Value::String("Task Fixture".into())),
        ("ConfigApplicationPath", Value::String(INSTALL.into())),
        (
            "ConfigGDFBinaryPath",
            Value::String(format!(r"{INSTALL}\GameDefinition.dll")),
        ),
    ] {
        r.insert(key.clone(), name, value)
    }
    let base = m
        .env()
        .known_folder(env::KnownFolder::ProgramData)
        .unwrap()
        .join(format!("Microsoft/Windows/GameExplorer/{GUID}/PlayTasks/0"));
    fs::create_dir_all(&base).unwrap();
    let file = base.join("Play.lnk");
    fs::write(
        &file,
        link(
            &format!(r"{INSTALL}\play.exe"),
            "--campaign \"space quest\"",
            INSTALL,
        ),
    )
    .unwrap();
    let games = m.scan(r);
    assert_eq!(games[0].state, InstallState::Installed);
    assert!(
        matches!(&games[0].launch[0],LaunchTarget::Exec{program,args,cwd,..} if program.ends_with("play.exe") && args==&["--campaign","space quest"] && cwd.as_ref()==games[0].install_dir.as_ref())
    );
    assert!(games[0].evidence.contains(&Evidence::File { path: file }));
}

#[test]
fn malformed_fields_and_upgrade_hints_do_not_invent_registered_games() {
    let m = Machine::new();
    m.payload();
    let mut r = m.registry(
        RegistryHive::LocalMachine,
        RegistryView::Registry64,
        "GamesToFindOnWindowsUpgrade",
    );
    let key = m.key(
        RegistryHive::LocalMachine,
        RegistryView::Registry64,
        "Games",
    );
    r.insert(key.clone(), "Title", Value::Binary(vec![1, 2]));
    r.insert(
        key,
        "AppExePath",
        Value::String("https://example.invalid/game.exe".into()),
    );
    let ctx = ScanCtx::new(Cancel::new());
    assert!(
        GameUx
            .scan(&m.env().with_registry(Arc::new(r)), &ctx)
            .unwrap()
            .is_empty()
    );
    assert!(!ctx.take_warnings().is_empty());
}

#[test]
fn canonical_install_directory_merges_uninstall_metadata_without_title_matching() {
    let m = Machine::new();
    m.payload();
    let env = m.env().with_registry(Arc::new(m.registry(
        RegistryHive::LocalMachine,
        RegistryView::Registry64,
        "Games",
    )));
    let mut game = GameUx
        .scan(&env, &ScanCtx::new(Cancel::new()))
        .unwrap()
        .remove(0);
    let mut uninstall = DetectedGame::new(
        SourceId::new("uninstall"),
        "retail-fixture",
        "Different installed-program label",
    );
    uninstall.confidence = Confidence::Likely;
    uninstall.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    uninstall.install_dir = Some(PathBuf::from(r"c:/games/FIXTURE2009"));
    uninstall.uninstall = Some(UninstallInfo {
        target: LaunchTarget::Exec {
            program: PathBuf::from(r"C:\Games\Fixture2009\uninstall.exe"),
            args: vec!["/quiet".into()],
            cwd: None,
            env: BTreeMap::new(),
        },
        quiet: None,
        registry_key: Some("original synthetic uninstall key".into()),
    });
    let merged = merge::merge(
        &env,
        vec![game.clone(), uninstall.clone()],
        &merge::MergeConfig::default(),
    )
    .unwrap();
    let dirs: Vec<Vec<_>> = merged
        .iter()
        .map(|group| group.entries.iter().map(|e| &e.install_dir).collect())
        .collect();
    assert_eq!(merged.len(), 1, "{dirs:?}");
    assert_eq!(merged[0].entries.len(), 2);
    assert_eq!(merged[0].primary_entry().unwrap().source, ID);
    assert!(merged[0].primary_entry().unwrap().uninstall.is_some());
    game.install_dir = Some(PathBuf::from(r"C:\Games\AnotherCopy"));
    game.launch.clear();
    uninstall.title = game.title.clone();
    assert_eq!(
        merge::merge(&env, vec![game, uninstall], &merge::MergeConfig::default())
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn registry_failures_are_warnings_and_precancellation_stops_before_enumeration() {
    struct Denied;
    impl Registry for Denied {
        fn key_exists(&self, _: &RegistryKey) -> std::io::Result<bool> {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
        fn subkeys(&self, _: &RegistryKey) -> std::io::Result<Vec<String>> {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
        fn values(&self, _: &RegistryKey) -> std::io::Result<Vec<RegistryEntry>> {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
    }
    let m = Machine::new();
    let env = m.env().with_registry(Arc::new(Denied));
    let ctx = ScanCtx::new(Cancel::new());
    assert!(GameUx.scan(&env, &ctx).unwrap().is_empty());
    assert!(!ctx.take_warnings().is_empty());
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(matches!(
        GameUx.scan(&env, &ScanCtx::new(cancel)),
        Err(SourceError::Cancelled)
    ));
    assert!(
        GameUx
            .watch_paths(&env)
            .iter()
            .all(|p| p.path.starts_with(env.fs_root()))
    );
}

#[test]
fn oversized_registry_enumeration_is_a_bounded_source_error() {
    struct Oversized;
    impl Registry for Oversized {
        fn key_exists(&self, _: &RegistryKey) -> std::io::Result<bool> {
            Ok(true)
        }
        fn subkeys(&self, _: &RegistryKey) -> std::io::Result<Vec<String>> {
            Ok((0..5000).map(|i| format!("S-1-5-{i}")).collect())
        }
        fn values(&self, _: &RegistryKey) -> std::io::Result<Vec<RegistryEntry>> {
            unreachable!("enumeration must stop before reading records")
        }
    }
    let m = Machine::new();
    assert!(matches!(
        GameUx.scan(
            &m.env().with_registry(Arc::new(Oversized)),
            &ScanCtx::new(Cancel::new())
        ),
        Err(SourceError::Other { .. })
    ));
}
