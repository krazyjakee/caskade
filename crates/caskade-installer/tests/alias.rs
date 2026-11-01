//! #31: alias create/retarget/remove through the real filesystem.
use caskade_installer::runtime::alias::{AliasError, AliasStore, supports_alias};
use std::fs;

#[test]
fn short_ids_are_stable_and_valid() {
    let id = AliasStore::short_id("root-1/Indie Game");
    assert_eq!(id, AliasStore::short_id("root-1/Indie Game"));
    assert_ne!(id, AliasStore::short_id("root-1/Other Game"));
    assert_eq!(id.len(), 12);
    let store = AliasStore::new("/tmp/never-used");
    assert!(store.path(&id).is_ok());
    for bad in ["", "..", "a/b", "A", "x".repeat(65).as_str()] {
        assert!(matches!(store.path(bad), Err(AliasError::InvalidId(_))));
    }
}

#[test]
fn alias_resolves_to_the_game_retargets_and_removal_keeps_the_target() {
    let home = tempfile::tempdir().unwrap();
    let store = AliasStore::new(home.path().join("Caskade/g"));
    let first = home.path().join("Games/Indie Game");
    let second = home.path().join("Other Drive/Indie Game");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    fs::write(first.join("game.cfg"), b"first").unwrap();
    fs::write(second.join("game.cfg"), b"second").unwrap();
    let id = AliasStore::short_id("root/Indie Game");

    assert_eq!(store.target(&id).unwrap(), None);
    let alias = store.link(&id, &first).unwrap();
    assert_eq!(alias, store.base().join(&id));
    assert_eq!(fs::read(alias.join("game.cfg")).unwrap(), b"first");
    // Writes through the alias land in the real directory.
    fs::write(alias.join("saved.ini"), b"through alias").unwrap();
    assert!(first.join("saved.ini").is_file());
    // Idempotent for the same target, retarget for a new one.
    assert_eq!(store.link(&id, &first).unwrap(), alias);
    store.link(&id, &second).unwrap();
    assert_eq!(
        store.target(&id).unwrap().as_deref(),
        Some(second.as_path())
    );
    assert_eq!(fs::read(alias.join("game.cfg")).unwrap(), b"second");

    store.remove(&id).unwrap();
    assert!(fs::symlink_metadata(&alias).is_err());
    assert!(first.join("saved.ini").is_file() && second.join("game.cfg").is_file());
    store.remove(&id).unwrap();
}

#[test]
fn real_directories_and_bad_targets_are_never_treated_as_aliases() {
    let home = tempfile::tempdir().unwrap();
    let store = AliasStore::new(home.path());
    let id = "abc123";
    fs::create_dir(home.path().join(id)).unwrap();
    fs::write(home.path().join(id).join("keep"), b"user data").unwrap();
    assert!(matches!(store.remove(id), Err(AliasError::NotALink(_))));
    assert!(matches!(store.target(id), Err(AliasError::NotALink(_))));
    assert!(home.path().join(id).join("keep").is_file());
    assert!(matches!(
        store.link("other", std::path::Path::new("relative/dir")),
        Err(AliasError::InvalidTarget(_))
    ));
    assert!(matches!(
        store.link("other", &home.path().join("missing")),
        Err(AliasError::InvalidTarget(_))
    ));
    assert!(supports_alias(home.path()));
    if cfg!(windows) {
        assert!(!supports_alias(std::path::Path::new(
            r"\\server\share\Games"
        )));
        assert!(!supports_alias(std::path::Path::new(
            r"\\?\UNC\server\share\Games"
        )));
    }
}

/// #270: an alias whose folder was renamed away (a same-volume move, an
/// outside rename) still reads, retargets, restores and removes. On Windows
/// this is a dangling junction, which follows-the-link checks call a plain
/// directory.
#[test]
fn dangling_aliases_read_retarget_and_remove_without_touching_folders() {
    let home = tempfile::tempdir().unwrap();
    let store = AliasStore::new(home.path().join("Caskade/g"));
    let before = home.path().join("Games/Indie Game");
    let after = home.path().join("Games/Indie Game (moved)");
    fs::create_dir_all(&before).unwrap();
    fs::write(before.join("game.cfg"), b"kept").unwrap();
    let id = AliasStore::short_id("root/Indie Game");
    let alias = store.link(&id, &before).unwrap();

    fs::rename(&before, &after).unwrap();
    assert!(!alias.exists(), "the alias dangles");
    assert_eq!(
        store.target(&id).unwrap().as_deref(),
        Some(before.as_path())
    );
    store.link(&id, &after).unwrap();
    assert_eq!(store.target(&id).unwrap().as_deref(), Some(after.as_path()));
    assert_eq!(fs::read(alias.join("game.cfg")).unwrap(), b"kept");

    // Rollback puts the dangling link back; removing it keeps the folder.
    store.restore(&id, &before).unwrap();
    assert_eq!(
        store.target(&id).unwrap().as_deref(),
        Some(before.as_path())
    );
    store.remove(&id).unwrap();
    assert!(fs::symlink_metadata(&alias).is_err());
    assert_eq!(store.target(&id).unwrap(), None);
    assert_eq!(fs::read(after.join("game.cfg")).unwrap(), b"kept");
}
