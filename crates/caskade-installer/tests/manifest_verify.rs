use caskade_installer::{
    manifest::{FileChange, InstallManifest, ManifestError, RootManifest, verify},
    testing::TestRoot,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn fixture() -> InstallManifest {
    InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json")).unwrap()
}

fn populate(game: &Path, manifest: &InstallManifest) {
    for file in &manifest.record().files {
        let path = game.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes: &[u8] = if file.path.as_str().ends_with("config.ini") {
            b"config".as_slice()
        } else if file.path.as_str().ends_with(".txt") {
            b"ok"
        } else {
            b"game"
        };
        fs::write(path, bytes).unwrap();
    }
}

#[test]
fn manifest_and_root_files_write_read_and_replace_atomically() {
    let root = TestRoot::new().unwrap();
    let manifest = fixture();
    manifest.write(root.path()).unwrap();
    assert_eq!(InstallManifest::read(root.path()).unwrap(), manifest);
    let root_manifest = RootManifest::new("root-test-identity").unwrap();
    root_manifest.write(root.path()).unwrap();
    assert_eq!(RootManifest::read(root.path()).unwrap(), root_manifest);
    let mut record = manifest.into_record();
    record.title = "Updated title".into();
    let updated = InstallManifest::new(record).unwrap();
    updated.write(root.path()).unwrap();
    assert_eq!(InstallManifest::read(root.path()).unwrap(), updated);
    let mut names: Vec<_> = fs::read_dir(root.path().join(".caskade"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["install.json", "root.json"]);
    root.assert_sentinel_untouched();
}

#[test]
fn failed_atomic_replacement_keeps_the_existing_target_and_removes_temporary_files() {
    let root = TestRoot::new().unwrap();
    fs::create_dir_all(root.path().join(".caskade/install.json")).unwrap();
    fs::write(root.path().join(".caskade/install.json/keep"), b"keep").unwrap();
    assert!(fixture().write(root.path()).is_err());
    assert_eq!(
        fs::read(root.path().join(".caskade/install.json/keep")).unwrap(),
        b"keep"
    );
    let names: Vec<_> = fs::read_dir(root.path().join(".caskade"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["install.json"]);
    root.assert_sentinel_untouched();
}

#[test]
fn clean_install_and_user_added_files_verify_successfully() {
    let root = TestRoot::new().unwrap();
    let manifest = fixture();
    populate(root.path(), &manifest);
    manifest.write(root.path()).unwrap();
    fs::write(root.path().join("user-save.dat"), b"save").unwrap();
    let report = manifest.verify(root.path()).unwrap();
    assert!(report.is_ok());
    assert_eq!(report.checked_files, 3);
    assert!(report.missing.is_empty());
    assert!(report.changed.is_empty());
    assert_eq!(
        serde_json::from_value::<caskade_installer::manifest::VerifyReport>(
            serde_json::to_value(&report).unwrap()
        )
        .unwrap(),
        report
    );
    root.assert_sentinel_untouched();
}

#[test]
fn verify_reports_every_missing_size_changed_and_hash_changed_file() {
    let root = TestRoot::new().unwrap();
    let manifest = fixture();
    populate(root.path(), &manifest);
    fs::remove_file(root.path().join("data/config.ini")).unwrap();
    fs::write(root.path().join("bin/FixtureGame"), b"fake").unwrap();
    fs::write(root.path().join("data/café.txt"), b"long").unwrap();
    let report = verify(root.path(), &manifest).unwrap();
    assert!(!report.is_ok());
    assert_eq!(report.checked_files, 3);
    assert_eq!(
        report
            .missing
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>(),
        ["data/config.ini"]
    );
    assert_eq!(report.changed.len(), 2);
    assert_eq!(report.changed[0].path.as_str(), "bin/FixtureGame");
    assert!(matches!(
        report.changed[0].reason,
        FileChange::Sha256 { .. }
    ));
    assert_eq!(report.changed[1].path.as_str(), "data/café.txt");
    assert_eq!(
        report.changed[1].reason,
        FileChange::Size {
            expected: 2,
            actual: 4
        }
    );
    root.assert_sentinel_untouched();
}

#[test]
fn optional_hashes_are_size_only_and_a_directory_is_not_a_file() {
    let root = TestRoot::new().unwrap();
    let manifest = fixture();
    populate(root.path(), &manifest);
    fs::write(root.path().join("data/config.ini"), b"edited").unwrap();
    assert!(manifest.verify(root.path()).unwrap().is_ok());
    fs::remove_file(root.path().join("bin/FixtureGame")).unwrap();
    fs::create_dir(root.path().join("bin/FixtureGame")).unwrap();
    let report = manifest.verify(root.path()).unwrap();
    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.changed[0].reason, FileChange::NotRegularFile);
}

#[test]
fn missing_nested_directories_are_reported_without_aborting_the_scan() {
    let root = TestRoot::new().unwrap();
    let report = fixture().verify(root.path()).unwrap();
    assert_eq!(report.checked_files, 3);
    assert_eq!(report.missing.len(), 3);
    assert!(report.changed.is_empty());
    root.assert_clean();
}

#[test]
fn nonexistent_root_is_an_error_rather_than_a_successful_empty_report() {
    let root = TestRoot::new().unwrap();
    assert!(matches!(
        fixture().verify(&root.path().join("missing")),
        Err(ManifestError::Io(_))
    ));
    root.assert_clean();
}

#[test]
fn storage_identity_and_game_verification_survive_a_root_move() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("old root ü");
    let new = temp.path().join("new root ü");
    let game = PathBuf::from("Library/Fixture Game");
    fs::create_dir_all(old.join(&game)).unwrap();
    let identity = RootManifest::new("stable-root-identity").unwrap();
    identity.write(&old).unwrap();
    let manifest = fixture();
    populate(&old.join(&game), &manifest);
    manifest.write(&old.join(&game)).unwrap();
    let before = fs::read(old.join(&game).join(".caskade/install.json")).unwrap();
    assert!(!String::from_utf8_lossy(&before).contains(old.to_str().unwrap()));
    fs::rename(&old, &new).unwrap();
    assert_eq!(RootManifest::read(&new).unwrap(), identity);
    let reloaded = InstallManifest::read(&new.join(&game)).unwrap();
    assert_eq!(reloaded, manifest);
    assert!(reloaded.verify(&new.join(&game)).unwrap().is_ok());
    assert_eq!(
        before,
        fs::read(new.join(&game).join(".caskade/install.json")).unwrap()
    );
}

#[test]
fn migrations_read_without_rewriting_and_write_only_the_current_schema() {
    let root = TestRoot::new().unwrap();
    fs::create_dir(root.path().join(".caskade")).unwrap();
    let old = include_bytes!("manifests/install.v0.json");
    let path = root.path().join(".caskade/install.json");
    fs::write(&path, old).unwrap();
    let manifest = InstallManifest::read(root.path()).unwrap();
    assert_eq!(fs::read(&path).unwrap(), old);
    assert_eq!(manifest, fixture());
    manifest.write(root.path()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert!(value["handlers"].is_array());
    root.assert_sentinel_untouched();
}

#[test]
fn future_documents_are_left_byte_for_byte_untouched() {
    let root = TestRoot::new().unwrap();
    fs::create_dir(root.path().join(".caskade")).unwrap();
    for (name, bytes) in [
        (
            "install.json",
            b"{\"schema_version\":999,\"future-data\":true}".as_slice(),
        ),
        (
            "root.json",
            b"{\"schema_version\":999,\"root_id\":\"future-root\"}".as_slice(),
        ),
    ] {
        let path = root.path().join(".caskade").join(name);
        fs::write(&path, bytes).unwrap();
        if name == "install.json" {
            assert!(InstallManifest::read(root.path()).is_err());
        } else {
            assert!(RootManifest::read(root.path()).is_err());
        }
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    root.assert_sentinel_untouched();
}

#[cfg(unix)]
#[test]
fn leaf_symlinks_and_outside_parent_symlinks_cannot_spoof_verified_files() {
    let root = TestRoot::new().unwrap();
    let manifest = fixture();
    populate(root.path(), &manifest);
    let outside = root.parent_path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("FixtureGame"), b"game").unwrap();
    fs::remove_file(root.path().join("bin/FixtureGame")).unwrap();
    std::os::unix::fs::symlink(
        outside.join("FixtureGame"),
        root.path().join("bin/FixtureGame"),
    )
    .unwrap();
    let report = manifest.verify(root.path()).unwrap();
    assert_eq!(report.changed[0].reason, FileChange::NotRegularFile);
    fs::remove_file(root.path().join("bin/FixtureGame")).unwrap();
    fs::remove_dir(root.path().join("bin")).unwrap();
    std::os::unix::fs::symlink(&outside, root.path().join("bin")).unwrap();
    let report = manifest.verify(root.path()).unwrap();
    assert!(!report.is_ok());
    assert!(matches!(
        report.changed[0].reason,
        FileChange::Unreadable { .. }
    ));
    assert_eq!(fs::read(outside.join("FixtureGame")).unwrap(), b"game");
    root.assert_sentinel_untouched();
}

#[cfg(unix)]
#[test]
fn metadata_symlinks_cannot_redirect_manifest_reads_or_writes_outside_the_root() {
    let root = TestRoot::new().unwrap();
    let outside = root.parent_path().join("outside");
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.path().join(".caskade")).unwrap();
    assert!(fixture().write(root.path()).is_err());
    assert!(
        RootManifest::new("root")
            .unwrap()
            .write(root.path())
            .is_err()
    );
    assert!(InstallManifest::read(root.path()).is_err());
    assert!(RootManifest::read(root.path()).is_err());
    assert!(fs::read_dir(&outside).unwrap().next().is_none());
    root.assert_sentinel_untouched();
}

#[cfg(unix)]
#[test]
fn metadata_file_symlinks_are_refused_on_read_and_atomically_replaced_on_write() {
    let root = TestRoot::new().unwrap();
    fs::create_dir(root.path().join(".caskade")).unwrap();
    let outside = root.parent_path().join("outside.json");
    fs::write(&outside, b"unchanged").unwrap();
    let path = root.path().join(".caskade/install.json");
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(InstallManifest::read(root.path()).is_err());
    fixture().write(root.path()).unwrap();
    assert!(!path.is_symlink());
    assert_eq!(fs::read(&outside).unwrap(), b"unchanged");
    assert_eq!(InstallManifest::read(root.path()).unwrap(), fixture());
    root.assert_sentinel_untouched();
}

#[cfg(unix)]
#[test]
fn non_unicode_game_paths_are_rejected_before_serialization() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let mut record = fixture().into_record();
    record.launch_targets[0].path = OsString::from_vec(vec![b'b', 0xff]).into();
    assert!(InstallManifest::new(record).is_err());
}
