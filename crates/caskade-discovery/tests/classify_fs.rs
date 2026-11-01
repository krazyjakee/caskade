use caskade_discovery::{
    ClassifierConfig, HostEnv, LaunchTarget,
    classify::{self, Metadata, Verdict},
};
use caskade_installer::host::HostCaps;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
const ICON: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0,
    5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
fn host() -> HostCaps {
    toml::from_str(include_str!(
        "../../caskade-installer/tests/hosts/linux.toml"
    ))
    .unwrap()
}
fn fixture() -> (tempfile::TempDir, HostEnv, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("machine.toml"),
        "schema_version=1\nos=\"linux\"\n",
    )
    .unwrap();
    let root = temp.path().join("root/games/Original");
    fs::create_dir_all(root.join("Original_Data")).unwrap();
    fs::write(
        root.join("UnityPlayer.dll"),
        b"original synthetic engine marker",
    )
    .unwrap();
    fs::write(
        root.join("Original_Data/globalgamemanagers"),
        b"original data marker",
    )
    .unwrap();
    fs::write(
        root.join("start.sh"),
        b"#!/bin/sh\nprintf 'synthetic game\\n'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("start.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(root.join("icon.png"), ICON).unwrap();
    fs::write(root.join("Original.desktop"),b"[Desktop Entry]\nType=Application\nName=Original\nExec=start.sh\nIcon=icon.png\nCategories=Game;\n").unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    (temp, env, root)
}
fn contents(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(base: &Path, root: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_symlink() {
                continue;
            }
            if path.is_dir() {
                visit(base, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(base).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}
#[test]
fn folder_fingerprints_choose_existing_launch_without_writing_icon_or_metadata() {
    let (_temp, env, root) = fixture();
    let before = contents(&root);
    let classified = classify::classify_path(
        &env,
        &host(),
        Path::new("/games/Original"),
        &Metadata::default(),
        &ClassifierConfig::default(),
    )
    .unwrap();
    assert_eq!(classified.verdict, Verdict::Likely);
    match classified.launch.unwrap() {
        LaunchTarget::Exec { program, .. } => {
            assert_eq!(program, root.join("start.sh"));
            assert!(program.is_file());
        }
        other => panic!("unexpected target {other:?}"),
    }
    assert_eq!(contents(&root), before);
    assert!(!root.join(".caskade").exists());
}
#[test]
fn executable_input_and_entry_depth_budgets_are_supported() {
    let (_temp, env, root) = fixture();
    let result = classify::classify_path(
        &env,
        &host(),
        &root.join("start.sh"),
        &Metadata::default(),
        &ClassifierConfig::default(),
    )
    .unwrap();
    assert!(result.launch.is_some());
    let small = ClassifierConfig {
        max_entries: 2,
        ..Default::default()
    };
    assert!(classify::classify_path(&env, &host(), &root, &Metadata::default(), &small).is_err());
    let shallow = ClassifierConfig {
        max_depth: 0,
        ..Default::default()
    };
    let result =
        classify::classify_path(&env, &host(), &root, &Metadata::default(), &shallow).unwrap();
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("depth budget"))
    );
}
#[cfg(unix)]
#[test]
fn read_only_tree_and_external_symlinks_are_never_written_or_followed() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (temp, env, root) = fixture();
    let outside = temp.path().join("sentinel");
    fs::write(&outside, b"untouched").unwrap();
    symlink(&outside, root.join("external-link")).unwrap();
    for path in [root.clone(), root.join("Original_Data")] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o555)).unwrap();
    }
    let before = contents(&root);
    let result = classify::classify_path(
        &env,
        &host(),
        &root,
        &Metadata::default(),
        &ClassifierConfig::default(),
    )
    .unwrap();
    assert_eq!(result.verdict, Verdict::Likely);
    assert_eq!(contents(&root), before);
    assert_eq!(fs::read(outside).unwrap(), b"untouched");
    assert!(!root.join(".caskade").exists());
    for path in [root.clone(), root.join("Original_Data")] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
