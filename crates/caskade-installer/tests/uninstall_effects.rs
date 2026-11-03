use caskade_installer::{
    exec::JournalEntry,
    manifest::{ExternalEffect, InstallManifest, effects_from_journal},
};
use std::{collections::BTreeMap, fs};

#[test]
fn executor_effects_become_portable_scoped_manifest_records() {
    let scope = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(scope.path()).unwrap();
    let journal = vec![
        JournalEntry::ExternalDirectory {
            path: base.join("Caskade"),
        },
        JournalEntry::ExternalFile {
            path: base.join("Caskade/game.lnk"),
            original: None,
        },
        JournalEntry::ExternalFile {
            path: base.join("settings.ini"),
            original: Some(b"old settings".to_vec()),
        },
    ];
    let roots = BTreeMap::from([("user.desktop".to_owned(), base.clone())]);
    let effects = effects_from_journal(&journal, &roots).unwrap();
    assert_eq!(effects.len(), 3);
    assert_eq!(effects[1].location().scope, "user.desktop");
    assert_eq!(effects[1].location().path.as_str(), "Caskade/game.lnk");
    assert!(
        matches!(&effects[2],ExternalEffect::File { original:Some(bytes),.. } if bytes==b"old settings")
    );
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.external_effects = effects;
    let manifest = InstallManifest::new(record).unwrap();
    let bytes = serde_json::to_vec(&manifest).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains(base.to_str().unwrap()));
    assert_eq!(manifest, InstallManifest::from_json(&bytes).unwrap());
}

#[test]
fn the_most_specific_external_scope_is_used() {
    let scope = tempfile::tempdir().unwrap();
    fs::create_dir(scope.path().join("desktop")).unwrap();
    let base = dunce::canonicalize(scope.path()).unwrap();
    let roots = BTreeMap::from([
        ("profile".into(), base.clone()),
        ("desktop".into(), base.join("desktop")),
    ]);
    let effects = effects_from_journal(
        &[JournalEntry::ExternalFile {
            path: base.join("desktop/game.lnk"),
            original: None,
        }],
        &roots,
    )
    .unwrap();
    assert_eq!(effects[0].location().scope, "desktop");
    assert_eq!(effects[0].location().path.as_str(), "game.lnk");
}

#[test]
fn unbound_absolute_traversing_and_scope_root_effects_are_not_recorded() {
    let scope = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(scope.path()).unwrap();
    let roots = BTreeMap::from([("desktop".into(), base.clone())]);
    for path in [
        base.parent().unwrap().join("outside"),
        base.clone(),
        base.join("../outside"),
        "relative".into(),
    ] {
        assert!(effects_from_journal(&[JournalEntry::ExternalDirectory { path }], &roots).is_err());
    }
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.external_effects = serde_json::from_value(
        serde_json::json!([{"type":"directory","location":{"scope":"../escape","path":"game"}}]),
    )
    .unwrap();
    assert!(InstallManifest::new(record).is_err());
}
