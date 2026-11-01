//! Only synthetic machines are read. No real launcher directories are inspected.
use caskade_discovery::{
    Cancel, Config, DetectedGame, ExternalId, HostEnv, LauncherStatus, OsSet, ScanCtx, Source,
    SourceError, SourceId, WatchPath, dump::dump_anonymized, env::KnownFolder, scan_with_sources,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use crate::fixtures;
fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

struct Heroic;
impl Source for Heroic {
    fn id(&self) -> SourceId {
        SourceId::new("fixture.heroic")
    }
    fn hosts(&self) -> OsSet {
        OsSet::ALL
    }
    fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn watch_paths(&self, _: &HostEnv) -> Vec<WatchPath> {
        vec![]
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        ctx.check()?;
        let path = env
            .known_folder(KnownFolder::ConfigHome)
            .unwrap()
            .join("heroic/legendaryConfig/legendary/installed.json");
        let records: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).map_err(|e| SourceError::io(&path, e))?)
                .map_err(|e| SourceError::parse(&path, e))?;
        let mut games = vec![];
        for (key, record) in records.as_object().unwrap() {
            let mut game = DetectedGame::new(self.id(), key, record["title"].as_str().unwrap());
            game.ids.push(ExternalId::Epic {
                namespace: record["namespace"].as_str().unwrap().into(),
                catalog_item: record["catalog_item_id"].as_str().unwrap().into(),
                app_name: key.clone(),
            });
            game.install_dir = Some(
                env.resolve_path(Path::new(record["install_path"].as_str().unwrap()))
                    .map_err(|e| SourceError::io(&path, e))?,
            );
            games.push(game);
        }
        Ok(games)
    }
}
fn groups(env: &HostEnv) -> serde_json::Value {
    let report =
        scan_with_sources(env, &Config::default(), &Cancel::new(), &[Arc::new(Heroic)]).unwrap();
    assert!(
        report.sources.iter().all(|source| source.error.is_none()),
        "{report:?}"
    );
    assert!(
        !report.groups.is_empty(),
        "an empty replay does not prove anything"
    );
    let mut json = serde_json::to_string(&report.groups).unwrap();
    // Anonymization deliberately changes the home; compare the same semantic
    // paths relative to each captured home, rather than erasing all path data.
    let home = env.home().to_string_lossy();
    json = json.replace(
        serde_json::to_string(home.as_ref())
            .unwrap()
            .trim_matches('"'),
        "$HOME",
    );
    serde_json::from_str(&json).unwrap()
}

#[test]
fn scenario_loader_is_sorted_confined_and_materializes_only_declared_machine() {
    let all = fixtures::all(&corpus()).unwrap();
    assert!(all.len() >= 4);
    assert!(all.windows(2).all(|pair| pair[0].path < pair[1].path));
    assert!(fixtures::Scenario::load(&corpus(), "../private").is_err());
    let scenario = fixtures::Scenario::load(&corpus(), "linux-mixed").unwrap();
    assert!(scenario.file("../README.md").is_err());
    let temp = tempfile::tempdir().unwrap();
    let copy = temp.path().join("machine");
    scenario.materialize(&copy).unwrap();
    assert!(copy.join("machine.toml").is_file());
    assert!(!copy.join("src").exists());
    assert!(scenario.materialize(&copy).is_err());
}

#[test]
fn nested_report_snapshot_paths_become_fixture_relative() {
    let scenario = fixtures::Scenario::load(&corpus(), "linux-mixed").unwrap();
    let path = scenario.path.join("root").join("some-file");
    let sibling = format!("{}-other/file", scenario.path.display());
    let value =
        serde_json::json!({"evidence":[{"path":path}],"sibling":sibling,"title":"Fixture Puzzle"});
    let normalized = fixtures::relative_value(&value, &scenario.path).unwrap();
    assert_eq!(normalized["sibling"], sibling);
    assert_eq!(
        normalized["evidence"][0]["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/"),
        "$FIXTURE/root/some-file"
    );
}

#[test]
fn nonempty_scan_groups_survive_anonymized_dump_and_fixture_replay() {
    let scenario = fixtures::Scenario::load(&corpus(), "linux-mixed").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let live = temp.path().join("synthetic-live");
    scenario.materialize(&live).unwrap();
    let metadata = fs::read_to_string(live.join("machine.toml"))
        .unwrap()
        .replace("/home/fixture", "/home/private-test-user");
    fs::write(live.join("machine.toml"), metadata).unwrap();
    fs::rename(
        live.join("root/home/fixture"),
        live.join("root/home/private-test-user"),
    )
    .unwrap();
    let file = live.join(
        "root/home/private-test-user/.config/heroic/legendaryConfig/legendary/installed.json",
    );
    let records = fs::read_to_string(&file)
        .unwrap()
        .replace("/home/fixture", "/home/private-test-user");
    fs::write(&file, &records).unwrap();
    let original = HostEnv::fixture(&live).unwrap();
    let before = groups(&original);
    let secret = file.parent().unwrap().join("user.json");
    fs::write(
        &secret,
        r#"{"password":"do-not-copy","account_id":"real-account"}"#,
    )
    .unwrap();
    let output = temp.path().join("anonymized");
    let report = dump_anonymized(&original, &output).unwrap();
    assert!(!report.files.is_empty());
    assert!(!output.join("root/home/private-test-user").exists());
    assert!(!report.files.iter().any(|path| path.ends_with("user.json")));
    let replay = HostEnv::fixture(&output).unwrap();
    assert_eq!(before, groups(&replay));
    assert_eq!(
        records,
        fs::read_to_string(file).unwrap(),
        "dump changed source metadata"
    );
    assert_eq!(
        fs::read_to_string(secret).unwrap(),
        r#"{"password":"do-not-copy","account_id":"real-account"}"#
    );
    insta::assert_json_snapshot!("nonempty_dump_replay", before);
}

#[test]
fn existing_destination_is_refused_without_changes() {
    let env = HostEnv::fixture(corpus().join("machines/linux-mixed")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("keep"), "existing data").unwrap();
    assert!(dump_anonymized(&env, temp.path()).is_err());
    assert_eq!(
        fs::read_to_string(temp.path().join("keep")).unwrap(),
        "existing data"
    );
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_export_preserves_games_and_omits_deleted_pages_and_account_tables() {
    let scenario = fixtures::Scenario::load(&corpus(), "linux-mixed").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let live = temp.path().join("machine");
    scenario.materialize(&live).unwrap();
    let source = live.join("root/home/fixture/.local/share/lutris/pga.db");
    {
        let db = rusqlite::Connection::open(&source).unwrap();
        db.execute_batch("PRAGMA secure_delete=OFF; CREATE TABLE credentials (secret TEXT); INSERT INTO credentials VALUES ('deleted-sensitive-sentinel'); DELETE FROM credentials;").unwrap();
    }
    assert!(
        fs::read(&source)
            .unwrap()
            .windows(b"deleted-sensitive-sentinel".len())
            .any(|w| w == b"deleted-sensitive-sentinel")
    );
    let before = fs::read(&source).unwrap();
    let output = temp.path().join("dump");
    dump_anonymized(&HostEnv::fixture(&live).unwrap(), &output).unwrap();
    let target = output.join("root/home/fixture/.local/share/lutris/pga.db");
    let bytes = fs::read(&target).unwrap();
    assert!(
        !bytes
            .windows(b"deleted-sensitive-sentinel".len())
            .any(|w| w == b"deleted-sensitive-sentinel")
    );
    let db =
        rusqlite::Connection::open_with_flags(target, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM games WHERE installed=1", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        2
    );
    assert!(db.prepare("SELECT * FROM credentials").is_err());
    assert_eq!(before, fs::read(source).unwrap());
}

#[test]
fn generated_windows_macos_and_wine_metadata_dumps_replay_without_host_access() {
    for name in [
        "linux-mixed",
        "windows-mixed",
        "macos-mixed",
        "linux-wine",
        "steam-linux-shared",
        "steam-windows-libraries",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let env = HostEnv::fixture(corpus().join("machines").join(name)).unwrap();
        let output = temp.path().join("dump");
        let report = dump_anonymized(&env, &output).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(report.files.len() > 1, "{name}: no launcher data captured");
        let replay = HostEnv::fixture(&output).unwrap();
        assert_eq!(env.os(), replay.os());
        for relative in &report.files {
            let path = output.join(relative);
            let bytes = fs::read(&path).unwrap();
            assert!(!bytes.is_empty(), "{path:?}");
            let limits = caskade_discovery::parsers::Limits::default();
            match path.file_name().and_then(|s| s.to_str()).unwrap() {
                "shortcuts.vdf" => {
                    caskade_discovery::parsers::binary_vdf::parse(&bytes, &limits).unwrap();
                }
                "appinfo.vdf" => {
                    caskade_discovery::parsers::binary_vdf::parse_appinfo(&bytes, &limits).unwrap();
                }
                "product.db" => {
                    caskade_discovery::parsers::protobuf::parse_product_db(&bytes, &limits)
                        .unwrap();
                }
                _ if path.extension().is_some_and(|s| s == "lnk") => {
                    caskade_discovery::parsers::lnk::parse(&bytes, &limits).unwrap();
                }
                _ => {}
            }
        }
    }
}

#[test]
fn malformed_allowlisted_metadata_does_not_publish_a_partial_dump() {
    let scenario = fixtures::Scenario::load(&corpus(), "linux-mixed").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let live = temp.path().join("machine");
    scenario.materialize(&live).unwrap();
    fs::write(
        live.join("root/home/fixture/.config/heroic/legendaryConfig/legendary/installed.json"),
        "{malformed",
    )
    .unwrap();
    let output = temp.path().join("dump");
    assert!(dump_anonymized(&HostEnv::fixture(&live).unwrap(), &output).is_err());
    assert!(!output.exists());
    assert!(!fs::read_dir(temp.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".caskade-discovery-dump-")
    }));
}
