//! #62 T7: real C: -> D: moves. Only the dedicated native Windows lane runs this.
#![cfg(all(windows, feature = "windows-formats"))]

use caskade_installer::{
    Input, PlanSet, RuntimeSpec, Strategy,
    disk::FileSystemDiskSpace,
    exec::{CancellationToken, ExecRequest, Executor},
    formats::windows::{inno::InnoHandler, msi::MsiHandler},
    host::HostCaps,
    manifest::{
        ExternalEffect, InstallManifest, InstalledFile, ManifestPath, ManifestSource,
        RelocationVerdict, RootManifest, Sha256Digest, UninstallRequest, Uninstaller,
        native::{PlatformRegistry, RegistryEffect, uninstall_registration_effects},
    },
    plan::{PlanEngine, Policy},
    probe::ProbeEngine,
    relocate::{
        RelocateRequest, Relocator, RunningCheck,
        backends::{ShellShortcuts, WindowsRegistry},
    },
    runtime::{NativeRuntime, ProcessCommand, Runtime, alias::AliasStore},
    scan::{self, ScanInput},
    testing::{FakeDiskSpace, RecordingSink},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const CUSTOM_KEY: &str = r"Software\CaskadeFixture\FixtureGame";
// Each known key is checked absent before arming this guard. Failures must not
// leave installer state behind for the next serial native-profile case.
struct Cleanup {
    game: PathBuf,
    product_code: Option<String>,
    uninstall_key: String,
    alias: Option<String>,
    aliases: AliasStore,
    armed: bool,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let runtime = NativeRuntime::new();
        let (program, args) = if let Some(code) = &self.product_code {
            (
                PathBuf::from("msiexec"),
                vec!["/x".into(), code.clone(), "/qn".into(), "/norestart".into()],
            )
        } else {
            (
                self.game.join("unins000.exe"),
                vec![
                    "/VERYSILENT".into(),
                    "/SUPPRESSMSGBOXES".into(),
                    "/NORESTART".into(),
                ],
            )
        };
        if let Ok(pid) = runtime.spawn(&ProcessCommand {
            program,
            args,
            env: BTreeMap::new(),
            cwd: std::env::temp_dir(),
        }) {
            let _ = runtime.wait(pid, Duration::from_secs(60));
            let _ = runtime.kill(pid);
        }
        if let Some(id) = &self.alias {
            let _ = self.aliases.remove(id);
        }
        use winreg::{
            RegKey,
            enums::{
                HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_WOW64_32KEY,
                KEY_WOW64_64KEY,
            },
        };
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
                if let Ok(key) =
                    RegKey::predef(hive).open_subkey_with_flags("Software", KEY_ALL_ACCESS | view)
                {
                    let _ = key.delete_subkey_all(CUSTOM_KEY.trim_start_matches("Software\\"));
                    let _ = key.delete_subkey_all(format!(
                        "Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
                        self.uninstall_key
                    ));
                }
            }
        }
    }
}
struct NotRunning;
impl RunningCheck for NotRunning {
    fn running(&self, _: &Path) -> bool {
        false
    }
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
            let bytes = fs::read(&path).unwrap();
            files.push(InstalledFile {
                path: ManifestPath::new(
                    path.strip_prefix(game)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace('\\', "/"),
                )
                .unwrap(),
                size: metadata.len(),
                sha256: Some(Sha256Digest::new(format!("{:x}", Sha256::digest(&bytes))).unwrap()),
            });
        }
    }
}

fn self_check(runtime: &dyn Runtime, game: &Path, launch_dir: &Path) {
    let token = game.join("move-self-check");
    let id = runtime
        .spawn(&ProcessCommand {
            program: launch_dir.join("FixtureGame.exe"),
            args: vec![
                "--self-check".into(),
                "--token".into(),
                token.to_string_lossy().into_owned(),
            ],
            env: BTreeMap::new(),
            cwd: launch_dir.into(),
        })
        .unwrap();
    let exit = runtime.wait(id, Duration::from_secs(60));
    runtime.kill(id).unwrap();
    assert_eq!(
        exit.unwrap().code,
        0,
        "self-check token: {:?}",
        fs::read_to_string(&token)
    );
    assert_eq!(fs::read_to_string(&token).unwrap(), "ok\n");
    fs::remove_file(token).unwrap();
}

#[tokio::test]
#[ignore = "native C: and D: volumes, HKLM registry and installers; CASKADE_TEST_NATIVE=1"]
async fn c_to_d_minis_self_check_and_use_the_safe_uninstall_route_with_and_without_aliases() {
    if std::env::var("CASKADE_TEST_NATIVE").as_deref() != Ok("1") {
        return;
    }
    let target_parent = std::env::var_os("CASKADE_TEST_MOVE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\CaskadeTests"));
    fs::create_dir_all(&target_parent).expect("provision the documented writable D: test volume");
    assert!(
        target_parent
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("D:"),
        "target must be the runner's second D: volume"
    );
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/windows");
    let mut evidence = vec![];
    for id in ["gog-inno-unity", "legacy-registry-path", "msi-plus-cab"] {
        for aliased in [true, false] {
            let old = tempfile::tempdir().unwrap();
            let old_root = old.path().to_path_buf();
            let drive = old_root
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_ascii_uppercase();
            assert!(
                drive.starts_with("C:"),
                "Windows native TEMP must be on C:, got {}",
                old_root.display()
            );
            let target = tempfile::tempdir_in(&target_parent).unwrap();
            let target_root = target.path().to_path_buf();
            RootManifest::new("moving-windows-c")
                .unwrap()
                .write(&old_root)
                .unwrap();
            RootManifest::new("moving-windows-d")
                .unwrap()
                .write(&target_root)
                .unwrap();
            let alias_home = tempfile::tempdir().unwrap();
            let aliases = AliasStore::new(alias_home.path().join("moving aliases"));
            assert!(
                scan::registry::read_keys(&[("HKLM".into(), CUSTOM_KEY.into())], 128).is_empty(),
                "fixture key must be absent before installation"
            );
            let source = fixture_root.join(id).join(if id == "msi-plus-cab" {
                "setup.msi"
            } else {
                "setup.exe"
            });
            let node = ProbeEngine::with_builtin_formats(Default::default())
                .probe_path(&source)
                .unwrap()
                .root;
            let product_code = (id == "msi-plus-cab").then(|| {
                caskade_installer::formats::windows::msi::metadata_from_artifact(&node)
                    .unwrap()
                    .properties["ProductCode"]
                    .clone()
            });
            let uninstall_key = product_code
                .clone()
                .unwrap_or_else(|| "CaskadeFixture_is1".into());
            let uninstall_path =
                format!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{uninstall_key}");
            assert!(
                scan::registry::read_keys(
                    &[
                        ("HKCU".into(), uninstall_path.clone()),
                        ("HKLM".into(), uninstall_path)
                    ],
                    128
                )
                .is_empty(),
                "fixture Uninstall registration must be absent before installation"
            );
            let mut planner = PlanEngine::new();
            planner.register(Box::new(InnoHandler::new())).unwrap();
            planner.register(Box::new(MsiHandler::new())).unwrap();
            // detect() leaves the filesystem unmeasured (not writable), which
            // rules out every candidate that writes the game folder.
            let mut host = HostCaps::detect();
            host.filesystem.writable = true;
            let plans = planner
                .plan(std::slice::from_ref(&node), &host, &Policy::default())
                .unwrap();
            let candidate = plans
                .candidates
                .into_iter()
                .find(|c| c.strategy == Strategy::RunSilent)
                .expect("native silent candidate");
            let runtime = Arc::new(NativeRuntime::new());
            let mut executor = Executor::new(
                runtime.clone(),
                Arc::new(FakeDiskSpace::fixed(u64::MAX)),
                Arc::new(RecordingSink::default()),
            )
            .with_observer(Arc::new(caskade_installer::runtime::RegistryWatcher));
            if aliased {
                executor = executor.with_aliases(aliases.clone(), "moving-windows-c");
            }
            let mut cleanup = Cleanup {
                game: old_root.join("Moving Game"),
                product_code,
                uninstall_key,
                alias: None,
                aliases: aliases.clone(),
                armed: true,
            };
            let report = executor
                .execute(
                    ExecRequest {
                        job_id: format!("move-{id}-{aliased}"),
                        root: old_root.clone(),
                        relative_dir: "Moving Game".into(),
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
            cleanup.alias = report.alias.clone();
            assert!(report.failures.is_empty());
            assert_eq!(report.alias.is_some(), aliased);
            let alias_path = report.alias.as_ref().map(|id| aliases.path(id).unwrap());
            self_check(
                &*runtime,
                &report.dir,
                alias_path.as_deref().unwrap_or(&report.dir),
            );
            if aliased && id == "gog-inno-unity" {
                // Reproduce a game's post-launch config resolving its junction
                // to the real path. Its installer's binary log still uses the
                // alias: this is a genuinely Rewritable GOG-style installation,
                // alongside the Aliased MSI and no-alias Fragile cases.
                fs::write(
                    report.dir.join("config.ini"),
                    format!("[Game]\r\nInstallPath={}\r\n", report.dir.display()),
                )
                .unwrap();
                self_check(&*runtime, &report.dir, alias_path.as_deref().unwrap());
            }
            let registrations =
                uninstall_registration_effects(&report.dir, &PlatformRegistry).unwrap();
            assert!(
                !registrations.is_empty(),
                "fixture must provide its genuine native uninstaller"
            );
            let mut keys = vec![("HKLM".into(), CUSTOM_KEY.into())];
            for effect in &registrations {
                if let ExternalEffect::Registry {
                    effect: RegistryEffect::UninstallKey { location, .. },
                } = effect
                {
                    let hive = if location.scope.contains("hkcu") {
                        "HKCU"
                    } else {
                        "HKLM"
                    };
                    keys.push((hive.into(), location.path.as_str().replace('/', "\\")));
                }
            }
            let values = scan::registry::read_keys(&keys, 4096);
            let scan = scan::scan_references(&ScanInput {
                game_dir: &report.dir,
                real_path: &report.dir.to_string_lossy(),
                alias_path: alias_path.as_deref().and_then(Path::to_str),
                real_variants: &[],
                registry: &values,
                external_files: &[],
                delegated: false,
                limits: Default::default(),
            })
            .unwrap();
            if aliased && id == "gog-inno-unity" {
                assert_eq!(scan.verdict, RelocationVerdict::Rewritable, "{scan:?}");
            } else if aliased {
                assert_eq!(scan.verdict, RelocationVerdict::Aliased, "{id}: {scan:?}");
            } else {
                assert!(
                    matches!(
                        scan.verdict,
                        RelocationVerdict::Rewritable | RelocationVerdict::Fragile
                    ),
                    "{id}: {scan:?}"
                );
            }
            let mut record =
                InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
                    .unwrap()
                    .into_record();
            record.source = ManifestSource::from_input(&Input::Path(source), None, None).unwrap();
            record.plan.input = record.source.input.clone();
            record.plan.artifacts = vec![node.clone()];
            record.plan.candidate = report.candidate.clone();
            record.plan.root_id = "moving-windows-c".into();
            record.plan.relative_dir = report.relative_dir.clone();
            record.handlers = vec![caskade_installer::manifest::HandlerVersion {
                id: report.candidate.handler.clone(),
                version: env!("CARGO_PKG_VERSION").into(),
            }];
            record.runtime = RuntimeSpec::Native;
            record.alias = report.alias.clone();
            record.launch_targets.clear();
            record.files.clear();
            inventory(&report.dir, &report.dir, &mut record.files);
            record.uninstall_steps = caskade_installer::formats::windows::uninstall_steps(
                std::slice::from_ref(&node),
                &report.candidate,
            );
            assert_eq!(
                record.uninstall_steps.len(),
                1,
                "native uninstaller must be recorded"
            );
            // As the pipeline records them: what the registry watcher saw the
            // installer create, then its Uninstall registrations.
            record.external_effects = caskade_installer::manifest::effects_from_journal(
                &report.journal,
                &BTreeMap::new(),
            )
            .unwrap();
            assert!(
                record.external_effects.iter().any(|effect| matches!(
                    effect,
                    ExternalEffect::Registry {
                        effect: RegistryEffect::Value { name, .. }
                    } if name == "InstallPath"
                )),
                "{id}: the installer's InstallPath value was not recorded: {:?}",
                record.external_effects
            );
            record.external_effects.extend(registrations);
            record.relocation.verdict = scan.verdict;
            record.relocation.references = scan.references.clone();
            record.relocation.last_path = Some(report.dir.to_string_lossy().into_owned());
            InstallManifest::new(record)
                .unwrap()
                .write(&report.dir)
                .unwrap();
            let mover = Relocator {
                disk: Arc::new(FileSystemDiskSpace),
                sink: Arc::new(RecordingSink::default()),
                registry: Arc::new(WindowsRegistry {
                    runtime: runtime.clone(),
                    scratch: alias_home.path().join("registry scratch"),
                }),
                shortcuts: Arc::new(ShellShortcuts {
                    runtime: runtime.clone(),
                }),
                running: Arc::new(NotRunning),
            };
            let moved = mover
                .relocate(
                    &RelocateRequest {
                        job_id: format!("c-to-d-{id}"),
                        game_dir: report.dir.clone(),
                        target_root: target_root.clone(),
                        references: scan.references,
                        spellings: vec![],
                        alias: report.alias.clone().map(|id| (aliases.clone(), id)),
                        force_copy: false,
                    },
                    &CancellationToken::new(),
                )
                .unwrap();
            cleanup.game = moved.game_dir.clone();
            assert!(moved.copied, "C: to D: must use actual cross-volume copy");
            assert!(!report.dir.exists());
            self_check(
                &*runtime,
                &moved.game_dir,
                alias_path.as_deref().unwrap_or(&moved.game_dir),
            );
            if scan.verdict == RelocationVerdict::Aliased {
                assert_eq!((moved.rewritten_files, moved.rewritten_values), (0, 0));
            } else if aliased && id == "gog-inno-unity" {
                assert!(
                    moved.rewritten_files > 0,
                    "the real-path GOG config must be rewritten"
                );
            }
            let current_values = scan::registry::read_keys(&keys, 4096);
            let remaining = scan::scan_references(&ScanInput {
                game_dir: &moved.game_dir,
                real_path: &report.dir.to_string_lossy(),
                alias_path: None,
                real_variants: &[],
                registry: &current_values,
                external_files: &[],
                delegated: false,
                limits: Default::default(),
            })
            .unwrap();
            assert!(
                remaining
                    .references
                    .iter()
                    .all(|reference| !reference.location.rewritable()),
                "move left rewritable C: references: {remaining:?}"
            );
            if aliased {
                assert!(
                    remaining.references.is_empty(),
                    "aliased install retained real-path evidence: {remaining:?}"
                );
            }
            let manifest = InstallManifest::read(&moved.game_dir).unwrap();
            assert_eq!(manifest.record().plan.root_id, "moving-windows-d");
            let verify = manifest.verify(&moved.game_dir).unwrap();
            assert!(
                verify.changed.is_empty() && verify.missing.is_empty(),
                "{verify:?}"
            );
            let verdict = manifest.record().relocation.verdict;
            let removed = Uninstaller::new(executor)
                .with_aliases(aliases.clone())
                .uninstall(
                    UninstallRequest {
                        job_id: format!("remove-moved-{id}"),
                        root: target_root.clone(),
                        relative_dir: report.relative_dir,
                        manifest,
                    },
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            let native = removed
                .native
                .as_ref()
                .expect("recorded native uninstaller report");
            if matches!(
                verdict,
                RelocationVerdict::Free | RelocationVerdict::Aliased
            ) {
                assert!(native.attempted && native.succeeded, "{removed:?}");
            } else {
                assert!(
                    !native.attempted && native.manifest_fallback,
                    "unsafe Inno/MSI state must use manifest cleanup: {removed:?}"
                );
            }
            assert!(removed.is_clean(), "{removed:?}");
            assert!(!moved.game_dir.exists());
            assert!(
                scan::registry::read_keys(&keys, 4096).is_empty(),
                "uninstall left fixture registry state"
            );
            evidence.push(serde_json::json!({
                "fixture": id,
                "alias": aliased,
                "verdict": verdict,
                "copied": moved.copied,
                "self_check": "passed",
                "native_attempted": native.attempted,
                "native_succeeded": native.succeeded,
                "manifest_fallback": native.manifest_fallback,
                "native_diagnostics": &native.diagnostics,
                "root_clean": removed.is_clean(),
            }));
            if let Some(id) = report.alias {
                assert!(aliases.target(&id).unwrap().is_none());
            }
            // A no-alias MSI intentionally used manifest cleanup. The test
            // teardown also unregisters its cached MSI product, whose private
            // component database is outside the manifest's recorded effects.
            cleanup.armed = !aliased && id == "msi-plus-cab";
        }
    }
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/native-windows");
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("moving.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}
