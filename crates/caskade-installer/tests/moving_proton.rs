//! #62 T7: genuine Proton install, move to a second filesystem, launch and
//! own uninstall. Uses an existing writable mount; never mounts or uses sudo.
#![cfg(all(target_os = "linux", feature = "fetch", feature = "windows-formats"))]
use caskade_installer::{
    disk::{DiskSpace, FileSystemDiskSpace},
    exec::{CancellationToken, ExecRequest, Executor},
    host::HostCaps,
    manifest::{
        InstallManifest, InstalledFile, ManifestPath, ManifestSource, RelocationVerdict,
        Sha256Digest,
    },
    probe::ProbeEngine,
    relocate::{
        RelocateRequest, Relocator, RunningCheck,
        backends::{NoRegistry, NoShortcuts},
    },
    runtime::{
        NativeRuntime, ProcessCommand, Runtime,
        proton::{GAME_DIR, Launcher, PrefixRuntime},
        umu::UmuBootstrap,
    },
    scan::{self, ScanInput},
    testing::{FakeDiskSpace, RecordingSink, TestRoot},
    *,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

struct NotRunning;
impl RunningCheck for NotRunning {
    fn running(&self, _: &Path) -> bool {
        false
    }
}
fn run(runtime: &dyn Runtime, game: &Path, program: &str, args: Vec<String>) -> i32 {
    let pid = runtime
        .spawn(&ProcessCommand {
            program: program.into(),
            args,
            env: BTreeMap::new(),
            cwd: game.into(),
        })
        .unwrap();
    let exit = runtime.wait(pid, Duration::from_secs(600));
    runtime.kill(pid).unwrap();
    exit.unwrap().code
}
fn self_check(runtime: &dyn Runtime, game: &Path) {
    assert_eq!(
        run(
            runtime,
            game,
            &format!(r"{GAME_DIR}\FixtureGame.exe"),
            vec![
                "--self-check".into(),
                "--check-z-drive".into(),
                "--token".into(),
                format!(r"{GAME_DIR}\move-token")
            ]
        ),
        0,
        "{:?}",
        fs::read_to_string(game.join("move-token"))
    );
    assert_eq!(fs::read_to_string(game.join("move-token")).unwrap(), "ok\n");
    fs::remove_file(game.join("move-token")).unwrap();
}
fn inventory(game: &Path, dir: &Path, files: &mut Vec<InstalledFile>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path == game.join(".caskade") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).unwrap();
        if metadata.is_dir() {
            inventory(game, &path, files);
        } else if metadata.is_file() {
            files.push(InstalledFile {
                path: ManifestPath::new(path.strip_prefix(game).unwrap().to_str().unwrap())
                    .unwrap(),
                size: metadata.len(),
                sha256: Some(
                    Sha256Digest::new(format!("{:x}", Sha256::digest(fs::read(&path).unwrap())))
                        .unwrap(),
                ),
            });
        }
    }
}
fn registry(game: &Path) -> Vec<scan::RegistryValue> {
    caskade_installer::runtime::registry::parse_prefix_reg(
        &fs::read_to_string(game.join(".caskade/prefix/system.reg")).unwrap(),
        "HKLM",
    )
    .unwrap()
    .into_iter()
    .filter(|v| {
        v.key
            .eq_ignore_ascii_case(r"Software\CaskadeFixture\FixtureGame")
            && v.name == "InstallPath"
    })
    .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "real cached umu/Proton, executable second filesystem; CASKADE_TEST_NATIVE=1"]
async fn aliased_proton_install_moves_filesystems_without_rewriting_then_self_checks_and_own_uninstalls()
 {
    if std::env::var("CASKADE_TEST_NATIVE").as_deref() != Ok("1") {
        return;
    }
    let root = TestRoot::new().unwrap();
    let second_mount = std::env::var_os("CASKADE_TEST_MOVE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/dev/shm"));
    let target = tempfile::tempdir_in(&second_mount).expect(
        "provide an existing writable executable mount via CASKADE_TEST_MOVE_ROOT, e.g. /dev/shm",
    );
    assert_ne!(
        fs::metadata(root.path()).unwrap().dev(),
        fs::metadata(target.path()).unwrap().dev(),
        "a second directory on the source filesystem does not prove a drive move"
    );
    assert!(
        FileSystemDiskSpace.available_bytes(target.path()).unwrap() >= 4 * 1024 * 1024 * 1024,
        "the second filesystem needs at least 4 GiB free for the real Proton prefix; configure CASKADE_TEST_MOVE_ROOT or the runner's tmpfs size"
    );
    let umu = UmuBootstrap::for_host().unwrap().ensure().await.unwrap();
    let host = HostCaps::detect();
    let proton = host
        .proton_builds
        .first()
        .expect("provision cached UMU-Proton as documented in xtask/runner/README.md");
    let runtime = Arc::new(PrefixRuntime::new(Launcher::Umu {
        umu_run: umu,
        proton_path: proton.to_string_lossy().into_owned(),
        game_id: "umu-default".into(),
    }));
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/windows/legacy-registry-path/setup.exe");
    let node = ProbeEngine::with_builtin_formats(Default::default())
        .probe_path(&source)
        .unwrap()
        .root;
    let spec = RuntimeSpec::ProtonPrefix {
        id: "moving".into(),
    };
    let candidate = Candidate {
        handler: HandlerId("inno".into()),
        strategy: Strategy::RunSilent,
        confidence: 1.0,
        fidelity: Fidelity::Native,
        requires: Requirements {
            executes_code: true,
            runtime: Some(spec.clone()),
            may_prompt_elevation: false,
            interactive: false,
            est_disk_bytes: None,
        },
        steps: vec![Step::Run {
            program: Program::Source(SourcePath::Artifact {
                artifact: node.id.clone(),
                path: ".".into(),
            }),
            args: vec![
                "/VERYSILENT".into(),
                "/SUPPRESSMSGBOXES".into(),
                "/NORESTART".into(),
                "/SP-".into(),
                "/NOICONS".into(),
                format!("/DIR={}", exec::INSTALL_DIR),
            ],
            env: BTreeMap::new(),
            runtime: spec.clone(),
            timeout_secs: 1800,
            expect: vec![
                "FixtureGame.exe".into(),
                "config.ini".into(),
                "unins000.exe".into(),
            ],
        }],
        warnings: vec![],
    };
    let mut executor = Executor::new(
        Arc::new(NativeRuntime::new()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    );
    executor.register_runtime(spec.clone(), runtime.clone());
    let installed = executor
        .execute(
            ExecRequest {
                job_id: "moving-proton-install".into(),
                root: root.path().into(),
                relative_dir: "Proton Moving Game".into(),
                plans: PlanSet {
                    candidates: vec![candidate],
                    selected: Some(0),
                    question: None,
                },
                sources: BTreeMap::from([(node.id.clone(), source.clone())]),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    self_check(&*runtime, &installed.dir);
    let registry_before = registry(&installed.dir);
    assert_eq!(
        registry_before.len(),
        1,
        "genuine installer must write the self-check registry value"
    );
    let scan = scan::scan_references(&ScanInput {
        game_dir: &installed.dir,
        real_path: &installed.dir.to_string_lossy(),
        alias_path: Some(GAME_DIR),
        real_variants: &[],
        registry: &registry_before,
        external_files: &[],
        delegated: false,
        limits: Default::default(),
    })
    .unwrap();
    assert_eq!(scan.verdict, RelocationVerdict::Aliased, "{scan:?}");
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.source = ManifestSource::from_input(&Input::Path(source), None, None).unwrap();
    record.plan.input = record.source.input.clone();
    record.plan.artifacts = vec![node];
    record.handlers = vec![caskade_installer::manifest::HandlerVersion {
        id: installed.candidate.handler.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
    }];
    record.plan.candidate = installed.candidate;
    record.plan.relative_dir = installed.relative_dir;
    record.runtime = spec;
    record.launch_targets.clear();
    record.uninstall_steps.clear();
    record.external_effects.clear();
    record.alias = None;
    record.files.clear();
    inventory(&installed.dir, &installed.dir, &mut record.files);
    record.relocation.verdict = scan.verdict;
    record.relocation.references = scan.references.clone();
    record.relocation.last_path = Some(installed.dir.to_string_lossy().into_owned());
    let manifest = InstallManifest::new(record).unwrap();
    manifest.write(&installed.dir).unwrap();
    let moved = Relocator {
        disk: Arc::new(FileSystemDiskSpace),
        sink: Arc::new(RecordingSink::default()),
        registry: Arc::new(NoRegistry),
        shortcuts: Arc::new(NoShortcuts),
        running: Arc::new(NotRunning),
    }
    .relocate(
        &RelocateRequest {
            job_id: "moving-proton".into(),
            game_dir: installed.dir.clone(),
            target_root: target.path().into(),
            references: scan.references,
            spellings: vec![],
            alias: None,
            force_copy: false,
        },
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(moved.copied);
    assert_eq!((moved.rewritten_files, moved.rewritten_values), (0, 0));
    assert!(!installed.dir.exists());
    let verification = manifest.verify(&moved.game_dir).unwrap();
    assert!(
        verification.changed.is_empty() && verification.missing.is_empty(),
        "{verification:?}"
    );
    assert_eq!(
        registry(&moved.game_dir),
        registry_before,
        "guest registry data must survive without rewriting"
    );
    let g = moved.game_dir.join(".caskade/prefix/dosdevices/g:");
    assert_eq!(fs::read_link(g).unwrap(), Path::new("../drive_g"));
    assert_eq!(
        fs::canonicalize(moved.game_dir.join(".caskade/prefix/drive_g/Game")).unwrap(),
        moved.game_dir.canonicalize().unwrap()
    );
    self_check(&*runtime, &moved.game_dir);
    assert_eq!(
        run(
            &*runtime,
            &moved.game_dir,
            "reg.exe",
            vec![
                "query".into(),
                r"HKLM\Software\CaskadeFixture\FixtureGame".into(),
                "/reg:64".into()
            ]
        ),
        0
    );
    assert_eq!(
        run(
            &*runtime,
            &moved.game_dir,
            &format!(r"{GAME_DIR}\unins000.exe"),
            vec![
                "/VERYSILENT".into(),
                "/SUPPRESSMSGBOXES".into(),
                "/NORESTART".into()
            ]
        ),
        0,
        "the game's own uninstaller must run after the move"
    );
    assert!(
        !moved.game_dir.join("FixtureGame.exe").exists(),
        "own uninstaller left the core payload"
    );
    assert_ne!(
        run(
            &*runtime,
            &moved.game_dir,
            "reg.exe",
            vec![
                "query".into(),
                r"HKLM\Software\CaskadeFixture\FixtureGame".into(),
                "/reg:64".into()
            ]
        ),
        0,
        "own uninstaller left its registry key"
    );
    fs::remove_dir_all(&moved.game_dir).unwrap();
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    root.assert_clean();
}
