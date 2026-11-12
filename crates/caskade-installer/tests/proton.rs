//! #45 AC under real Proton (native profile: CASKADE_TEST_NATIVE=1). Needs a
//! Proton build (PROTONPATH or Steam's compatibility tools) and network for
//! the first umu run; umu-launcher is bootstrapped user-locally.
#![cfg(all(target_os = "linux", feature = "fetch", feature = "windows-formats"))]

use caskade_installer::{
    exec::{self, CancellationToken, ExecRequest, Executor},
    host::HostCaps,
    runtime::{
        ProcessCommand, Runtime,
        proton::{Launcher, PrefixRuntime},
        umu::UmuBootstrap,
    },
    testing::{FakeDiskSpace, RecordingSink, TestRoot},
    *,
};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc, time::Duration};

fn native() -> bool {
    let enabled = std::env::var_os("CASKADE_TEST_NATIVE").is_some();
    if !enabled {
        eprintln!("skipping: set CASKADE_TEST_NATIVE=1 (native nextest profile)");
    }
    enabled
}

fn proton_runtime(umu_run: std::path::PathBuf) -> PrefixRuntime {
    let host = HostCaps::detect();
    let proton = host
        .proton_builds
        .first()
        .expect("a Proton build (GE-Proton/UMU-Proton/Steam Proton) on this host")
        .clone();
    eprintln!("Proton build: {}", proton.display());
    PrefixRuntime::new(Launcher::Umu {
        umu_run,
        proton_path: proton.to_string_lossy().into_owned(),
        game_id: "umu-default".into(),
    })
}

/// Installs `setup` silently to `G:\` (the game directory) under Proton,
/// then launches the installed game, which must not be able to read Z:.
async fn install_and_launch(fixture: &str, args: &[&str]) {
    let umu = UmuBootstrap::for_host().unwrap();
    let umu_run = umu.ensure().await.unwrap();
    let runtime = Arc::new(proton_runtime(umu_run));
    let setup = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/windows")
        .join(fixture)
        .join("setup.exe");
    let spec = RuntimeSpec::ProtonPrefix {
        id: "default".into(),
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
                artifact: ArtifactId("setup".into()),
                path: ".".into(),
            }),
            args: args
                .iter()
                .map(|arg| arg.replace("{dir}", exec::INSTALL_DIR))
                .collect(),
            env: BTreeMap::new(),
            runtime: spec.clone(),
            timeout_secs: 1800,
            expect: vec!["FixtureGame.exe".into()],
        }],
        warnings: vec![],
    };
    let root = TestRoot::new().unwrap();
    let mut executor = Executor::new(
        Arc::new(caskade_installer::runtime::NativeRuntime::new()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    );
    executor.register_runtime(spec, runtime.clone());
    let report = executor
        .execute(
            ExecRequest {
                job_id: format!("proton-{fixture}"),
                root: root.path().into(),
                relative_dir: "Fixture Game".into(),
                plans: PlanSet {
                    candidates: vec![candidate],
                    selected: Some(0),
                    question: None,
                },
                sources: BTreeMap::from([(ArtifactId("setup".into()), setup)]),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let game = dunce::canonicalize(&report.dir).unwrap();
    assert!(game.join("FixtureGame.exe").is_file());
    let prefix = game.join(".caskade/prefix");
    assert_eq!(
        fs::read_link(prefix.join("drive_g/Game")).unwrap(),
        Path::new("../../..")
    );
    // Proton recreates a missing z: as /, so it points at an empty directory.
    assert_eq!(
        fs::read_dir(prefix.join("dosdevices/z:")).unwrap().count(),
        0
    );

    // Launch the installed game through Proton from its final location,
    // with Proton's log of exceptions, processes and loaded modules, which a
    // failure prints (#123). In the game's folder: the only one the
    // container may write.
    let logs = game.join(".caskade/proton-log");
    fs::create_dir_all(&logs).unwrap();
    let launch = ProcessCommand {
        program: game.join("FixtureGame.exe"),
        args: vec![
            "--token".into(),
            format!(
                r"{}\launch-token",
                caskade_installer::runtime::proton::GAME_DIR
            ),
            "--check-z-drive".into(),
        ],
        env: BTreeMap::from([
            ("PROTON_LOG".into(), "+seh,+process,+loaddll".into()),
            ("PROTON_LOG_DIR".into(), logs.to_string_lossy().into_owned()),
        ]),
        cwd: game.clone(),
    };
    let process = runtime.spawn(&launch).unwrap();
    let exit = runtime.wait(process, Duration::from_secs(600));
    runtime.kill(process).unwrap();
    let token = fs::read_to_string(game.join("launch-token")).unwrap_or_default();
    let code = exit.unwrap().code;
    assert_eq!(
        code,
        0,
        "token: {token}\nexit {code}{}\n{}",
        signal_of(code),
        proton_log(&logs)
    );
    assert!(
        token
            .trim_end()
            .to_ascii_lowercase()
            .ends_with(r"\fixturegame.exe"),
        "{token}"
    );
    assert!(!token.contains("sandbox leak"), "{token}");
    root.assert_sentinel_untouched();
}

/// What an exit code says about a signal: umu and Proton are Python, whose
/// `sys.exit(-N)` for a child killed by signal N exits with 256 - N, and
/// a shell or pressure-vessel's own convention is 128 + N.
fn signal_of(code: i32) -> String {
    if (129..=255).contains(&code) {
        format!(
            " (a child killed by signal {} if from Python's sys.exit, {} if 128 + signal)",
            256 - code,
            code - 128
        )
    } else {
        String::new()
    }
}

/// The end of Proton's logs in `dir`.
fn proton_log(dir: &Path) -> String {
    let mut out = String::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let text = fs::read_to_string(entry.path()).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(120)..].join("\n");
        out.push_str(&format!("--- {}\n{tail}\n", entry.path().display()));
    }
    if out.is_empty() {
        out.push_str("(no Proton log)");
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn nsis_installs_under_proton_to_g_and_the_game_cannot_read_z() {
    if native() {
        install_and_launch("nsis3-unicode-nonsolid", &["/S", "/D={dir}"]).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn inno_installs_under_proton_to_g_and_the_game_cannot_read_z() {
    if native() {
        install_and_launch(
            "inno60-basic",
            &[
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/SP-",
                "/CURRENTUSER",
                "/DIR={dir}",
            ],
        )
        .await;
    }
}

/// #46 AC: `legacy-registry-path` extracted on Linux, with its [INI] and
/// [Registry] effects replayed into the Proton prefix ({app} = G:\Game), so
/// the fixture game's `--self-check` (config.ini + HKLM InstallPath) passes.
#[tokio::test(flavor = "multi_thread")]
async fn extracted_legacy_registry_path_self_checks_under_proton() {
    use caskade_installer::formats::windows::inno::{InnoExtractor, InnoLimits, inspect_path};
    if !native() {
        return;
    }
    let umu_run = UmuBootstrap::for_host().unwrap().ensure().await.unwrap();
    let runtime = Arc::new(proton_runtime(umu_run));
    let setup = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/windows/legacy-registry-path/setup.exe");
    let spec = RuntimeSpec::ProtonPrefix {
        id: "default".into(),
    };
    let artifact = ArtifactId("setup".into());
    let metadata = inspect_path(&setup, InnoLimits::default()).unwrap();
    let mut steps = vec![Step::Extract {
        from: artifact.clone(),
        select: vec![],
        strip: None,
        to: ".".into(),
    }];
    steps.extend(metadata.prefix_replay_steps(spec.clone()).unwrap());
    let candidate = Candidate {
        handler: HandlerId("inno".into()),
        strategy: Strategy::Extract,
        confidence: 1.0,
        fidelity: Fidelity::Exact,
        requires: Requirements {
            executes_code: true,
            runtime: Some(spec.clone()),
            may_prompt_elevation: false,
            interactive: false,
            est_disk_bytes: None,
        },
        steps,
        warnings: vec![],
    };
    let sources = BTreeMap::from([(artifact, setup)]);
    let root = TestRoot::new().unwrap();
    let mut executor = Executor::new(
        Arc::new(caskade_installer::runtime::NativeRuntime::new()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(Arc::new(InnoExtractor::new(sources.clone())));
    executor.register_runtime(spec, runtime.clone());
    let report = executor
        .execute(
            ExecRequest {
                job_id: "proton-legacy".into(),
                root: root.path().into(),
                relative_dir: "Legacy Game".into(),
                plans: PlanSet {
                    candidates: vec![candidate],
                    selected: Some(0),
                    question: None,
                },
                sources,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let game = dunce::canonicalize(&report.dir).unwrap();
    let game_dir = caskade_installer::runtime::proton::GAME_DIR;
    assert!(
        fs::read_to_string(game.join("config.ini"))
            .unwrap()
            .contains(&format!("InstallPath={game_dir}"))
    );
    let registry = caskade_installer::runtime::registry::parse_prefix_reg(
        &fs::read_to_string(game.join(".caskade/prefix/system.reg")).unwrap(),
        "HKLM",
    )
    .unwrap();
    assert!(registry.iter().any(|value| {
        value
            .key
            .eq_ignore_ascii_case(r"Software\CaskadeFixture\FixtureGame")
            && value.name == "InstallPath"
    }));
    // Launch by its Windows path so the game sees itself under G:\Game.
    let launch = ProcessCommand {
        program: format!(r"{game_dir}\FixtureGame.exe").into(),
        args: vec![
            "--token".into(),
            format!(r"{game_dir}\self-check"),
            "--self-check".into(),
            "--check-z-drive".into(),
        ],
        env: BTreeMap::new(),
        cwd: game.clone(),
    };
    let process = runtime.spawn(&launch).unwrap();
    let exit = runtime.wait(process, Duration::from_secs(600));
    runtime.kill(process).unwrap();
    let token = fs::read_to_string(game.join("self-check")).unwrap_or_default();
    assert_eq!(token, "ok\n");
    assert_eq!(exit.unwrap().code, 0);
    root.assert_sentinel_untouched();
}
