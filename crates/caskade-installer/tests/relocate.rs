//! #60: relocate with transactional rewrites (fake registry and shortcuts).
use caskade_installer::{
    events::JobEvent,
    relocate::{
        RegistryStore, RelocateError, RelocateRequest, Relocator, RunningCheck, ShortcutEditor,
    },
    runtime::alias::AliasStore,
    scan::{self, ScanInput, ScanLimits},
    testing::FakeDiskSpace,
};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio_util::sync::CancellationToken;

type Key = (String, String, String);

#[derive(Default)]
struct FakeRegistry {
    values: Mutex<BTreeMap<Key, scan::RegistryValue>>,
    /// Fail the n-th apply (1-based) after writing half of its values.
    fail_apply: AtomicUsize,
    applies: AtomicUsize,
}
impl FakeRegistry {
    fn set(&self, write: scan::RegistryValue) {
        self.values.lock().unwrap().insert(
            (write.hive.clone(), write.key.clone(), write.name.clone()),
            write,
        );
    }
    fn snapshot(&self) -> BTreeMap<Key, scan::RegistryValue> {
        self.values.lock().unwrap().clone()
    }
}
impl RegistryStore for FakeRegistry {
    fn read(&self, hive: &str, key: &str, name: &str) -> io::Result<Option<scan::RegistryValue>> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(&(hive.into(), key.into(), name.into()))
            .cloned())
    }
    fn apply(&self, writes: &[scan::RegistryValue]) -> io::Result<()> {
        let call = self.applies.fetch_add(1, Ordering::SeqCst) + 1;
        let fail = call == self.fail_apply.load(Ordering::SeqCst);
        let keep = if fail { writes.len() / 2 } else { writes.len() };
        for write in &writes[..keep.max(usize::from(fail))] {
            self.set(write.clone());
        }
        if fail {
            return Err(io::Error::other("injected registry failure halfway"));
        }
        Ok(())
    }
}

/// Rewrites UTF-16 strings inside the file like IShellLinkW would.
struct FakeShortcuts;
impl ShortcutEditor for FakeShortcuts {
    fn rewrite(&self, path: &Path, old: &str, new: &str) -> io::Result<()> {
        let bytes = fs::read(path)?;
        if let Some(updated) =
            caskade_installer::relocate::replace_all(&bytes, &[(old.to_owned(), new.to_owned())])
        {
            fs::write(path, updated)?;
        }
        Ok(())
    }
}

struct Running(AtomicBool);
impl RunningCheck for Running {
    fn running(&self, _: &Path) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Cancels after `after` move events.
struct Sink {
    cancel: CancellationToken,
    after: usize,
    seen: AtomicUsize,
}
impl caskade_installer::events::EventSink for Sink {
    fn emit(&self, _: JobEvent) {
        if self.seen.fetch_add(1, Ordering::SeqCst) + 1 == self.after {
            self.cancel.cancel();
        }
    }
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

struct Setup {
    _dirs: Vec<tempfile::TempDir>,
    game: PathBuf,
    target: PathBuf,
    external: PathBuf,
    store: AliasStore,
    alias: String,
    registry: Arc<FakeRegistry>,
}

fn setup() -> Setup {
    let old_root = tempfile::tempdir().unwrap();
    let new_root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let aliases = tempfile::tempdir().unwrap();
    let game = dunce::canonicalize(old_root.path())
        .unwrap()
        .join("Indie Game");
    fs::create_dir_all(game.join("data")).unwrap();
    let real = game.to_string_lossy().into_owned();
    let windows = format!("Z:{}", real.replace('/', "\\"));
    fs::write(
        game.join("config.ini"),
        format!("[Game]\r\nInstallPath={real}\r\nFoo=1\r\n"),
    )
    .unwrap();
    fs::write(
        game.join("data/settings.json"),
        format!(
            "{{\"dir\": \"{}\\\\saves\"}}\n",
            windows.replace('\\', "\\\\")
        ),
    )
    .unwrap();
    let mut save = vec![0xff, 0xfe];
    save.extend(utf16(&format!("path={windows}\\save1\r\n")));
    fs::write(game.join("data/save.cfg"), save).unwrap();
    fs::write(
        game.join("launch.desktop"),
        format!("[Desktop Entry]\nExec={real}/game\n"),
    )
    .unwrap();
    let mut lnk = b"L\0\0\0".to_vec();
    lnk.extend(utf16(&format!("{windows}\\game.exe")));
    fs::write(game.join("Game.lnk"), lnk).unwrap();
    fs::write(game.join("game.bin"), b"\0\x01binary").unwrap();
    // Binary evidence (Fragile): recorded, never rewritten.
    let mut cache = b"\0\x7fcache".to_vec();
    cache.extend(utf16(&windows));
    fs::write(game.join("data/cache.dat"), cache).unwrap();
    let external = outside.path().join("Indie Game.desktop");
    fs::write(&external, format!("[Desktop Entry]\nPath={real}\n")).unwrap();
    let registry = Arc::new(FakeRegistry::default());
    for (hive, value_type) in [("HKCU", "REG_SZ"), ("HKLM", "REG_EXPAND_SZ")] {
        registry.set(scan::RegistryValue {
            hive: hive.into(),
            key: r"Software\Indie".into(),
            name: "InstallPath".into(),
            value_type: value_type.into(),
            data: utf16(&format!("{windows}\0")),
        });
    }
    let store = AliasStore::new(aliases.path().join("g"));
    let alias = AliasStore::short_id("root/Indie Game");
    store.link(&alias, &game).unwrap();
    let target = dunce::canonicalize(new_root.path()).unwrap();
    Setup {
        _dirs: vec![old_root, new_root, outside, aliases],
        game,
        target,
        external,
        store,
        alias,
        registry,
    }
}

fn request(setup: &Setup, force_copy: bool) -> RelocateRequest {
    let registry: Vec<scan::RegistryValue> = setup
        .registry
        .snapshot()
        .into_values()
        .map(|value| scan::RegistryValue {
            hive: value.hive,
            key: value.key,
            name: value.name,
            value_type: value.value_type,
            data: value.data,
        })
        .collect();
    let external = vec![setup.external.clone()];
    let report = scan::scan_references(&ScanInput {
        game_dir: &setup.game,
        real_path: &setup.game.to_string_lossy(),
        alias_path: None,
        real_variants: &[],
        registry: &registry,
        external_files: &external,
        delegated: false,
        limits: ScanLimits::default(),
    })
    .unwrap();
    // config.ini, settings.json, save.cfg, launch.desktop, Game.lnk,
    // the binary cache.dat, the external .desktop and two registry values.
    assert_eq!(report.references.len(), 9, "{:#?}", report.references);
    RelocateRequest {
        job_id: "move-1".into(),
        game_dir: setup.game.clone(),
        target_root: setup.target.clone(),
        references: report.references,
        spellings: vec![],
        alias: Some((setup.store.clone(), setup.alias.clone())),
        force_copy,
    }
}

fn relocator(setup: &Setup, cancel: &CancellationToken, cancel_after: usize) -> Relocator {
    Relocator {
        disk: Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        sink: Arc::new(Sink {
            cancel: cancel.clone(),
            after: cancel_after,
            seen: AtomicUsize::new(0),
        }),
        registry: setup.registry.clone(),
        shortcuts: Arc::new(FakeShortcuts),
        running: Arc::new(Running(AtomicBool::new(false))),
    }
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    files
}

/// AC: a rewrite failing halfway (registry batch) restores every file,
/// value and link, for both a rename and a copy move, and on cancellation.
#[test]
fn failed_rewrite_halfway_restores_every_file_value_and_link() {
    for (force_copy, cancel_after) in [(false, 0), (true, 0), (false, 4), (true, 3)] {
        let setup = setup();
        let files = snapshot(&setup.game);
        let external = fs::read(&setup.external).unwrap();
        let values = setup.registry.snapshot();
        let request = request(&setup, force_copy);
        if cancel_after == 0 {
            setup.registry.fail_apply.store(1, Ordering::SeqCst);
        }
        let cancel = CancellationToken::new();
        let error = relocator(&setup, &cancel, cancel_after)
            .relocate(&request, &cancel)
            .unwrap_err();
        match cancel_after {
            0 => assert!(matches!(error, RelocateError::RolledBack(_)), "{error}"),
            _ => assert!(matches!(error, RelocateError::Cancelled), "{error}"),
        }
        assert_eq!(snapshot(&setup.game), files, "copy={force_copy}");
        assert_eq!(fs::read(&setup.external).unwrap(), external);
        assert_eq!(setup.registry.snapshot(), values);
        assert_eq!(
            setup.store.target(&setup.alias).unwrap().as_deref(),
            Some(setup.game.as_path())
        );
        assert!(!setup.target.join("Indie Game").exists());
    }
}

#[test]
fn successful_move_rewrites_every_reference_and_keeps_encodings() {
    for force_copy in [false, true] {
        let setup = setup();
        let old = setup.game.to_string_lossy().into_owned();
        let request = request(&setup, force_copy);
        let cancel = CancellationToken::new();
        let report = relocator(&setup, &cancel, usize::MAX)
            .relocate(&request, &cancel)
            .unwrap();
        let game = setup.target.join("Indie Game");
        let new = game.to_string_lossy().into_owned();
        let new_windows = format!("Z:{}", new.replace('/', "\\"));
        assert_eq!(report.game_dir, game);
        assert_eq!(report.copied, force_copy);
        assert_eq!((report.rewritten_files, report.rewritten_values), (6, 2));
        assert!(!setup.game.exists());
        assert_eq!(
            fs::read_to_string(game.join("config.ini")).unwrap(),
            format!("[Game]\r\nInstallPath={new}\r\nFoo=1\r\n")
        );
        assert_eq!(
            fs::read_to_string(game.join("data/settings.json")).unwrap(),
            format!(
                "{{\"dir\": \"{}\\\\saves\"}}\n",
                new_windows.replace('\\', "\\\\")
            )
        );
        let mut save = vec![0xff, 0xfe];
        save.extend(utf16(&format!("path={new_windows}\\save1\r\n")));
        assert_eq!(fs::read(game.join("data/save.cfg")).unwrap(), save);
        assert!(
            fs::read_to_string(game.join("launch.desktop"))
                .unwrap()
                .contains(&format!("Exec={new}/game"))
        );
        assert!(
            fs::read(game.join("Game.lnk"))
                .unwrap()
                .ends_with(&utf16(&format!("{new_windows}\\game.exe")))
        );
        assert_eq!(fs::read(game.join("game.bin")).unwrap(), b"\0\x01binary");
        let mut cache = b"\0\x7fcache".to_vec();
        cache.extend(utf16(&format!("Z:{}", old.replace('/', "\\"))));
        assert_eq!(fs::read(game.join("data/cache.dat")).unwrap(), cache);
        assert!(
            fs::read_to_string(&setup.external)
                .unwrap()
                .contains(&format!("Path={new}"))
        );
        assert!(!fs::read_to_string(&setup.external).unwrap().contains(&old));
        for value in setup.registry.snapshot().values() {
            assert_eq!(value.data, utf16(&format!("{new_windows}\0")));
        }
        assert_eq!(
            setup.store.target(&setup.alias).unwrap().as_deref(),
            Some(game.as_path())
        );
    }
}

#[test]
fn relocate_with_verdict_records_it_with_the_move() {
    use caskade_installer::manifest::{InstallManifest, RelocationVerdict};
    let setup = setup();
    // A minimal valid manifest for the game being moved.
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.files.clear();
    fs::create_dir_all(setup.game.join(".caskade")).unwrap();
    InstallManifest::new(record)
        .unwrap()
        .write(&setup.game)
        .unwrap();
    let request = request(&setup, false);
    let cancel = CancellationToken::new();
    let report = relocator(&setup, &cancel, usize::MAX)
        .relocate_with_verdict(&request, &cancel, Some(RelocationVerdict::Fragile))
        .unwrap();
    let manifest = InstallManifest::read(&report.game_dir).unwrap();
    assert_eq!(
        manifest.record().relocation.verdict,
        RelocationVerdict::Fragile
    );
    assert_eq!(manifest.record().relocation.references.len(), 9);
}

#[test]
fn preconditions_refuse_running_games_full_disks_and_existing_targets() {
    let setup = setup();
    let request = request(&setup, true);
    let cancel = CancellationToken::new();
    let mut relocator = relocator(&setup, &cancel, usize::MAX);
    relocator.running = Arc::new(Running(AtomicBool::new(true)));
    assert!(matches!(
        relocator.relocate(&request, &cancel),
        Err(RelocateError::Running)
    ));
    relocator.running = Arc::new(Running(AtomicBool::new(false)));
    relocator.disk = Arc::new(FakeDiskSpace::fixed(10));
    assert!(matches!(
        relocator.relocate(&request, &cancel),
        Err(RelocateError::Space { .. })
    ));
    relocator.disk = Arc::new(FakeDiskSpace::fixed(u64::MAX));
    fs::create_dir(setup.target.join("Indie Game")).unwrap();
    assert!(matches!(
        relocator.relocate(&request, &cancel),
        Err(RelocateError::Target(_))
    ));
    assert!(setup.game.join("config.ini").is_file());
}

/// #22 Windows runner: a real cross-volume move (e.g. C: → D:). Set
/// CASKADE_TEST_NATIVE=1 and CASKADE_TEST_MOVE_ROOT to a folder on another
/// volume.
#[cfg(windows)]
#[test]
fn native_cross_volume_move_copies_verifies_and_deletes_the_source() {
    let (Some(_), Some(target)) = (
        std::env::var_os("CASKADE_TEST_NATIVE"),
        std::env::var_os("CASKADE_TEST_MOVE_ROOT"),
    ) else {
        eprintln!("skipping: set CASKADE_TEST_NATIVE=1 and CASKADE_TEST_MOVE_ROOT");
        return;
    };
    let setup = setup();
    let mut request = request(&setup, false);
    request.target_root = PathBuf::from(target);
    let cancel = CancellationToken::new();
    let report = relocator(&setup, &cancel, usize::MAX)
        .relocate(&request, &cancel)
        .unwrap();
    assert!(report.copied, "target must be on another volume");
    assert!(!setup.game.exists());
    fs::remove_dir_all(report.game_dir).unwrap();
}

/// The development CLI end to end: install an archive fixture into one root,
/// `move` it to another root, and find it there with an updated manifest.
#[cfg(all(feature = "cli", unix))]
#[test]
fn cli_moves_an_installed_game_to_another_root() {
    use caskade_installer::manifest::{InstallManifest, RootManifest};
    let binary = env!("CARGO_BIN_EXE_caskade-install");
    let run = |args: &[&std::ffi::OsStr]| {
        let output = std::process::Command::new(binary)
            .arg("--json")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/linux.tar");
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let installed = run(&[
        "install".as_ref(),
        fixture.as_os_str(),
        "--root".as_ref(),
        first.path().as_os_str(),
    ]);
    let game = first
        .path()
        .join(installed["value"]["dir"].as_str().unwrap());
    // Give the target its own identity, as a configured storage root has.
    RootManifest::new("second-root".to_owned())
        .unwrap()
        .write(second.path())
        .unwrap();
    let moved = run(&["move".as_ref(), game.as_os_str(), second.path().as_os_str()]);
    assert_eq!(moved["type"], "moved");
    let destination = PathBuf::from(moved["value"]["dir"].as_str().unwrap());
    assert!(!game.exists());
    let manifest = InstallManifest::read(&destination).unwrap();
    assert_eq!(manifest.record().plan.root_id, "second-root");
    assert_eq!(
        manifest.record().relocation.last_path.as_deref(),
        Some(destination.to_string_lossy().as_ref())
    );
    assert!(manifest.verify(&destination).unwrap().changed.is_empty());
}
