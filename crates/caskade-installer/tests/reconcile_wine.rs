//! Explicit local Wine proof. Native Windows acceptance is a separate runner.
#![cfg(unix)]

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use caskade_installer::{
    RuntimeSpec,
    exec::CancellationToken,
    manifest::{
        InstallManifest, InstalledFile, ManifestPath, RelocationVerdict, RootManifest, Sha256Digest,
    },
    reconcile::{ReconcileStatus, Reconciler},
    relocate::{
        RegistryStore, Relocator, RunningCheck,
        backends::{NoShortcuts, PrefixRegistry},
    },
    runtime::{
        NativeRuntime, ProcessCommand, ProcessExit, ProcessId, Runtime, RuntimeError,
        proton::{Launcher, PrefixRuntime},
    },
    scan::{self, Encoding, Location, Reference, Via},
    testing::{FakeDiskSpace, RecordingSink},
};
use sha2::{Digest, Sha256};

struct Stopped;
impl RunningCheck for Stopped {
    fn running(&self, _: &Path) -> bool {
        false
    }
}

struct Isolated {
    wine: PrefixRuntime,
    environment: BTreeMap<String, String>,
}
impl Runtime for Isolated {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        let mut command = command.clone();
        command.env.extend(self.environment.clone());
        self.wine.spawn(&command)
    }
    fn wait(&self, id: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError> {
        self.wine.wait(id, timeout)
    }
    fn kill(&self, id: ProcessId) -> Result<(), RuntimeError> {
        self.wine.kill(id)
    }
}
fn run(runtime: &dyn Runtime, command: ProcessCommand) {
    let id = runtime.spawn(&command).unwrap();
    let result = runtime.wait(id, Duration::from_secs(180));
    runtime.kill(id).unwrap();
    assert_eq!(result.unwrap().code, 0);
}
fn environment(prefix: &Path, home: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("WINEPREFIX".into(), prefix.to_str().unwrap().into()),
        ("WINEDEBUG".into(), "-all".into()),
        ("WINEDLLOVERRIDES".into(), "winemenubuilder.exe=d".into()),
        (
            "XDG_DATA_HOME".into(),
            home.join("xdg-data").to_str().unwrap().into(),
        ),
        (
            "XDG_CONFIG_HOME".into(),
            home.join("xdg-config").to_str().unwrap().into(),
        ),
    ])
}
fn stop(prefix: &Path, home: &Path) {
    // Stop only this private prefix, flushing registry files before a byte read.
    let runtime = NativeRuntime::new();
    let command = ProcessCommand {
        program: "wineserver".into(),
        args: vec!["-k".into()],
        env: environment(prefix, home),
        cwd: home.into(),
    };
    if let Ok(id) = runtime.spawn(&command) {
        let _ = runtime.wait(id, Duration::from_secs(20));
        let _ = runtime.kill(id);
    }
}
struct StopServers {
    prefixes: Vec<PathBuf>,
    home: PathBuf,
}
impl Drop for StopServers {
    fn drop(&mut self) {
        for prefix in &self.prefixes {
            stop(prefix, &self.home);
        }
    }
}

#[tokio::test]
#[ignore = "private real Wine registry; opt in with CASKADE_TEST_WINE=1 under Xvfb"]
async fn explorer_move_then_scan_root_rewrites_a_real_private_wine_registry() {
    if std::env::var("CASKADE_TEST_WINE").as_deref() != Ok("1") {
        eprintln!("Wine proof requires the separate CASKADE_TEST_WINE=1 opt-in");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("library");
    fs::create_dir(&root).unwrap();
    RootManifest::new("wine-reconcile-root")
        .unwrap()
        .write(&root)
        .unwrap();
    let old = root.join("Old Game");
    let new = root.join("Moved Game");
    fs::create_dir(&old).unwrap();
    let prefix = old.join(".caskade/prefix");
    fs::create_dir_all(prefix.parent().unwrap()).unwrap();
    let mut cleanup = StopServers {
        prefixes: vec![prefix.clone(), new.join(".caskade/prefix")],
        home: home.path().into(),
    };
    let native = NativeRuntime::new();
    // Initialise before PrefixRuntime so even wineboot gets private XDG paths.
    run(
        &native,
        ProcessCommand {
            program: "wine".into(),
            args: vec!["wineboot".into(), "-u".into()],
            env: environment(&prefix, home.path()),
            cwd: old.clone(),
        },
    );
    stop(&prefix, home.path());
    let wine: Arc<dyn Runtime> = Arc::new(Isolated {
        wine: PrefixRuntime::new(Launcher::Wine {
            wine: "wine".into(),
        }),
        environment: environment(&prefix, home.path()),
    });
    let registry = PrefixRegistry::new(wine, prefix.clone(), old.clone());
    let windows_old = format!("Z:{}", old.to_str().unwrap().replace('/', "\\"));
    let write = scan::RegistryValue {
        hive: "HKCU".into(),
        key: "Software\\CaskadeReconcileProof".into(),
        name: "InstallPath".into(),
        value_type: "REG_SZ".into(),
        data: windows_old
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect(),
    };
    registry.apply(std::slice::from_ref(&write)).unwrap();
    stop(&prefix, home.path());
    assert!(
        registry
            .read(&write.hive, &write.key, &write.name)
            .unwrap()
            .is_some()
    );
    fs::write(old.join("config.ini"), old.to_str().unwrap()).unwrap();
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.plan.root_id = "wine-reconcile-root".into();
    record.plan.relative_dir = "Old Game".into();
    record.runtime = RuntimeSpec::WinePrefix {
        id: "wine-proof".into(),
    };
    record.launch_targets.clear();
    record.uninstall_steps.clear();
    record.external_effects.clear();
    record.alias = None;
    let bytes = fs::read(old.join("config.ini")).unwrap();
    record.files = vec![InstalledFile {
        path: ManifestPath::new("config.ini").unwrap(),
        size: bytes.len() as u64,
        sha256: Some(Sha256Digest::new(format!("{:x}", Sha256::digest(&bytes))).unwrap()),
    }];
    record.relocation.verdict = RelocationVerdict::Rewritable;
    record.relocation.last_path = Some(old.to_str().unwrap().into());
    record.relocation.references = vec![Reference {
        location: Location::Registry {
            hive: write.hive.clone(),
            key: write.key.clone(),
            name: write.name.clone(),
            value_type: write.value_type.clone(),
            rewritable: true,
        },
        via: Via::Real,
        encoding: Encoding::Utf16Le,
    }];
    InstallManifest::new(record).unwrap().write(&old).unwrap();
    fs::rename(&old, &new).unwrap(); // External Explorer-style move.
    let prefix = new.join(".caskade/prefix");
    let wine: Arc<dyn Runtime> = Arc::new(Isolated {
        wine: PrefixRuntime::new(Launcher::Wine {
            wine: "wine".into(),
        }),
        environment: environment(&prefix, home.path()),
    });
    let backend = Arc::new(PrefixRegistry::new(
        wine.clone(),
        prefix.clone(),
        new.clone(),
    ));
    let scanner = Reconciler::new(Arc::new(Relocator {
        disk: Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        sink: Arc::new(RecordingSink::default()),
        registry: backend.clone(),
        shortcuts: Arc::new(NoShortcuts),
        running: Arc::new(Stopped),
    }));
    let report = scanner
        .scan_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        report.games[0].status,
        ReconcileStatus::Reconciled,
        "{report:?}"
    );
    assert_eq!(report.games[0].rewritten_values, 1);
    // Query the live Wine registry through real reg.exe, not just a fake store.
    run(
        &*wine,
        ProcessCommand {
            program: "reg".into(),
            args: vec![
                "export".into(),
                format!("HKCU\\{}", write.key),
                r"G:\Game\registry-proof.reg".into(),
                "/y".into(),
            ],
            env: environment(&prefix, home.path()),
            cwd: new.clone(),
        },
    );
    let exported = fs::read(new.join("registry-proof.reg")).unwrap();
    let exported = String::from_utf16_lossy(
        &exported
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect::<Vec<_>>(),
    );
    let windows_new = format!("Z:{}", new.to_str().unwrap().replace('/', "\\"));
    assert!(
        exported.contains(&format!(
            "\"InstallPath\"=\"{}\"",
            windows_new.replace('\\', "\\\\")
        )),
        "{exported}"
    );
    fs::remove_file(new.join("registry-proof.reg")).unwrap();
    stop(&prefix, home.path());
    let persisted = backend
        .read(&write.hive, &write.key, &write.name)
        .unwrap()
        .unwrap();
    let persisted_text = String::from_utf16_lossy(
        &persisted
            .data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect::<Vec<_>>(),
    );
    assert_eq!(persisted_text.trim_end_matches('\0'), windows_new);
    assert!(!old.exists());
    assert_eq!(
        fs::read_to_string(new.join("config.ini")).unwrap(),
        new.to_str().unwrap()
    );
    let manifest = InstallManifest::read(&new).unwrap();
    assert_eq!(
        manifest.record().relocation.last_path.as_deref(),
        new.to_str()
    );
    assert!(manifest.verify(&new).unwrap().changed.is_empty());
    assert_eq!(
        RootManifest::read(&root).unwrap().root_id(),
        "wine-reconcile-root"
    );

    #[cfg(feature = "cli")]
    {
        // Exercise the managed CLI move as well: its backend must be bound to
        // the destination prefix after the physical move, rather than the now
        // missing source directory used during preparation.
        let target_root = home.path().join("Other Root");
        fs::create_dir(&target_root).unwrap();
        RootManifest::new("wine-target-root")
            .unwrap()
            .write(&target_root)
            .unwrap();
        let moved = target_root.join("Moved Game");
        let moved_prefix = moved.join(".caskade/prefix");
        cleanup.prefixes.push(moved_prefix.clone());
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .args(["--json", "move"])
            .arg(&new)
            .arg(&target_root)
            .envs(environment(&moved_prefix, home.path()))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        stop(&moved_prefix, home.path());
        let registry = PrefixRegistry::new(
            Arc::new(Isolated {
                wine: PrefixRuntime::new(Launcher::Wine {
                    wine: "wine".into(),
                }),
                environment: environment(&moved_prefix, home.path()),
            }),
            moved_prefix,
            moved.clone(),
        );
        let value = registry
            .read(&write.hive, &write.key, &write.name)
            .unwrap()
            .unwrap();
        let text = String::from_utf16_lossy(
            &value
                .data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            text.trim_end_matches('\0'),
            format!("Z:{}", moved.to_str().unwrap().replace('/', "\\"))
        );
        assert!(!new.exists());
        assert_eq!(
            fs::read_to_string(moved.join("config.ini")).unwrap(),
            moved.to_str().unwrap()
        );
        let manifest = InstallManifest::read(&moved).unwrap();
        assert_eq!(manifest.record().plan.root_id, "wine-target-root");
        assert_eq!(
            manifest.record().relocation.last_path.as_deref(),
            moved.to_str()
        );
        assert!(manifest.verify(&moved).unwrap().changed.is_empty());
    }
    // Keep the cleanup guard live until every real registry query has finished.
    let _ = &mut cleanup;
}
