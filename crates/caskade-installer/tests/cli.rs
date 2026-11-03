#![cfg(feature = "cli")]

use caskade_installer::{
    manifest::{InstallManifest, ROOT_MANIFEST_PATH},
    testing::TestRoot,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[allow(dead_code)]
#[path = "fixtures/support/mod.rs"]
mod fixtures;

fn binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_caskade-install"));
    command.envs(caskade_installer::testing::windows_runner_path().map(|path| ("PATH", path)));
    command
}
fn run(args: &[&str]) -> Output {
    binary().args(args).output().unwrap()
}
fn value(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}
fn root_clean(root: &TestRoot) {
    // Storage identity deliberately survives uninstall. Assert that it is the
    // only remaining state, remove that test-owned identity, then assert_clean.
    assert!(root.path().join(ROOT_MANIFEST_PATH).is_file());
    fs::remove_file(root.path().join(ROOT_MANIFEST_PATH)).unwrap();
    fs::remove_dir(root.path().join(".caskade")).unwrap();
    root.assert_clean();
}

fn cycle(fixture: &fixtures::Fixture, sidecar: Option<&Path>) {
    let root = TestRoot::new().unwrap();
    let input = fixture.input();
    let execute = |command: &str, argument: &Path| {
        let mut process = binary();
        process.arg("--json");
        if let Some(path) = sidecar {
            process.arg("--seven-zip").arg(path);
        }
        process.arg(command).arg(argument);
        if command == "install" {
            process.arg("--extract");
        }
        if command != "uninstall" {
            process.arg("--root").arg(root.path());
        }
        process.output().unwrap()
    };
    let probed = value(&execute("probe", &input));
    let format = match fixture.expected.format.as_str() {
        "7z" | "7z-multivolume" => "seven_zip",
        "tar.gz" => "gzip",
        "tar.bz2" => "bzip2",
        "tar.xz" => "xz",
        "tar.zst" => "zstd",
        "rar5" | "rar5-multivolume" => "rar",
        "zip-sfx" | "7z-sfx" | "rar-sfx" => "pe",
        "gog-sh" => "mojo_setup",
        other => other,
    };
    assert_eq!(probed["artifacts"][0]["format"], format);
    let planned = value(&execute("plan", &input));
    assert_eq!(planned["plans"]["selected"], 0);
    let native_windows =
        cfg!(windows) && matches!(fixture.expected.format.as_str(), "msi" | "inno" | "nsis");
    assert_eq!(
        planned["plans"]["candidates"][0]["strategy"],
        if native_windows {
            "run_silent"
        } else {
            "extract"
        }
    );
    if fixture.expected.format.ends_with("-sfx") {
        assert!(
            planned["plans"]["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|candidate| candidate["handler"] == "sfx")
        );
    }
    let installed = value(&execute("install", &input));
    assert_eq!(installed["type"], "installed", "{}", fixture.expected.id);
    let game = root
        .path()
        .join(installed["value"]["dir"].as_str().unwrap());
    assert!(game.join(&fixture.expected.launch_target).is_file());
    let manifest = InstallManifest::read(&game).unwrap();
    let verified = manifest.verify(&game).unwrap();
    assert!(verified.missing.is_empty() && verified.changed.is_empty());
    let mut actual = fixtures::snapshot_tree(&game).unwrap();
    actual.retain(|entry| entry.path != ".caskade" && !entry.path.starts_with(".caskade/"));
    let mut expected = fixture.tree.clone();
    if cfg!(windows) {
        for entry in actual.iter_mut().chain(expected.iter_mut()) {
            entry.executable = false;
        }
    }
    assert_eq!(actual, expected, "{}", fixture.expected.id);
    let uninstalled = value(&execute("uninstall", &game));
    assert_eq!(uninstalled["type"], "uninstalled");
    assert_eq!(uninstalled["value"]["game_removed"], true);
    assert!(!game.exists());
    assert!(
        fixture.parts().iter().all(|path| path.is_file()),
        "local sources retained"
    );
    root_clean(&root);
}

#[test]
fn committed_archive_fixtures_complete_the_binary_pipeline() {
    let mut count = 0;
    for fixture in fixtures::load_all(&fixtures_root()).unwrap() {
        if fixture.expected.format.starts_with("rar") {
            continue;
        }
        cycle(&fixture, None);
        count += 1;
    }
    assert_eq!(count, 12);
}

#[cfg(feature = "windows-formats")]
#[test]
fn genuine_native_sfx_fixtures_complete_the_binary_pipeline() {
    let mut count = 0;
    for fixture in fixtures::load_windows(&fixtures_root()).unwrap() {
        if !matches!(fixture.expected.id.as_str(), "zip-sfx" | "sevenzip-sfx") {
            continue;
        }
        cycle(&fixture, None);
        count += 1;
    }
    assert_eq!(count, 2);
}

#[cfg(feature = "windows-formats")]
#[test]
fn genuine_msi_and_inno_installers_complete_the_binary_pipeline() {
    let mut count = 0;
    for fixture in fixtures::load_windows(&fixtures_root()).unwrap() {
        if fixture.expected.id.starts_with("msi-")
            || (fixture.expected.format == "inno"
                && fixture
                    .expected
                    .windows
                    .as_ref()
                    .is_none_or(|w| w.password.is_none()))
        {
            cycle(&fixture, None);
            count += 1;
        }
    }
    assert_eq!(count, 18);
}

#[cfg(feature = "windows-formats")]
#[test]
fn genuine_nsis_fixtures_complete_the_binary_pipeline() {
    let fixtures: Vec<_> = fixtures::load_windows(&fixtures_root())
        .unwrap()
        .into_iter()
        .filter(|fixture| fixture.expected.format == "nsis")
        .collect();
    assert_eq!(fixtures.len(), 6);
    for fixture in fixtures {
        cycle(&fixture, None);
    }
}

#[cfg(feature = "linux-formats")]
#[test]
fn genuine_linux_shell_fixtures_complete_the_binary_pipeline() {
    let fixtures = fixtures::load_linux_shell(&fixtures_root()).unwrap();
    assert_eq!(fixtures.len(), 2);
    for fixture in fixtures {
        if cfg!(target_os = "linux") {
            cycle(&fixture, None);
        } else {
            // A Linux game installs only on Linux: elsewhere nothing is selected.
            let root = TestRoot::new().unwrap();
            let input = fixture.input();
            let planned = value(&run(&[
                "--json",
                "plan",
                input.to_str().unwrap(),
                "--root",
                root.path().to_str().unwrap(),
            ]));
            assert_eq!(planned["plans"]["selected"], Value::Null);
        }
    }
}

#[test]
#[ignore = "requires CASKADE_7ZZ pointing to the pinned official sidecar"]
fn rar_fixtures_complete_the_binary_pipeline_with_the_verified_sidecar() {
    let tool = PathBuf::from(std::env::var_os("CASKADE_7ZZ").expect("set CASKADE_7ZZ"));
    let mut count = 0;
    for fixture in fixtures::load_all(&fixtures_root()).unwrap() {
        if !fixture.expected.format.starts_with("rar") {
            continue;
        }
        cycle(&fixture, Some(&tool));
        count += 1;
    }
    assert_eq!(count, 3);
    #[cfg(feature = "windows-formats")]
    {
        let fixture = fixtures::load_windows(&fixtures_root())
            .unwrap()
            .into_iter()
            .find(|fixture| fixture.expected.id == "rar-sfx")
            .unwrap();
        cycle(&fixture, Some(&tool));
    }
}

#[test]
fn all_host_profiles_and_custom_toml_plan_through_the_binary() {
    let input = fixtures_root().join("bin/unity.zip");
    let mut profiles = std::collections::BTreeMap::new();
    let hosts = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hosts");
    for entry in fs::read_dir(hosts).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_str().unwrap();
        let output = value(&run(&[
            "--json",
            "plan",
            input.to_str().unwrap(),
            "--host",
            name,
        ]));
        let custom = value(&run(&[
            "plan",
            input.to_str().unwrap(),
            "--host",
            path.to_str().unwrap(),
            "--json",
        ]));
        assert_eq!(output, custom);
        profiles.insert(
            name.to_owned(),
            json!({ "host": output["host"], "plans": output["plans"] }),
        );
    }
    assert_eq!(profiles.len(), 6);
    insta::assert_json_snapshot!("host_plans", profiles);
}

#[test]
fn human_output_progress_help_and_structured_errors_are_separate() {
    let root = TestRoot::new().unwrap();
    fs::create_dir_all(root.path().join(".caskade/staging/old-job")).unwrap();
    fs::write(
        root.path().join(".caskade/staging/old-job/unfinished"),
        b"old",
    )
    .unwrap();
    let input = fixtures_root().join("bin/unity.zip");
    let probed = run(&["probe", input.to_str().unwrap()]);
    assert!(probed.status.success());
    assert!(String::from_utf8_lossy(&probed.stdout).contains("Zip"));
    let installed = run(&[
        "install",
        input.to_str().unwrap(),
        "--root",
        root.path().to_str().unwrap(),
    ]);
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    assert!(String::from_utf8_lossy(&installed.stdout).starts_with("Installed "));
    let progress = String::from_utf8_lossy(&installed.stderr);
    assert!(
        progress.contains("install") && progress.contains("bytes") && progress.contains("files")
    );
    let game = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().unwrap() != ".caskade")
        .unwrap();
    let uninstalled = run(&["uninstall", game.to_str().unwrap()]);
    assert!(uninstalled.status.success());
    assert!(String::from_utf8_lossy(&uninstalled.stdout).starts_with("Uninstalled "));
    root_clean(&root);
    let bad_profile = run(&[
        "--json",
        "plan",
        input.to_str().unwrap(),
        "--host",
        "not-a-profile",
    ]);
    assert_eq!(bad_profile.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&bad_profile.stdout).unwrap();
    assert_eq!(error["type"], "error");
    let missing = tempfile::tempdir().unwrap();
    let source = missing.path().join("missing-game");
    let target = missing.path().join("target");
    let failed_move = run(&[
        "--json",
        "move",
        source.to_str().unwrap(),
        target.to_str().unwrap(),
    ]);
    assert_eq!(failed_move.status.code(), Some(1));
    let failure: Value = serde_json::from_slice(&failed_move.stdout).unwrap();
    assert_eq!(failure["type"], "error");
    assert!(!target.exists());
    let help = run(&["--help"]);
    assert!(help.status.success());
    for word in [
        "fetch",
        "probe",
        "plan",
        "install",
        "uninstall",
        "reconcile",
        "move",
        "--json",
    ] {
        assert!(String::from_utf8_lossy(&help.stdout).contains(word));
    }
}

#[test]
fn portable_folder_and_rom_inputs_use_their_registered_handlers() {
    let root = TestRoot::new().unwrap();
    let source = fixtures_root().join("src/layouts/linux");
    let installed = value(&run(&[
        "--json",
        "install",
        source.to_str().unwrap(),
        "--root",
        root.path().to_str().unwrap(),
        "--job-id",
        "x",
    ]));
    assert_eq!(installed["type"], "installed");
    let game = root
        .path()
        .join(installed["value"]["dir"].as_str().unwrap());
    assert_eq!(
        InstallManifest::read(&game)
            .unwrap()
            .record()
            .plan
            .candidate
            .handler
            .0,
        "portable-folder"
    );
    value(&run(&["--json", "uninstall", game.to_str().unwrap()]));
    root_clean(&root);
    let temporary = tempfile::tempdir().unwrap();
    let rom = temporary.path().join("Fixture.nes");
    fs::write(&rom, b"NES\x1a\0\0\0\0\0\0\0\0\0\0\0\0").unwrap();
    let handoff = value(&run(&[
        "--json",
        "install",
        rom.to_str().unwrap(),
        "--root",
        root.path().to_str().unwrap(),
    ]));
    assert_eq!(handoff["type"], "hand_off");
    assert!(rom.is_file());
    root.assert_clean();
}

#[test]
fn incomplete_uninstall_reports_missing_scopes_and_accepts_an_explicit_binding() {
    use caskade_installer::manifest::{ExternalEffect, ExternalLocation, ManifestPath};
    let root = TestRoot::new().unwrap();
    let external = tempfile::tempdir().unwrap();
    fs::write(external.path().join("Game.lnk"), b"created shortcut").unwrap();
    let installed = value(&run(&[
        "--json",
        "install",
        fixtures_root().join("bin/unity.zip").to_str().unwrap(),
        "--root",
        root.path().to_str().unwrap(),
    ]));
    let game = root
        .path()
        .join(installed["value"]["dir"].as_str().unwrap());
    let mut record = InstallManifest::read(&game).unwrap().into_record();
    record.external_effects.push(ExternalEffect::File {
        location: ExternalLocation {
            scope: "test.shortcuts".into(),
            path: ManifestPath::new("Game.lnk").unwrap(),
        },
        original: None,
    });
    InstallManifest::new(record).unwrap().write(&game).unwrap();
    let failed = run(&["--json", "uninstall", game.to_str().unwrap()]);
    assert_eq!(failed.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(report["type"], "error");
    assert_eq!(report["value"]["report"]["game_removed"], false);
    assert_eq!(
        report["value"]["report"]["failures"][0]["reason"]["type"],
        "missing_external_scope"
    );
    assert!(game.is_dir() && external.path().join("Game.lnk").is_file());
    let binding = format!("test.shortcuts={}", external.path().display());
    value(&run(&[
        "--json",
        "uninstall",
        game.to_str().unwrap(),
        "--external-root",
        &binding,
    ]));
    assert!(!external.path().join("Game.lnk").exists());
    root_clean(&root);
}

#[test]
fn ambiguous_and_unsupported_installs_have_distinct_json_exit_statuses() {
    use std::io::Read;
    let root = TestRoot::new().unwrap();
    let source = tempfile::tempdir().unwrap();
    let mut archive =
        zip::ZipArchive::new(fs::File::open(fixtures_root().join("bin/unity.zip")).unwrap())
            .unwrap();
    let mut executable = vec![];
    archive
        .by_name("FixtureGame.exe")
        .unwrap()
        .read_to_end(&mut executable)
        .unwrap();
    fs::write(source.path().join("A.exe"), &executable).unwrap();
    fs::write(source.path().join("B.exe"), &executable).unwrap();
    let output = run(&[
        "--json",
        "install",
        source.path().to_str().unwrap(),
        "--root",
        root.path().to_str().unwrap(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["type"], "needs_user");
    root_clean(&root);
    // A bare Linux binary currently has no install handler and cannot be
    // mistaken for an archive even when a sidecar exists on PATH.
    let binary_path = source.path().join("single.elf");
    let mut elf = vec![0; 64];
    elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    fs::write(&binary_path, elf).unwrap();
    let output = run(&[
        "--json",
        "install",
        binary_path.to_str().unwrap(),
        "--root",
        root.path().to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(3));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["type"], "unsupported");
    root.assert_clean();
}

#[cfg(feature = "fetch")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn urls_download_probe_plan_install_and_uninstall_through_the_binary() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/unity.zip"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(fs::read(fixtures_root().join("bin/unity.zip")).unwrap())
                .insert_header("content-disposition", "attachment; filename=unity.zip"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let root = TestRoot::new().unwrap();
    let url = format!("{}/unity.zip", server.uri());
    let mut installed = Value::Null;
    for command in ["fetch", "probe", "plan", "install"] {
        let args = vec![
            "--json".to_owned(),
            command.into(),
            url.clone(),
            "--root".into(),
            root.path().to_str().unwrap().into(),
        ];
        let output = tokio::task::spawn_blocking(move || binary().args(args).output().unwrap())
            .await
            .unwrap();
        let result = value(&output);
        match command {
            "fetch" => assert_eq!(result["type"], "downloaded"),
            "probe" => assert_eq!(result["artifacts"][0]["format"], "zip"),
            "plan" => assert_eq!(result["plans"]["selected"], 0),
            _ => installed = result,
        }
    }
    assert_eq!(installed["type"], "installed");
    assert!(!root.path().join(".caskade/downloads").exists());
    let game = root
        .path()
        .join(installed["value"]["dir"].as_str().unwrap());
    value(&run(&["--json", "uninstall", game.to_str().unwrap()]));
    root_clean(&root);
}

#[cfg(all(unix, feature = "fetch"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_c_cancels_a_slow_download_and_exits_after_cleanup() {
    use std::{process::Stdio, time::Duration};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(5))
                .set_body_bytes(b"later".as_slice()),
        )
        .mount(&server)
        .await;
    let root = TestRoot::new().unwrap();
    let child = binary()
        .args([
            "--json",
            "install",
            &format!("{}/slow.zip", server.uri()),
            "--root",
            root.path().to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = rustix::process::Pid::from_raw(child.id().try_into().unwrap()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while server.received_requests().await.unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::INT).unwrap();
    let mut waiting = tokio::task::spawn_blocking(move || child.wait_with_output().unwrap());
    let output = match tokio::time::timeout(Duration::from_secs(3), &mut waiting).await {
        Ok(output) => output.unwrap(),
        Err(_) => {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            let _ = waiting.await;
            panic!("Ctrl-C did not finish promptly");
        }
    };
    assert_eq!(
        output.status.code(),
        Some(130),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["type"], "cancelled");
    assert!(!root.path().join(".caskade/staging").exists());
    let fetch = caskade_installer::fetch::FetchEngine::new(
        Default::default(),
        std::sync::Arc::new(caskade_installer::testing::FakeDiskSpace::fixed(u64::MAX)),
        std::sync::Arc::new(caskade_installer::testing::RecordingSink::default()),
    )
    .unwrap();
    fetch.cleanup_orphans(root.path(), &[]).await.unwrap();
    root.assert_clean();
}

#[cfg(all(feature = "linux-formats", target_os = "linux"))]
#[test]
fn package_desktop_metadata_selects_its_exact_launcher_through_the_binary() {
    let fixture = fixtures::load_packages(&fixtures_root())
        .unwrap()
        .into_iter()
        .find(|fixture| fixture.expected.id == "deb-self-contained")
        .unwrap();
    let root = TestRoot::new().unwrap();
    let installed = value(&run(&[
        "--json",
        "install",
        fixture.input().to_str().unwrap(),
        "--extract",
        "--root",
        root.path().to_str().unwrap(),
    ]));
    assert_eq!(installed["type"], "installed");
    assert_eq!(
        installed["value"]["launch_targets"][0]["path"],
        fixture.expected.launch_target
    );
    let game = root
        .path()
        .join(installed["value"]["dir"].as_str().unwrap());
    let manifest = InstallManifest::read(&game).unwrap();
    let verified = manifest.verify(&game).unwrap();
    assert!(verified.changed.is_empty() && verified.missing.is_empty());
    let desktop =
        fs::read_to_string(game.join("usr/share/applications/caskade-fixture.desktop")).unwrap();
    assert!(desktop.contains(game.to_str().unwrap()));
    assert!(!desktop.contains(".caskade/staging"));
    value(&run(&["--json", "uninstall", game.to_str().unwrap()]));
    root_clean(&root);
}

#[cfg(all(feature = "disc", feature = "windows-formats"))]
#[test]
#[ignore = "requires CASKADE_7ZZ pointing to the pinned official sidecar"]
fn genuine_disc_inputs_install_nested_payloads_and_hand_off_console_members() {
    use caskade_installer::Input;
    let tool = PathBuf::from(std::env::var_os("CASKADE_7ZZ").expect("set pinned CASKADE_7ZZ"));
    let fixtures = fixtures::load_disc(&fixtures_root()).unwrap();
    let mut pc = 0;
    for fixture in fixtures
        .iter()
        .filter(|fixture| fixture.expected.format == "iso")
    {
        pc += 1;
        let root = TestRoot::new().unwrap();
        let installed = value(&run(&[
            "--json",
            "--seven-zip",
            tool.to_str().unwrap(),
            "install",
            fixture.input().to_str().unwrap(),
            "--extract",
            "--root",
            root.path().to_str().unwrap(),
        ]));
        assert_eq!(installed["type"], "installed", "{}", fixture.expected.id);
        let game = root
            .path()
            .join(installed["value"]["dir"].as_str().unwrap());
        let manifest = InstallManifest::read(&game).unwrap();
        assert_eq!(
            manifest.record().source.input,
            Input::Path(fixture.input().file_name().unwrap().into())
        );
        assert_eq!(
            manifest.record().plan.artifacts[0].format,
            caskade_installer::Format::Inno
        );
        let mut actual = fixtures::snapshot_tree(&game).unwrap();
        actual.retain(|entry| entry.path != ".caskade" && !entry.path.starts_with(".caskade/"));
        let mut expected = fixture.tree.clone();
        if cfg!(windows) {
            for entry in actual.iter_mut().chain(expected.iter_mut()) {
                entry.executable = false;
            }
        }
        assert_eq!(actual, expected, "{}", fixture.expected.id);
        value(&run(&["--json", "uninstall", game.to_str().unwrap()]));
        root_clean(&root);
        assert!(fixture.input().is_file());
    }
    assert_eq!(pc, 2);
    let fixture = fixtures
        .iter()
        .find(|fixture| fixture.expected.id == "ps1-cue-bin")
        .unwrap();
    for input in fixture.parts() {
        let root = TestRoot::new().unwrap();
        let outcome = value(&run(&[
            "--json",
            "--seven-zip",
            tool.to_str().unwrap(),
            "install",
            input.to_str().unwrap(),
            "--root",
            root.path().to_str().unwrap(),
        ]));
        assert_eq!(outcome["type"], "hand_off");
        assert_eq!(outcome["value"]["type"], "rom");
        assert_eq!(outcome["value"]["value"]["platform"], "sony_playstation");
        assert_eq!(
            outcome["value"]["value"]["files"].as_array().unwrap().len(),
            2
        );
        root.assert_clean();
    }
}
