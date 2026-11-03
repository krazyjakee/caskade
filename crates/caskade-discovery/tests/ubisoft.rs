#![cfg(feature = "windows-sources")]
//! Synthetic, local Ubisoft records, never an installed launcher/account.
use crate::fixtures::write;
use caskade_discovery::{
    registry::*,
    sources::ubisoft::{UbisoftConnect, cache::parse_configurations},
    *,
};
use std::{fs, path::Path, sync::Arc};
fn scenario() -> tempfile::TempDir {
    let t = crate::fixtures::tempdir();
    fs::create_dir(t.path().join("root")).unwrap();
    write(t.path(), "machine.toml", "schema_version=1\nos='windows'\n");
    t
}
fn key(view: RegistryView, path: &str) -> RegistryKey {
    RegistryKey::new(RegistryHive::LocalMachine, view, path)
}
fn insert(reg: &mut MemoryRegistry, view: RegistryView, path: &str, name: &str, value: &str) {
    reg.insert(key(view, path), name, RegistryValue::String(value.into()));
}
fn installed(reg: &mut MemoryRegistry, id: u64, name: &str, view: RegistryView) {
    insert(
        reg,
        view,
        &format!(r"SOFTWARE\Ubisoft\Launcher\Installs\{id}"),
        "InstallDir",
        &format!(r"C:\Games\{name}"),
    );
}
fn uninstall(reg: &mut MemoryRegistry, id: u64, title: &str, dir: &str, view: RegistryView) {
    let path = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Uplay Install {id}");
    for (name, value) in [
        ("DisplayName", title),
        ("InstallLocation", dir),
        ("UninstallString", &format!("uplay://uninstall/{id}")),
    ] {
        insert(reg, view, &path, name, value);
    }
}
fn integer(mut value: u64) -> Vec<u8> {
    let mut bytes = vec![];
    while value > 127 {
        bytes.push(value as u8 | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}
fn field(number: u8, value: &[u8]) -> Vec<u8> {
    let mut bytes = integer(u64::from(number) << 3 | 2);
    bytes.extend(integer(value.len() as u64));
    bytes.extend(value);
    bytes
}
fn record(id: u64, install_id: u64, yaml: &str) -> Vec<u8> {
    let mut bytes = vec![8];
    bytes.extend(integer(id));
    bytes.push(16);
    bytes.extend(integer(install_id));
    bytes.extend(field(3, yaml.as_bytes()));
    field(1, &bytes)
}
fn cache() -> Vec<u8> {
    let mut b = record(
        11,
        101,
        "version: 2.0\nroot:\n  name: cache.first\nlocalizations:\n  default:\n    cache.first: 'Cache First'\n",
    );
    b.extend(record(
        22,
        202,
        "version: 2.0\nroot:\n  name: 'Cache Second Café'\n",
    ));
    b.extend(record(
        33,
        303,
        "root:\n  name: 'Owned but not installed'\n",
    ));
    b
}
fn env(root: &Path, registry: MemoryRegistry) -> HostEnv {
    HostEnv::fixture(root)
        .unwrap()
        .with_registry(Arc::new(registry))
}
fn scan(env: &HostEnv) -> Vec<DetectedGame> {
    UbisoftConnect.scan(env, &ScanCtx::default()).unwrap()
}
#[test]
fn two_installed_games_title_from_uninstall_then_cache_and_no_owned_only_games() {
    let temp = scenario();
    let mut registry = MemoryRegistry::new();
    for (id, name, view) in [
        (101, "First", RegistryView::Registry32),
        (202, "Second", RegistryView::Registry64),
    ] {
        installed(&mut registry, id, name, view);
        fs::create_dir_all(temp.path().join(format!("root/drives/c/Games/{name}"))).unwrap();
    }
    uninstall(
        &mut registry,
        101,
        "Uninstall First",
        r"C:\Games\First",
        RegistryView::Registry32,
    );
    uninstall(
        &mut registry,
        202,
        "",
        r"C:\Games\Second",
        RegistryView::Registry64,
    );
    write(
        temp.path(),
        "root/drives/c/Users/fixture/AppData/Local/Ubisoft Game Launcher/cache/configuration/configurations",
        cache(),
    );
    write(
        temp.path(),
        "root/drives/c/Program Files (x86)/Ubisoft/Ubisoft Game Launcher/UbisoftConnect.exe",
        b"client",
    );
    let env = env(temp.path(), registry);
    let games = scan(&env);
    assert_eq!(games.len(), 2, "{games:?}");
    assert_eq!(games[0].title, "Uninstall First");
    assert_eq!(games[1].title, "Cache Second Café");
    for (game, id) in games.iter().zip([101, 202]) {
        assert_eq!(game.ids, [ExternalId::Ubisoft { id }]);
        assert_eq!(
            game.launch,
            [LaunchTarget::Uri {
                uri: format!("uplay://launch/{id}/0")
            }]
        );
        assert_eq!(game.state, InstallState::Installed);
        assert_eq!(
            game.uninstall.as_ref().unwrap().target,
            LaunchTarget::Uri {
                uri: format!("uplay://uninstall/{id}")
            }
        );
        assert!(matches!(game.runtime, GameRuntime::Windows { .. }));
    }
    assert!(UbisoftConnect.detect(&env).iter().any(|c| c.found));
    assert!(
        UbisoftConnect
            .watch_paths(&env)
            .iter()
            .any(|w| w.path.ends_with("configurations") && !w.recursive)
    );
    let report = scan_with_sources(
        &env,
        &Config::default(),
        &Cancel::new(),
        &[Arc::new(UbisoftConnect)],
    )
    .unwrap();
    assert_eq!(report.groups.len(), 2);
    assert!(report.sources.iter().all(|s| s.error.is_none()));
}
#[test]
fn registry_views_and_duplicate_uninstall_records_yield_one_game_per_id() {
    let temp = scenario();
    let mut registry = MemoryRegistry::new();
    for view in [RegistryView::Registry32, RegistryView::Registry64] {
        installed(&mut registry, 42, "Café", view);
        uninstall(&mut registry, 42, "Fixture", r"C:\Games\Café", view);
    }
    fs::create_dir_all(temp.path().join("root/drives/c/Games/Café")).unwrap();
    let games = scan(&env(temp.path(), registry));
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].title, "Fixture");
    assert!(
        games[0]
            .evidence
            .iter()
            .filter(|e| matches!(e,Evidence::Registry{value:Some(n),..} if n=="InstallDir"))
            .count()
            >= 2
    );
}
#[test]
fn actual_uninstall_source_merges_both_registry_games_and_orphans_drop() {
    use caskade_discovery::sources::uninstall::UninstallSource;
    let temp = scenario();
    let mut registry = MemoryRegistry::new();
    for id in [101, 202] {
        installed(&mut registry, id, &id.to_string(), RegistryView::Registry32);
        uninstall(
            &mut registry,
            id,
            &format!("Game {id}"),
            &format!(r"C:\Games\{id}"),
            RegistryView::Registry32,
        );
        fs::create_dir_all(temp.path().join(format!("root/drives/c/Games/{id}"))).unwrap();
    }
    uninstall(
        &mut registry,
        999,
        "Not installed",
        r"C:\Missing",
        RegistryView::Registry64,
    );
    // Omit the orphan's path so Ubisoft has no install record for it. The
    // launcher-owned uninstall alias remains visible only to UninstallSource.
    registry.insert(
        key(
            RegistryView::Registry64,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Uplay Install 999",
        ),
        "InstallLocation",
        RegistryValue::Dword(0),
    );
    let env = env(temp.path(), registry);
    let raw = UninstallSource::new()
        .scan(&env, &ScanCtx::default())
        .unwrap();
    assert_eq!(raw.len(), 3);
    assert!(raw.iter().all(|g| {
        g.evidence
            .iter()
            .any(|e| matches!(e,Evidence::Rule{rule,..} if rule=="alias.launcher_owned"))
    }));
    let report = scan_with_sources(
        &env,
        &Config::default(),
        &Cancel::new(),
        &[Arc::new(UbisoftConnect), Arc::new(UninstallSource::new())],
    )
    .unwrap();
    assert!(
        report.sources.iter().all(|s| s.error.is_none()),
        "{report:?}"
    );
    assert_eq!(report.groups.len(), 2, "{report:?}");
    for group in report.groups {
        assert_eq!(group.entries.len(), 2);
        assert_eq!(
            group.primary_entry().unwrap().source,
            SourceId::new("ubisoft")
        );
        assert!(
            group
                .entries
                .iter()
                .any(|g| g.source.as_str() == "uninstall")
        );
    }
}
#[test]
fn missing_folders_and_malformed_cache_preserve_registry_games() {
    let temp = scenario();
    let mut registry = MemoryRegistry::new();
    installed(&mut registry, 42, "Missing", RegistryView::Registry32);
    write(
        temp.path(),
        "root/drives/c/Users/fixture/AppData/Local/Ubisoft Game Launcher/cache/configuration/configurations",
        b"\x0a\xff\xff",
    );
    let env = env(temp.path(), registry);
    let ctx = ScanCtx::default();
    let games = UbisoftConnect.scan(&env, &ctx).unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].title, "Missing");
    assert!(matches!(games[0].state, InstallState::Broken { .. }));
    assert_eq!(games[0].confidence, Confidence::Maybe);
    assert!(!ctx.take_warnings().is_empty());
}
#[test]
fn uninstall_only_record_and_client_registry_work_without_default_folders() {
    let temp = scenario();
    let mut registry = MemoryRegistry::new();
    uninstall(
        &mut registry,
        42,
        "Remembered game",
        r"D:\Library\Game",
        RegistryView::Registry32,
    );
    insert(
        &mut registry,
        RegistryView::Registry32,
        r"SOFTWARE\Ubisoft\Launcher",
        "InstallDir",
        r"D:\Clients\Ubi",
    );
    write(temp.path(), "root/drives/d/Clients/Ubi/upc.exe", b"client");
    fs::create_dir_all(temp.path().join("root/drives/d/Library/Game")).unwrap();
    write(
        temp.path(),
        "root/drives/d/Clients/Ubi/cache/configuration/configurations",
        record(
            42,
            42,
            "root:\n  name: Cache ignored because uninstall title exists\n",
        ),
    );
    let env = env(temp.path(), registry);
    let games = scan(&env);
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].title, "Remembered game");
    assert!(UbisoftConnect.detect(&env)[0].found);
}
#[test]
fn cache_wire_unknown_fields_localized_yaml_dlcs_and_corrupt_records() {
    let mut bytes = cache();
    bytes.extend(field(27, b"unknown future data"));
    bytes.extend(record(44, 404, "root:\n  name: DLC\n  is_ulc: true\n"));
    bytes.extend(record(55, 505, "root: [invalid"));
    let result = parse_configurations(&bytes).unwrap();
    assert_eq!(result.games.len(), 3);
    assert_eq!(result.games[0].title, "Cache First");
    assert_eq!(result.games[1].install_id, 202);
    assert_eq!(result.warnings.len(), 1);
    let plain = parse_configurations(
        b"version: 2.0\nuplay_id: 42\ninstall_id: 420\nroot:\n  name: 'Fixture & Co'\n",
    )
    .unwrap();
    assert_eq!(plain.games[0].id, 42);
    assert_eq!(plain.games[0].install_id, 420);
}
#[test]
fn hostile_lengths_yaml_expansion_and_wire_types_are_rejected_before_allocating() {
    for bytes in [
        b"\x0a\xff\xff\xff\xff\x7f".as_slice(),
        b"\x0b",
        b"\x00",
        b"\x0a\x05bad",
        b"\x80\x80\x80\x80\x80\x80\x80\x80\x80\xff",
    ] {
        assert!(parse_configurations(bytes).is_err(), "{bytes:?}");
    }
    assert!(parse_configurations(&vec![0; (8 << 20) + 1]).is_err());
    for text in [
        "root: &a {name: Fixture}\nother: *a",
        "root: {name: !custom Fixture}",
        "root:\n  name: '\u{0}'",
    ] {
        assert!(parse_configurations(text.as_bytes()).is_err());
    }
    let input = format!("root: {}0{}", "[".repeat(33), "]".repeat(33));
    assert!(parse_configurations(input.as_bytes()).is_err());
}
#[test]
fn empty_fixture_and_cancel_never_scan_the_live_machine() {
    let temp = scenario();
    let env = HostEnv::fixture(temp.path()).unwrap();
    assert!(scan(&env).is_empty());
    assert!(!UbisoftConnect.launcher_locations(&env).is_empty());
    assert!(
        UbisoftConnect
            .launcher_locations(&env)
            .iter()
            .all(|p| p.starts_with(env.fs_root()))
    );
    assert!(UbisoftConnect.detect(&env).iter().all(|s| !s.found));
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(matches!(
        UbisoftConnect.scan(&env, &ScanCtx::new(cancel)),
        Err(SourceError::Cancelled)
    ));
}
#[test]
#[cfg(unix)]
fn cache_symlink_cannot_read_outside_fixture() {
    let temp = scenario();
    let outside = crate::fixtures::tempdir();
    write(outside.path(), "configurations", cache());
    let path = temp.path().join(
        "root/drives/c/Users/fixture/AppData/Local/Ubisoft Game Launcher/cache/configuration",
    );
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(outside.path(), path).unwrap();
    assert!(scan(&HostEnv::fixture(temp.path()).unwrap()).is_empty());
}
proptest::proptest! {
 #[test]
 fn arbitrary_cache_bytes_never_panic(bytes in proptest::collection::vec(proptest::num::u8::ANY,0..4096)) {let _=parse_configurations(&bytes);}
}
