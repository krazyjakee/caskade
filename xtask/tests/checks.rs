use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;
use xtask::{
    checks::{self, FIXTURE_ROOT, GAME_ROOT},
    fixtures::{self, EntryKind, Expectation, TreeEntry},
    generate,
};

// This is a small, synthetic workspace for check-failure tests. No repository
// checkout is copied, and deliberate corruptions never touch committed inputs.
fn sample_workspace() -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    for directory in [
        "xtask/src",
        "xtask/tests",
        &format!("{GAME_ROOT}/src"),
        &format!("{GAME_ROOT}/tests"),
        &format!("{FIXTURE_ROOT}/support"),
        &format!("{FIXTURE_ROOT}/src"),
        &format!("{FIXTURE_ROOT}/bin"),
        &format!("{FIXTURE_ROOT}/handmade"),
        &format!("{FIXTURE_ROOT}/expected"),
    ] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    for file in [
        "xtask/Cargo.toml",
        "xtask/windows-fixtures.py",
        "xtask/src/main.rs",
        &format!("{GAME_ROOT}/Cargo.toml"),
        &format!("{GAME_ROOT}/Cargo.lock"),
        &format!("{GAME_ROOT}/src/main.rs"),
    ] {
        fs::write(root.join(file), "synthetic source\n").unwrap();
    }
    let fixtures = root.join(FIXTURE_ROOT);
    fs::write(
        fixtures.join("src/builders.toml"),
        "rustc = \"test-version\"\n",
    )
    .unwrap();
    fs::write(fixtures.join("src/layout.txt"), "layout v1\n").unwrap();
    fs::write(fixtures.join("bin/sample.input"), b"payload").unwrap();
    let expected = Expectation {
        schema_version: 1,
        id: "sample".into(),
        title: "Sample".into(),
        layout: "sample".into(),
        format: "raw-test-input".into(),
        platform: "linux".into(),
        input: "bin/sample.input".into(),
        parts: vec!["bin/sample.input".into()],
        tree: "sample.tree.json".into(),
        launch_target: "Game".into(),
        host: generate::hosts("linux"),
        windows: None,
    };
    fs::write(
        fixtures.join("expected/sample.toml"),
        toml::to_string(&expected).unwrap(),
    )
    .unwrap();
    let tree = vec![TreeEntry {
        path: "Game".into(),
        kind: EntryKind::File,
        size: 7,
        sha256: Some(fixtures::sha256_file(&fixtures.join("bin/sample.input")).unwrap()),
        executable: true,
        symlink_target: None,
    }];
    fs::write(
        fixtures.join("expected/sample.tree.json"),
        serde_json::to_vec(&tree).unwrap(),
    )
    .unwrap();
    checks::write_lock(root).unwrap();
    temp
}

fn cli(root: &Path, command: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["fixtures", command, "--root"])
        .arg(root)
        .output()
        .unwrap()
}

#[test]
fn clean_corpus_passes_without_any_builder_on_path() {
    let temp = sample_workspace();
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["fixtures", "check", "--root"])
        .arg(temp.path())
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn changed_layout_game_generator_and_builder_version_fail_staleness_command() {
    for file in [
        format!("{FIXTURE_ROOT}/src/layout.txt"),
        format!("{GAME_ROOT}/src/main.rs"),
        "xtask/src/main.rs".to_owned(),
        format!("{FIXTURE_ROOT}/src/builders.toml"),
    ] {
        let temp = sample_workspace();
        let new_source = if file.ends_with("builders.toml") {
            "rustc = \"changed-version\"\n"
        } else {
            "changed source\n"
        };
        fs::write(temp.path().join(&file), new_source).unwrap();
        let output = cli(temp.path(), "check-stale");
        assert!(
            !output.status.success(),
            "staleness check accepted modified {file}"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("fixtures are stale"));
    }
}

#[test]
fn missing_or_modified_fixture_and_expected_tree_are_hard_failures() {
    for file in [
        "bin/sample.input",
        "expected/sample.tree.json",
        "expected/sample.toml",
    ] {
        for remove in [true, false] {
            let temp = sample_workspace();
            let path = temp.path().join(FIXTURE_ROOT).join(file);
            if remove {
                fs::remove_file(path).unwrap();
            } else {
                fs::write(path, b"changed").unwrap();
            }
            let output = cli(temp.path(), "check-stale");
            assert!(!output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("outputs missing, changed, or unrecorded")
            );
        }
    }
}

#[test]
fn oversized_fixture_fails_the_real_cli_limit() {
    let temp = sample_workspace();
    let path = temp.path().join(FIXTURE_ROOT).join("bin/too-large.input");
    fs::File::create(path)
        .unwrap()
        .set_len(checks::MAX_FIXTURE_BYTES + 1)
        .unwrap();
    let output = cli(temp.path(), "check-size");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("limit is 3000000"));
}

#[test]
fn oversized_total_fails_the_real_cli_limit() {
    let temp = sample_workspace();
    // Source and expectation files count toward the aggregate budget too.
    let path = temp
        .path()
        .join(FIXTURE_ROOT)
        .join("src/sparse-large-source");
    fs::File::create(path)
        .unwrap()
        .set_len(checks::MAX_TOTAL_BYTES + 1)
        .unwrap();
    let output = cli(temp.path(), "check-size");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("limit is 80000000"));
}

#[test]
fn size_budget_includes_the_boundary_and_handmade_files() {
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join("handmade")).unwrap();
    fs::write(temp.path().join("handmade/test"), b"123").unwrap();
    fs::write(temp.path().join("metadata"), b"1234567").unwrap();
    checks::check_size_with_limits(temp.path(), 3, 10).unwrap();
    assert!(checks::check_size_with_limits(temp.path(), 2, 10).is_err());
    assert!(checks::check_size_with_limits(temp.path(), 3, 9).is_err());
}

#[test]
fn loader_refuses_missing_parts_and_unsafe_manifest_paths() {
    let temp = sample_workspace();
    let root = temp.path().join(FIXTURE_ROOT);
    let fixture = fixtures::Fixture::load(&root, "sample").unwrap();
    assert_eq!(fixture.input(), root.join("bin/sample.input"));
    assert_eq!(fixture.parts(), vec![fixture.input()]);
    fs::remove_file(fixture.input()).unwrap();
    assert!(
        fixtures::Fixture::load(&root, "sample")
            .unwrap_err()
            .to_string()
            .contains("missing committed fixture")
    );
    for path in [
        "../escape",
        "C:/escape",
        "C:\\escape",
        "/absolute",
        "\\\\host\\share",
        "a//b",
        "a/./b",
        "a/../b",
        "a\0b",
    ] {
        assert!(fixtures::safe_relative(path).is_err(), "accepted {path:?}");
    }
    assert!(fixtures::safe_relative("data/hello ü.txt").is_ok());
}

#[test]
fn committed_corpus_has_all_layouts_and_archive_families() {
    let root = xtask::workspace_root();
    checks::check_stale(&root).unwrap();
    checks::check_size(&root).unwrap();
    let corpus = generate::load_corpus(&root).unwrap();
    let loaded = fixtures::load_all(&root.join(FIXTURE_ROOT)).unwrap();
    assert_eq!(loaded.len(), corpus.fixtures.len() + corpus.handmade.len());
    for id in [
        "unity-zip",
        "unity-7z",
        "unity-7z-multivolume",
        "unreal-zip",
        "gog-zip",
        "linux-tar",
        "humble-linux-tar",
        "linux-tar-bz2",
        "linux-tar-xz",
        "linux-tar-zst",
        "macos-app-zip",
        "macos-app-tar-gz",
        "unity-rar5",
        "unreal-rar5",
        "rar-multivolume-portable",
    ] {
        let fixture = loaded
            .iter()
            .find(|fixture| fixture.expected.id == id)
            .unwrap();
        for host in ["windows", "linux", "macos"] {
            assert!(fixture.expected.host.contains_key(host));
        }
        if id.contains("multivolume") {
            assert!(fixture.parts().len() >= 2);
        }
        if id.starts_with("macos") {
            assert_eq!(
                fixture
                    .tree
                    .iter()
                    .filter(|entry| entry.kind == EntryKind::Symlink)
                    .count(),
                3
            );
        }
    }
}

#[test]
fn tree_comparison_checks_hash_mode_and_link_target() {
    let temp = sample_workspace();
    let fixtures_root = temp.path().join(FIXTURE_ROOT);
    let fixture = fixtures::Fixture::load(&fixtures_root, "sample").unwrap();
    let extracted = temp.path().join("extracted");
    fs::create_dir(&extracted).unwrap();
    let game = extracted.join("Game");
    fs::write(&game, b"payload").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&game, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fixture.assert_tree(&extracted).unwrap();
    fs::write(&game, b"changed").unwrap();
    assert!(fixture.assert_tree(&extracted).is_err());
    fs::write(&game, b"payload").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&game, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(fixture.assert_tree(&extracted).is_err());
        fs::set_permissions(&game, fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("../outside", extracted.join("unexpected-link")).unwrap();
        assert!(fixture.assert_tree(&extracted).is_err());
        let entries = fixtures::snapshot_tree(&extracted).unwrap();
        assert_eq!(
            entries
                .iter()
                .find(|e| e.path == "unexpected-link")
                .unwrap()
                .symlink_target
                .as_deref(),
            Some("../outside")
        );
    }
}

#[test]
fn sources_hash_the_same_with_crlf_checkout() {
    let temp = sample_workspace();
    let first = checks::source_hash(temp.path()).unwrap();
    fs::write(
        temp.path().join("xtask/src/main.rs"),
        "synthetic source\r\n",
    )
    .unwrap();
    assert_eq!(first, checks::source_hash(temp.path()).unwrap());
}

#[test]
fn check_flag_and_explicit_command_have_the_same_behavior() {
    let temp = sample_workspace();
    xtask::run(
        [
            "fixtures".to_owned(),
            "--root".to_owned(),
            temp.path().to_string_lossy().into_owned(),
            "--check".to_owned(),
        ]
        .into_iter(),
    )
    .unwrap();
}

#[test]
fn invalid_cli_is_an_error() {
    assert!(xtask::run(["other".to_owned()].into_iter()).is_err());
    assert!(xtask::run(["fixtures".to_owned(), "--root".to_owned()].into_iter()).is_err());
}

#[test]
fn paths_are_resolved_relative_to_fixture_root() {
    let temp = sample_workspace();
    let fixture = fixtures::Fixture::load(&temp.path().join(FIXTURE_ROOT), "sample").unwrap();
    assert_eq!(
        fixture.input(),
        PathBuf::from(temp.path())
            .join(FIXTURE_ROOT)
            .join("bin/sample.input")
    );
}
