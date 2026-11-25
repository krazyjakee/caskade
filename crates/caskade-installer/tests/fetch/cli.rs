use std::fs;

use caskade_installer::testing::TestRoot;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

use super::checksum;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn install_cli_downloads_a_real_zipped_fixture_and_writes_a_verified_manifest() {
    let server = MockServer::start().await;
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/unity.zip");
    let body = fs::read(fixture).unwrap();
    Mock::given(path("/game.zip"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body.clone())
                .insert_header("content-disposition", "attachment; filename=unity.zip"),
        )
        .mount(&server)
        .await;
    let root = TestRoot::new().unwrap();
    let target_root = root.path().to_owned();
    let url = format!("{}/game.zip#sha256={}", server.uri(), checksum(&body));
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .envs(caskade_installer::testing::windows_runner_path().map(|path| ("PATH", path)))
            .arg("--json")
            .arg("install")
            .arg(url)
            .arg("--root")
            .arg(target_root)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(outcome["type"], "installed");
    let game = root.path().join(outcome["value"]["dir"].as_str().unwrap());
    assert!(game.join("FixtureGame.exe").is_file());
    let manifest = caskade_installer::manifest::InstallManifest::read(&game).unwrap();
    assert_eq!(
        manifest.record().source.checksum.as_ref().unwrap().as_str(),
        checksum(&body)
    );
    assert!(matches!(
        manifest.record().source.input,
        caskade_installer::Input::Urls(_)
    ));
    let verification = manifest.verify(&game).unwrap();
    assert!(verification.missing.is_empty() && verification.changed.is_empty());
    assert!(!root.path().join(".caskade/downloads").exists());
    root.assert_sentinel_untouched();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn install_cli_accepts_a_local_archive_without_deleting_the_source() {
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/unity.zip");
    let root = TestRoot::new().unwrap();
    let source = fixture.clone();
    let target_root = root.path().to_owned();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .envs(caskade_installer::testing::windows_runner_path().map(|path| ("PATH", path)))
            .arg("--json")
            .arg("install")
            .arg(source)
            .arg("--root")
            .arg(target_root)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.exists());
    root.assert_sentinel_untouched();
}
