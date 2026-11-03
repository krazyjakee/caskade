#![cfg(feature = "windows-sources")]
//! Authored EA/Origin fixtures: no launcher installation or personal data.
use crate::fixtures::write;
use caskade_discovery::{
    registry::{MemoryRegistry, RegistryHive, RegistryKey, RegistryValue, RegistryView},
    sources::ea::{
        Ea,
        metadata::{parse_installer_xml, parse_origin_manifest},
    },
    *,
};
use std::{fs, path::Path, sync::Arc};

const XML: &str = r#"<?xml version="1.0"?><game>
<contentIDs><contentID>EA.OFR.Fixture</contentID><contentID>10001</contentID></contentIDs>
<gameTitles><gameTitle locale="de_DE">Anderer Titel</gameTitle><gameTitle locale="en_US">Fixture EA &amp; Friends</gameTitle></gameTitles>
<runtime><launcher><filePath>__InstallLoc__\bin\Game.exe</filePath></launcher></runtime></game>"#;
fn scenario() -> tempfile::TempDir {
    let temp = crate::fixtures::tempdir();
    fs::create_dir(temp.path().join("root")).unwrap();
    write(
        temp.path(),
        "machine.toml",
        "schema_version=1\nos='windows'\n",
    );
    temp
}
fn add_game(root: &Path, name: &str, xml: &str) {
    write(
        root,
        &format!("root/drives/c/Games/{name}/__Installer/installerdata.xml"),
        xml,
    );
    write(
        root,
        &format!("root/drives/c/Games/{name}/bin/Game.exe"),
        b"authored fixture executable",
    );
}
fn memory(game: &str, view: RegistryView) -> MemoryRegistry {
    let mut registry = MemoryRegistry::new();
    registry.insert(
        RegistryKey::new(
            RegistryHive::LocalMachine,
            view,
            r"SOFTWARE\EA Games\Fixture",
        ),
        "Install Dir",
        RegistryValue::String(format!(r"C:\Games\{game}")),
    );
    registry
}
fn origin(root: &Path, name: &str, offer: &str, state: &str) {
    let bytes = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("dipInstallPath", "")
        .append_pair("dipinstallpath", &format!(r"C:\Games\{name}"))
        .append_pair("id", offer)
        .append_pair("currentstate", state)
        .append_pair("displayname", name)
        .append_pair("totalbytes", "12345")
        .finish();
    write(
        root,
        &format!("root/drives/c/ProgramData/Origin/LocalContent/{name}/main.mfst"),
        bytes,
    );
}
fn scan(env: &HostEnv) -> Vec<DetectedGame> {
    Ea.scan(env, &ScanCtx::default()).unwrap()
}
fn uris(game: &DetectedGame) -> Vec<&str> {
    game.launch
        .iter()
        .filter_map(|t| {
            if let LaunchTarget::Uri { uri } = t {
                Some(uri.as_str())
            } else {
                None
            }
        })
        .collect()
}
#[test]
fn ea_app_and_origin_era_games_use_xml_and_mfst_without_account_access() {
    let temp = scenario();
    add_game(temp.path(), "Modern Café", XML);
    add_game(
        temp.path(),
        "Legacy",
        &XML.replace("EA.OFR.Fixture", "Origin.OFR.Legacy")
            .replace("10001", "10002")
            .replace("Fixture EA &amp; Friends", "Fixture Origin"),
    );
    for name in [
        "Electronic Arts/EA Desktop/EA Desktop/EADesktop.exe",
        "Origin/Origin.exe",
    ] {
        write(
            temp.path(),
            &format!("root/drives/c/Program Files/{name}"),
            b"client",
        );
    }
    origin(temp.path(), "Legacy", "Origin.OFR.Legacy", "kReadyToStart");
    // The registry loader uses an authored Wine-format fixture, both views.
    write(
        temp.path(),
        "registry/hklm64.reg",
        r#"WINE REGISTRY Version 2
[Software\\EA Games\\Fixture]
"Install Dir"="C:\\Games\\Modern Caf\xe9"
[Software\\Wow6432Node\\EA Games\\Fixture]
"Install Dir"="C:\\Games\\Modern Caf\xe9"
"#,
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    let games = scan(&env);
    assert_eq!(games.len(), 2, "{games:?}");
    let modern = games.iter().find(|g| g.key == "EA.OFR.Fixture").unwrap();
    assert_eq!(modern.title, "Fixture EA & Friends");
    assert_eq!(modern.ids.len(), 2);
    assert_eq!(
        modern.ids[1],
        ExternalId::Ea {
            offer: "10001".into()
        }
    );
    assert_eq!(uris(modern), ["link2ea://launchgame/EA.OFR.Fixture"]);
    assert_eq!(modern.tracking.as_ref().unwrap().exe_names, ["Game.exe"]);
    assert!(matches!(modern.runtime, GameRuntime::Windows { .. }));
    assert_eq!(modern.state, InstallState::Installed);
    let legacy = games.iter().find(|g| g.key == "Origin.OFR.Legacy").unwrap();
    assert_eq!(legacy.title, "Fixture Origin");
    assert_eq!(
        uris(legacy),
        ["origin2://game/launch?offerIds=Origin.OFR.Legacy"]
    );
    assert_eq!(legacy.size, Some(12345));
    assert_eq!(Ea.detect(&env).len(), 2);
    let report =
        scan_with_sources(&env, &Config::default(), &Cancel::new(), &[Arc::new(Ea)]).unwrap();
    assert_eq!(report.groups.len(), 2);
    assert!(report.sources.iter().all(|s| s.error.is_none()));
    let watch = Ea.watch_paths(&env);
    assert!(
        watch
            .iter()
            .any(|w| w.recursive && w.path.ends_with("Origin/LocalContent"))
    );
    assert!(
        watch
            .iter()
            .any(|w| !w.recursive && w.path.ends_with("__Installer/installerdata.xml"))
    );
}
#[test]
fn both_registry_views_deduplicate_and_updating_manifest_wins_over_registry() {
    let temp = scenario();
    add_game(temp.path(), "Game", XML);
    origin(temp.path(), "Game", "EA.OFR.Fixture", "kUpdating");
    let mut registry = memory("Game", RegistryView::Registry64);
    registry.insert(
        RegistryKey::new(
            RegistryHive::LocalMachine,
            RegistryView::Registry32,
            r"SOFTWARE\EA Games\Fixture",
        ),
        "Install Dir",
        RegistryValue::String(r"C:\Games\Game".into()),
    );
    let env = HostEnv::fixture(temp.path())
        .unwrap()
        .with_registry(Arc::new(registry));
    let games = scan(&env);
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].state, InstallState::Updating);
    assert_eq!(games[0].confidence, Confidence::Maybe);
    assert_eq!(games[0].size, Some(12345));
    assert!(
        games[0]
            .evidence
            .iter()
            .any(|e| matches!(e, Evidence::Registry { .. }))
    );
    assert!(
        games[0]
            .evidence
            .iter()
            .any(|e| matches!(e, Evidence::File { .. }))
    );
}
#[test]
fn stale_partial_addon_and_corrupt_manifest_are_conservative() {
    let temp = scenario();
    origin(
        temp.path(),
        "Missing",
        "Origin.OFR.Missing",
        "kReadyToStart",
    );
    origin(temp.path(), "Partial", "Origin.OFR.Partial", "kPaused");
    fs::create_dir_all(temp.path().join("root/drives/c/Games/Partial")).unwrap();
    origin(temp.path(), "Addon", "Origin.OFR.Addon", "kReadyToStart");
    let addon = temp
        .path()
        .join("root/drives/c/ProgramData/Origin/LocalContent/Addon/main.mfst");
    let mut text = fs::read_to_string(&addon).unwrap();
    text.push_str("&isDLC=1");
    fs::write(addon, text).unwrap();
    write(
        temp.path(),
        "root/drives/c/ProgramData/Origin/LocalContent/Bad/bad.mfst",
        b"id=bad%QZ&currentstate=kReadyToStart",
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    let ctx = ScanCtx::default();
    let games = Ea.scan(&env, &ctx).unwrap();
    assert_eq!(games.len(), 2);
    assert!(matches!(games[0].state, InstallState::Broken { .. }));
    assert_eq!(games[1].state, InstallState::Partial);
    assert!(games.iter().all(|g| g.confidence == Confidence::Maybe));
    assert!(!ctx.take_warnings().is_empty());
}
#[test]
fn modern_client_only_uses_link2ea_for_legacy_mfst_and_registryless_xml() {
    let temp = scenario();
    write(
        temp.path(),
        "root/drives/c/Program Files/Electronic Arts/EA Desktop/EA Desktop/EADesktop.exe",
        b"client",
    );
    origin(temp.path(), "Legacy", "Origin.OFR.Legacy", "kReadyToStart");
    fs::create_dir_all(temp.path().join("root/drives/c/Games/Legacy")).unwrap();
    write(
        temp.path(),
        "root/drives/c/Program Files/EA Games/Folder/__Installer/installerdata.xml",
        XML,
    );
    let games = scan(&HostEnv::fixture(temp.path()).unwrap());
    assert_eq!(games.len(), 2);
    assert!(
        games
            .iter()
            .all(|g| uris(g)[0].starts_with("link2ea://launchgame/"))
    );
}
#[test]
fn xml_supports_utf16_locale_entities_and_attribute_executables() {
    let parsed = parse_installer_xml(XML.as_bytes()).unwrap();
    assert_eq!(parsed.title.as_deref(), Some("Fixture EA & Friends"));
    assert_eq!(parsed.content_ids, ["EA.OFR.Fixture", "10001"]);
    let xml = XML.replace(
        r"<launcher><filePath>__InstallLoc__\bin\Game.exe</filePath></launcher>",
        r#"<execute file="bin\Game.exe"/>"#,
    );
    let bytes: Vec<u8> = [0xff, 0xfe]
        .into_iter()
        .chain(xml.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    assert_eq!(
        parse_installer_xml(&bytes).unwrap().executable.as_deref(),
        Some(r"bin\Game.exe")
    );
    assert_eq!(
        parse_installer_xml(b"<game><title><![CDATA[A & B]]></title></game>")
            .unwrap()
            .title
            .as_deref(),
        Some("A & B")
    );
}
#[test]
fn malformed_and_hostile_metadata_is_bounded() {
    for value in [
        b"<!DOCTYPE x [<!ENTITY a 'boom'>]><x/>".as_slice(),
        b"<game><title>truncated",
        b"<x>&unknown;</x>",
        b"<x><y></x>",
        b"\xff\xff",
    ] {
        assert!(parse_installer_xml(value).is_err(), "{value:?}");
    }
    let deep = format!("{}{}", "<a>".repeat(33), "</a>".repeat(33));
    assert!(parse_installer_xml(deep.as_bytes()).is_err());
    let long = format!("<title>{}</title>", "a".repeat(65537));
    assert!(parse_installer_xml(long.as_bytes()).is_err());
    assert!(parse_installer_xml(&vec![b' '; (4 << 20) + 1]).is_err());
    for bytes in [
        b"id=a&dipinstallpath=C%QZ&currentstate=kReadyToStart".as_slice(),
        b"id=a&id=b&dipinstallpath=C&currentstate=kReadyToStart",
        b"id=a&dipinstallpath=C&currentstate=not-known",
        b"id=%FF&dipinstallpath=C&currentstate=kReadyToStart",
    ] {
        assert!(parse_origin_manifest(bytes).is_err());
    }
    let m=parse_origin_manifest(b"dipInstallPath=&dipinstallpath=C%3A%5CGames%5CA+Game&id=OFR%3A1&currentstate=kReadyToStart").unwrap();
    assert_eq!(m.install_dir, r"C:\Games\A Game");
    assert_eq!(m.offer, "OFR:1");
}
#[test]
fn cancellation_and_absent_registry_do_not_fall_back_to_live_host() {
    let temp = scenario();
    let env = HostEnv::fixture(temp.path()).unwrap();
    assert!(scan(&env).is_empty());
    assert!(!Ea.launcher_locations(&env).is_empty());
    assert!(
        Ea.launcher_locations(&env)
            .iter()
            .all(|p| p.starts_with(env.fs_root()))
    );
    assert!(Ea.detect(&env).iter().all(|s| !s.found));
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(matches!(
        Ea.scan(&env, &ScanCtx::new(cancel)),
        Err(SourceError::Cancelled)
    ));
}
#[test]
#[cfg(unix)]
fn installer_metadata_symlink_cannot_escape_fixture() {
    let temp = scenario();
    let outside = crate::fixtures::tempdir();
    write(outside.path(), "installerdata.xml", XML);
    let dir = temp.path().join("root/drives/c/Games/Game/__Installer");
    fs::create_dir_all(dir.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(outside.path(), &dir).unwrap();
    let env = HostEnv::fixture(temp.path())
        .unwrap()
        .with_registry(Arc::new(memory("Game", RegistryView::Registry32)));
    assert!(scan(&env).is_empty());
}
#[test]
fn rejected_executable_traversal_never_becomes_a_tracking_path() {
    let temp = scenario();
    add_game(
        temp.path(),
        "Game",
        &XML.replace(r"__InstallLoc__\bin\Game.exe", r"..\Outside.exe"),
    );
    let env = HostEnv::fixture(temp.path())
        .unwrap()
        .with_registry(Arc::new(memory("Game", RegistryView::Registry32)));
    assert!(
        scan(&env)[0]
            .tracking
            .as_ref()
            .unwrap()
            .exe_names
            .is_empty()
    );
}
