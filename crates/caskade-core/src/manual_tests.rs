//! Games added by hand (#174): title-only records, programs, shortcuts and
//! URIs referenced in place, duplicates, and removal that leaves files.
use super::*;
use crate::{
    bus::Bus,
    db::LibraryStore,
    install::{InstallOptions, InstallService},
    launch::LaunchService,
};
use caskade_discovery::SourceId;

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn program(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, b"#!/bin/sh\n").unwrap();
    path
}

#[tokio::test]
async fn a_title_only_record_has_nothing_to_start() {
    let store = LibraryStore::in_memory().await.unwrap();
    let id = store
        .add_manual_game(&ManualGame {
            title: "  Chrono Trigger (cartridge) ".into(),
            target: None,
            link_to: None,
            keep_apart: false,
        })
        .await
        .unwrap();
    let game = store.game(&id).await.unwrap().unwrap();
    assert_eq!(game.title, "Chrono Trigger (cartridge)");
    assert!(store.entries_for_game(&id).await.unwrap().is_empty());
    // An empty title isn't a record.
    assert!(
        store
            .add_manual_game(&ManualGame {
                title: " ".into(),
                target: None,
                link_to: None,
                keep_apart: false,
            })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_program_is_referenced_where_it_is() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("My Games/Outer Wilds");
    let exe = program(&folder, "OuterWilds.x86_64");
    let before = listing(&folder);
    let target = resolve_target(exe.to_str().unwrap()).unwrap();
    assert_eq!(target.title, "OuterWilds");
    assert_eq!(target.folder.as_deref(), Some(folder.as_path()));
    assert!(!target.windows);
    assert_eq!(
        target.launch,
        LaunchSpec::Executable {
            path: exe.clone(),
            args: vec![],
            cwd: None,
            env: Default::default(),
        }
    );
    let store = LibraryStore::in_memory().await.unwrap();
    let id = store
        .add_manual_game(&ManualGame {
            title: "Outer Wilds".into(),
            target: Some(target),
            link_to: None,
            keep_apart: false,
        })
        .await
        .unwrap();
    let [entry] = store
        .entries_for_game(&id)
        .await
        .unwrap()
        .try_into()
        .unwrap();
    assert_eq!(entry.source_id.as_str(), MANUAL_SOURCE);
    assert_eq!(entry.state, InstallState::Installed);
    assert_eq!(
        entry.location,
        Some(InstallLocation::External {
            path: folder.clone()
        })
    );
    // Nothing was written into the game's folder.
    assert_eq!(listing(&folder), before);
}

#[tokio::test]
async fn a_windows_program_runs_through_a_compatibility_tool() {
    let temp = tempfile::tempdir().unwrap();
    let exe = program(temp.path(), "Game.exe");
    let target = resolve_target(exe.to_str().unwrap()).unwrap();
    assert!(target.windows);
    let entry = entry_for("g", &target);
    assert!(matches!(entry.runtime, GameRuntime::Windows { .. }));
}

#[tokio::test]
async fn a_shortcut_keeps_its_arguments_folder_and_environment() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("Games/Celeste Dir");
    let exe = program(&folder, "Celeste");
    let shortcut = temp.path().join("celeste.desktop");
    std::fs::write(
        &shortcut,
        format!(
            "[Desktop Entry]\nType=Application\nName=Celeste\nExec=env SDL_VIDEODRIVER=x11 \"{}\" --windowed \"two words\"\nPath={}\n",
            exe.display(),
            folder.display()
        ),
    )
    .unwrap();
    let target = resolve_target(shortcut.to_str().unwrap()).unwrap();
    assert_eq!(target.title, "Celeste");
    let LaunchSpec::Executable {
        path,
        args,
        cwd,
        env,
    } = &target.launch
    else {
        panic!("{:?}", target.launch);
    };
    assert_eq!(path, &exe);
    assert_eq!(args, &["--windowed", "two words"]);
    assert_eq!(cwd.as_deref(), Some(folder.as_path()));
    assert_eq!(env.get("SDL_VIDEODRIVER").map(String::as_str), Some("x11"));
    assert_eq!(target.folder.as_deref(), Some(folder.as_path()));
}

#[test]
fn uris_folders_and_missing_files() {
    let target = resolve_target("steam://rungameid/620").unwrap();
    assert_eq!(
        target.launch,
        LaunchSpec::Uri {
            uri: "steam://rungameid/620".into()
        }
    );
    assert!(target.folder.is_none());
    let temp = tempfile::tempdir().unwrap();
    assert!(resolve_target(temp.path().to_str().unwrap()).is_err());
    assert!(resolve_target(temp.path().join("gone.exe").to_str().unwrap()).is_err());
    assert!(resolve_target("relative/game.sh").is_err());
    assert!(resolve_target("").is_err());
}

#[tokio::test]
async fn duplicates_are_found_and_linked_or_kept_apart() {
    let temp = tempfile::tempdir().unwrap();
    let exe = program(temp.path(), "portal2.sh");
    let store = LibraryStore::in_memory().await.unwrap();
    let mut portal = Game::new("Portal 2");
    portal.aliases = vec!["Portal Two".into()];
    let steam = GameEntry::new(&portal.id, SourceId::new("steam"), "620");
    let portal_id = store
        .import_group(&portal, &[steam], None)
        .await
        .unwrap()
        .game_id;
    let target = resolve_target(exe.to_str().unwrap()).unwrap();

    let found = store
        .manual_duplicates("PORTAL: two", Some(&target))
        .await
        .unwrap();
    assert_eq!(
        found,
        [DuplicateGame {
            game_id: portal_id.clone(),
            title: "Portal 2".into(),
            reason: DuplicateReason::SameTitle,
        }]
    );
    // Linking adds the target as a release of the existing game.
    let linked = store
        .add_manual_game(&ManualGame {
            title: "Portal 2".into(),
            target: Some(target.clone()),
            link_to: Some(portal_id.clone()),
            keep_apart: false,
        })
        .await
        .unwrap();
    assert_eq!(linked, portal_id);
    assert_eq!(store.entries_for_game(&portal_id).await.unwrap().len(), 2);
    // Now the same program is found by target.
    let found = store
        .manual_duplicates("Something else", Some(&target))
        .await
        .unwrap();
    assert_eq!(found[0].reason, DuplicateReason::SameTarget);

    // Kept separate, a new game IGDB's same-game merging leaves alone.
    let apart = store
        .add_manual_game(&ManualGame {
            title: "Portal 2".into(),
            target: None,
            link_to: None,
            keep_apart: true,
        })
        .await
        .unwrap();
    assert_ne!(apart, portal_id);
    let groups = store.same_games().await.unwrap();
    assert!(!groups.iter().flatten().any(|id| *id == apart));
}

#[tokio::test]
async fn a_launcher_finding_the_game_later_keeps_the_record_and_its_notes() {
    let temp = tempfile::tempdir().unwrap();
    let exe = program(temp.path(), "hades.sh");
    let store = LibraryStore::in_memory().await.unwrap();
    let id = store
        .add_manual_game(&ManualGame {
            title: "Hades".into(),
            target: Some(resolve_target(exe.to_str().unwrap()).unwrap()),
            link_to: None,
            keep_apart: false,
        })
        .await
        .unwrap();
    store
        .edit_game(
            &id,
            &GameEdit {
                notes: Some(NotesEdit {
                    value: "My notes".into(),
                }),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    // Steam's copy arrives as a game of its own; merged, both releases stay.
    let steam = Game::new("Hades");
    let entry = GameEntry::new(&steam.id, SourceId::new("steam"), "1145360");
    let steam_id = store
        .import_group(&steam, &[entry], None)
        .await
        .unwrap()
        .game_id;
    assert_ne!(steam_id, id);
    store.merge_games(&steam_id, &id).await.unwrap();
    let merged = store.game(&steam_id).await.unwrap().unwrap();
    assert_eq!(merged.notes, "My notes");
    let sources: Vec<_> = store
        .entries_for_game(&steam_id)
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.source_id.as_str().to_owned())
        .collect();
    assert!(sources.contains(&MANUAL_SOURCE.to_owned()));
}

#[tokio::test]
async fn removing_a_record_never_touches_the_games_files() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("Game");
    let exe = program(&folder, "game.sh");
    let store = LibraryStore::in_memory().await.unwrap();
    let service = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    let launch = LaunchService::new(store.clone()).with_aliases(None);
    let id = store
        .add_manual_game(&ManualGame {
            title: "Game".into(),
            target: Some(resolve_target(exe.to_str().unwrap()).unwrap()),
            link_to: None,
            keep_apart: false,
        })
        .await
        .unwrap();
    let title_only = store
        .add_manual_game(&ManualGame {
            title: "Cartridge".into(),
            target: None,
            link_to: None,
            keep_apart: false,
        })
        .await
        .unwrap();
    assert!(
        service
            .remove_game(&id, launch.clone())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        service
            .remove_game(&title_only, launch)
            .await
            .unwrap()
            .is_none()
    );
    assert!(store.game(&id).await.unwrap().is_none());
    assert!(store.game(&title_only).await.unwrap().is_none());
    assert_eq!(listing(&folder), ["game.sh"]);
}
