#![cfg(feature = "sqlite")]
//! All machines/databases/receipts are original synthetic records.
use caskade_discovery::{sources::itch::Itch, *};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use crate::fixtures;
fn source(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/src/itch")
        .join(name)
}
struct Machine {
    temp: tempfile::TempDir,
    db: PathBuf,
    game_root: PathBuf,
}
impl Machine {
    fn new(os: OperatingSystem) -> Self {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("root")).unwrap();
        let os_name = match os {
            OperatingSystem::Linux => "linux",
            OperatingSystem::Windows => "windows",
            OperatingSystem::Macos => "macos",
        };
        fs::write(
            temp.path().join("machine.toml"),
            format!("schema_version=1\nos=\"{os_name}\"\n"),
        )
        .unwrap();
        let env = HostEnv::fixture(temp.path()).unwrap();
        let home = match os {
            OperatingSystem::Linux => "/home/fixture",
            OperatingSystem::Windows => r"C:\Users\fixture",
            OperatingSystem::Macos => "/Users/fixture",
        };
        let games = format!("{home}/itch-games");
        let game_root = env.resolve_path(Path::new(&games)).unwrap();
        let config = if os == OperatingSystem::Windows {
            env.known_folder(env::KnownFolder::AppData)
                .unwrap()
                .to_owned()
        } else if os == OperatingSystem::Macos {
            env.home().join("Library/Application Support")
        } else {
            env.known_folder(env::KnownFolder::ConfigHome)
                .unwrap()
                .to_owned()
        };
        let db = config.join("itch/butler.db");
        fs::create_dir_all(db.parent().unwrap()).unwrap();
        let sql = fs::read_to_string(source("butler.sql"))
            .unwrap()
            .replace("@GAMES@", &games);
        rusqlite::Connection::open(&db)
            .unwrap()
            .execute_batch(&sql)
            .unwrap();
        for file in [
            "portable/game.sh",
            "portable/game.exe",
            "windows/game.exe",
            "receipt-only/start.sh",
            "receipt-only/start.exe",
        ] {
            let p = game_root.join(file);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "synthetic inert target; never executed").unwrap();
        }
        fs::copy(
            source("portable.itch.toml"),
            game_root.join("portable/.itch.toml"),
        )
        .unwrap();
        let result = Self {
            temp,
            db,
            game_root,
        };
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(source("receipt.json")).unwrap()).unwrap();
        if os == OperatingSystem::Windows {
            receipt["upload"]["platforms"] = serde_json::json!({"windows":"all"});
            receipt["files"] = serde_json::json!(["../outside.exe", "start.exe"]);
        }
        result.receipt(&serde_json::to_vec(&receipt).unwrap());
        result
    }
    fn env(&self) -> HostEnv {
        HostEnv::fixture(self.temp.path()).unwrap()
    }
    fn receipt(&self, bytes: &[u8]) {
        let path = self.game_root.join("receipt-only/.itch/receipt.json.gz");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut gzip = flate2::write::GzEncoder::new(
            fs::File::create(path).unwrap(),
            flate2::Compression::fast(),
        );
        gzip.write_all(bytes).unwrap();
        gzip.finish().unwrap();
    }
    fn scan(&self) -> (Vec<DetectedGame>, Vec<String>) {
        let ctx = ScanCtx::new(Cancel::new());
        let games = Itch.scan(&self.env(), &ctx).unwrap();
        (games, ctx.take_warnings())
    }
}
#[test]
fn linux_windows_and_macos_profiles_read_real_sqlite_and_gzip_receipts() {
    for os in [
        OperatingSystem::Linux,
        OperatingSystem::Windows,
        OperatingSystem::Macos,
    ] {
        let machine = Machine::new(os);
        let (games, warnings) = machine.scan();
        assert_eq!(games.len(), 4);
        assert_eq!(warnings.len(), 2);
        assert!(Itch.detect(&machine.env())[0].found);
        assert!(Itch.hosts().contains(os));
        let portable = games.iter().find(|g| g.key == "cave-portable").unwrap();
        assert_eq!(portable.title, "Fixture Itch Portable");
        assert!(portable.ids.contains(&ExternalId::Itch { game_id: 101 }));
        assert_eq!(portable.playtime_secs, Some(3600));
        assert_eq!(portable.size, Some(4096));
        assert_eq!(
            portable.launch[0],
            LaunchTarget::Uri {
                uri: "itch://caves/cave-portable/launch".into()
            }
        );
        let LaunchTarget::Exec {
            program, args, cwd, ..
        } = &portable.launch[1]
        else {
            panic!("no manifest action")
        };
        assert_eq!(cwd, &portable.install_dir);
        match os {
            OperatingSystem::Windows => {
                assert!(program.ends_with("game.exe"));
                assert_eq!(args, &["/name", "Fixture Itch Portable"]);
            }
            OperatingSystem::Linux => {
                assert!(program.ends_with("game.sh"));
                assert_eq!(args, &["--name", "Fixture Itch Portable", "$(literal)"]);
            }
            OperatingSystem::Macos => assert_eq!(args, &["--macos"]),
        }
        let windows = games.iter().find(|g| g.key == "cave-windows").unwrap();
        assert!(matches!(windows.runtime, GameRuntime::Windows { .. }));
        assert!(
            windows
                .install_dir
                .as_ref()
                .unwrap()
                .ends_with("itch-games/windows")
        );
        let receipt = games.iter().find(|g| g.key == "cave-receipt").unwrap();
        assert_eq!(receipt.title, "Fixture Receipt Only");
        assert_eq!(receipt.ids, [ExternalId::Itch { game_id: 201 }]);
        assert_eq!(receipt.launch.len(), 2);
        let LaunchTarget::Exec { program, .. } = &receipt.launch[1] else {
            panic!("no receipt target")
        };
        assert!(program.ends_with(if os == OperatingSystem::Windows {
            "start.exe"
        } else {
            "start.sh"
        }));
        assert!(matches!(
            games
                .iter()
                .find(|g| g.key == "cave-missing")
                .unwrap()
                .state,
            InstallState::Broken { .. }
        ));
        assert_eq!(Itch.watch_paths(&machine.env()).len(), 2);
        fixtures::assert_snapshot(
            match os {
                OperatingSystem::Linux => "itch_linux",
                OperatingSystem::Windows => "itch_windows",
                OperatingSystem::Macos => "itch_macos",
            },
            &games,
            machine.temp.path(),
        );
    }
}
#[test]
fn malformed_receipt_and_decompression_bomb_cannot_hide_database_games() {
    let machine = Machine::new(OperatingSystem::Linux);
    machine.receipt(b"{malformed");
    let (games, warnings) = machine.scan();
    assert_eq!(games.len(), 3);
    assert!(warnings.len() >= 3);
    machine.receipt(&vec![b' '; (2 << 20) + 1]);
    let (games, warnings) = machine.scan();
    assert_eq!(games.len(), 3);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("decoded receipt byte limit"))
    );
    let before = fs::read(&machine.db).unwrap();
    let cancel = Cancel::new();
    cancel.cancel();
    assert_eq!(
        Itch.scan(&machine.env(), &ScanCtx::new(cancel)),
        Err(SourceError::Cancelled)
    );
    assert_eq!(before, fs::read(&machine.db).unwrap());
}
#[test]
fn actively_locked_wal_reads_committed_caves_without_source_writes() {
    let machine = Machine::new(OperatingSystem::Linux);
    let db = rusqlite::Connection::open(&machine.db).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL;BEGIN IMMEDIATE; UPDATE games SET title='Uncommitted' WHERE id=101;").unwrap();
    let before = fs::read(&machine.db).unwrap();
    let wal = fs::read(machine.db.with_file_name("butler.db-wal")).unwrap();
    let (games, _) = machine.scan();
    assert_eq!(
        games
            .iter()
            .find(|g| g.key == "cave-portable")
            .unwrap()
            .title,
        "Fixture Itch Portable"
    );
    assert_eq!(before, fs::read(&machine.db).unwrap());
    assert_eq!(
        wal,
        fs::read(machine.db.with_file_name("butler.db-wal")).unwrap()
    );
}
