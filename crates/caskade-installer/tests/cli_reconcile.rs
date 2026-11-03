#![cfg(feature = "cli")]

use caskade_installer::{
    Input,
    manifest::{
        InstallManifest, InstalledFile, ManifestPath, RelocationVerdict, RootManifest, Sha256Digest,
    },
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn setup(home: &Path, verdict: RelocationVerdict) -> (PathBuf, PathBuf) {
    let root = home.join("Library Café");
    let game = root.join("Old Game");
    fs::create_dir_all(&game).unwrap();
    RootManifest::new("cli-reconcile-root")
        .unwrap()
        .write(&root)
        .unwrap();
    let data = format!("[game]\r\npath={}\r\n", game.display()).into_bytes();
    fs::write(game.join("config.ini"), &data).unwrap();
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.plan.root_id = "cli-reconcile-root".into();
    record.plan.relative_dir = "Old Game".into();
    record.launch_targets.clear();
    record.uninstall_steps.clear();
    record.external_effects.clear();
    record.alias = None;
    record.files = vec![InstalledFile {
        path: ManifestPath::new("config.ini").unwrap(),
        size: data.len() as u64,
        sha256: Some(Sha256Digest::new(format!("{:x}", Sha256::digest(&data))).unwrap()),
    }];
    record.relocation.last_path = Some(game.to_str().unwrap().into());
    record.relocation.verdict = verdict;
    record.relocation.references = vec![];
    InstallManifest::new(record).unwrap().write(&game).unwrap();
    (root, game)
}
fn cli(args: &[&str]) -> (std::process::ExitStatus, Value, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_caskade-install"))
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (
        output.status,
        value,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn binary_repairs_external_folder_and_root_renames_then_uninstalls_from_the_new_location() {
    let home = tempfile::tempdir().unwrap();
    let (root, old) = setup(home.path(), RelocationVerdict::Rewritable);
    let new = root.join("Renamed Game");
    fs::rename(&old, &new).unwrap();
    let (status, report, stderr) = cli(&["reconcile", root.to_str().unwrap()]);
    assert!(status.success(), "{report} {stderr}");
    assert_eq!(report["type"], "reconciled");
    assert_eq!(report["value"]["root_id"], "cli-reconcile-root");
    assert_eq!(report["value"]["games"][0]["status"], "reconciled");
    assert!(!old.exists());
    assert!(
        fs::read_to_string(new.join("config.ini"))
            .unwrap()
            .contains(dunce::canonicalize(&new).unwrap().to_str().unwrap())
    );
    let mount = home.path().join("Changed Mount");
    fs::rename(&root, &mount).unwrap();
    let current = mount.join("Renamed Game");
    let (status, report, stderr) = cli(&["reconcile", mount.to_str().unwrap()]);
    assert!(status.success(), "{report} {stderr}");
    assert_eq!(report["value"]["root_id"], "cli-reconcile-root");
    let manifest = InstallManifest::read(&current).unwrap();
    assert_eq!(
        manifest.record().relocation.last_path.as_deref(),
        dunce::canonicalize(&current).unwrap().to_str()
    );
    assert!(manifest.verify(&current).unwrap().changed.is_empty());
    let (status, report, stderr) = cli(&["uninstall", current.to_str().unwrap()]);
    assert!(status.success(), "{report} {stderr}");
    assert!(!current.exists());
    assert_eq!(
        fs::read_dir(&mount).unwrap().count(),
        1,
        "only root identity remains"
    );
}

#[test]
fn binary_can_repair_a_legacy_manifest_from_an_explicit_old_anchor() {
    let home = tempfile::tempdir().unwrap();
    let (root, old) = setup(home.path(), RelocationVerdict::Rewritable);
    let mut record = InstallManifest::read(&old).unwrap().into_record();
    record.relocation.last_path = None;
    InstallManifest::new(record).unwrap().write(&old).unwrap();
    let new = root.join("New");
    fs::rename(&old, &new).unwrap();
    let (status, report, _) = cli(&["reconcile", root.to_str().unwrap()]);
    assert_eq!(status.code(), Some(2));
    assert_eq!(report["value"]["games"][0]["status"], "needs_user");
    let (status, report, stderr) = cli(&[
        "reconcile",
        root.to_str().unwrap(),
        "--game",
        "New",
        "--from",
        old.to_str().unwrap(),
    ]);
    assert!(status.success(), "{report} {stderr}");
    assert_eq!(report["value"]["status"], "reconciled");
    assert!(
        fs::read_to_string(new.join("config.ini"))
            .unwrap()
            .contains(dunce::canonicalize(&new).unwrap().to_str().unwrap())
    );
}

#[test]
fn fragile_reconciliation_returns_a_typed_reinstall_question_and_exit_two() {
    let home = tempfile::tempdir().unwrap();
    let (root, old) = setup(home.path(), RelocationVerdict::Fragile);
    let mut record = InstallManifest::read(&old).unwrap().into_record();
    record.source.input = Input::Urls(vec![
        "https://example.test/kept-installer.exe".parse().unwrap(),
    ]);
    InstallManifest::new(record).unwrap().write(&old).unwrap();
    let new = root.join("New");
    fs::rename(old, &new).unwrap();
    let (status, report, _) = cli(&["reconcile", root.to_str().unwrap()]);
    assert_eq!(status.code(), Some(2));
    let game = &report["value"]["games"][0];
    assert_eq!(game["status"], "needs_user");
    assert_eq!(game["question"]["type"], "reinstall_after_move");
    assert_eq!(game["question"]["value"]["source"]["type"], "urls");
    assert!(
        InstallManifest::read(&new)
            .unwrap()
            .verify(&new)
            .unwrap()
            .changed
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn binary_refuses_to_repair_a_game_with_a_live_process_in_its_directory() {
    struct Kill(std::process::Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let home = tempfile::tempdir().unwrap();
    let (root, old) = setup(home.path(), RelocationVerdict::Rewritable);
    let new = root.join("New");
    fs::rename(&old, &new).unwrap();
    let before = fs::read(new.join("config.ini")).unwrap();
    let _process = Kill(
        Command::new("sleep")
            .arg("30")
            .current_dir(&new)
            .spawn()
            .unwrap(),
    );
    let (status, report, _) = cli(&["reconcile", root.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert_eq!(report["value"]["games"][0]["status"], "failed");
    assert!(
        report["value"]["games"][0]["error"]
            .as_str()
            .unwrap()
            .contains("running")
    );
    assert_eq!(fs::read(new.join("config.ini")).unwrap(), before);
}

#[test]
fn managed_move_validates_references_before_moving_and_requires_explicit_external_roots() {
    use caskade_installer::scan::{Encoding, Location, Reference, Via};
    for target in ["../outside.ini", ".caskade/install.json"] {
        let home = tempfile::tempdir().unwrap();
        let (_, game) = setup(home.path(), RelocationVerdict::Rewritable);
        let target_root = home.path().join("Destination");
        let before = fs::read(game.join("config.ini")).unwrap();
        let mut record = InstallManifest::read(&game).unwrap().into_record();
        record.relocation.references.push(Reference {
            location: Location::GameFile {
                path: target.into(),
                rewritable: true,
            },
            via: Via::Real,
            encoding: Encoding::Utf8,
        });
        InstallManifest::new(record).unwrap().write(&game).unwrap();
        let (status, report, _) = cli(&[
            "move",
            game.to_str().unwrap(),
            target_root.to_str().unwrap(),
        ]);
        assert_eq!(status.code(), Some(1), "{report}");
        assert_eq!(fs::read(game.join("config.ini")).unwrap(), before);
        assert!(!target_root.join("Old Game").exists());
    }
    let home = tempfile::tempdir().unwrap();
    let (_, game) = setup(home.path(), RelocationVerdict::Rewritable);
    let external = home.path().join("external");
    fs::create_dir(&external).unwrap();
    let config = external.join("game.ini");
    fs::write(&config, game.to_str().unwrap()).unwrap();
    let mut record = InstallManifest::read(&game).unwrap().into_record();
    record.relocation.references.push(Reference {
        location: Location::ExternalFile {
            path: config.to_str().unwrap().into(),
            rewritable: true,
        },
        via: Via::Real,
        encoding: Encoding::Utf8,
    });
    InstallManifest::new(record).unwrap().write(&game).unwrap();
    let target_root = home.path().join("Destination");
    let (status, report, _) = cli(&[
        "move",
        game.to_str().unwrap(),
        target_root.to_str().unwrap(),
    ]);
    assert_eq!(status.code(), Some(1), "{report}");
    assert!(game.exists());
    let (status, report, stderr) = cli(&[
        "move",
        game.to_str().unwrap(),
        target_root.to_str().unwrap(),
        "--external-root",
        external.to_str().unwrap(),
    ]);
    assert!(status.success(), "{report} {stderr}");
    let moved = target_root.join("Old Game");
    assert!(!game.exists());
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        moved.to_str().unwrap()
    );
    let manifest = InstallManifest::read(&moved).unwrap();
    assert_eq!(
        manifest.record().relocation.verdict,
        RelocationVerdict::Rewritable
    );
    assert!(manifest.verify(&moved).unwrap().changed.is_empty());
}

#[cfg(unix)]
#[test]
fn managed_move_rejects_reference_symlinks_before_touching_either_root() {
    let home = tempfile::tempdir().unwrap();
    let (_, game) = setup(home.path(), RelocationVerdict::Rewritable);
    let outside = home.path().join("outside.ini");
    fs::rename(game.join("config.ini"), &outside).unwrap();
    std::os::unix::fs::symlink(&outside, game.join("config.ini")).unwrap();
    let mut record = InstallManifest::read(&game).unwrap().into_record();
    record
        .relocation
        .references
        .push(caskade_installer::scan::Reference {
            location: caskade_installer::scan::Location::GameFile {
                path: "config.ini".into(),
                rewritable: true,
            },
            via: caskade_installer::scan::Via::Real,
            encoding: caskade_installer::scan::Encoding::Utf8,
        });
    InstallManifest::new(record).unwrap().write(&game).unwrap();
    let target = home.path().join("Destination");
    let before = fs::read(&outside).unwrap();
    let (status, report, _) = cli(&["move", game.to_str().unwrap(), target.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1), "{report}");
    assert_eq!(fs::read(&outside).unwrap(), before);
    assert!(game.exists());
    assert!(!target.join("Old Game").exists());
}

#[test]
fn managed_move_records_a_conservative_verdict_for_an_unscanned_native_install() {
    let home = tempfile::tempdir().unwrap();
    let (_, game) = setup(home.path(), RelocationVerdict::Unscanned);
    let mut record = InstallManifest::read(&game).unwrap().into_record();
    record.plan.candidate.strategy = caskade_installer::Strategy::RunSilent;
    InstallManifest::new(record).unwrap().write(&game).unwrap();
    let target = home.path().join("Destination");
    let (status, report, stderr) = cli(&["move", game.to_str().unwrap(), target.to_str().unwrap()]);
    assert!(status.success(), "{report} {stderr}");
    let manifest = InstallManifest::read(&target.join("Old Game")).unwrap();
    assert_eq!(
        manifest.record().relocation.verdict,
        RelocationVerdict::Fragile
    );
}

#[cfg(unix)]
#[test]
fn managed_move_does_not_steal_an_alias_from_another_existing_installation() {
    use caskade_installer::runtime::alias::AliasStore;
    let home = tempfile::tempdir().unwrap();
    let (_, game) = setup(home.path(), RelocationVerdict::Aliased);
    let other = home.path().join("Other Installation");
    fs::create_dir(&other).unwrap();
    let aliases = home.path().join("aliases");
    let store = AliasStore::new(aliases.clone());
    store.link("copy-conflict", &other).unwrap();
    let mut record = InstallManifest::read(&game).unwrap().into_record();
    record.alias = Some("copy-conflict".into());
    InstallManifest::new(record).unwrap().write(&game).unwrap();
    let target = home.path().join("Destination");
    let (status, report, _) = cli(&[
        "move",
        game.to_str().unwrap(),
        target.to_str().unwrap(),
        "--alias-dir",
        aliases.to_str().unwrap(),
    ]);
    assert_eq!(status.code(), Some(1), "{report}");
    assert!(game.exists());
    assert!(!target.join("Old Game").exists());
    assert_eq!(
        store.target("copy-conflict").unwrap().as_deref(),
        Some(other.as_path())
    );
}
