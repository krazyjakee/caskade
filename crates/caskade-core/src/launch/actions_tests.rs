use super::*;
use crate::model::{BuiltInAction, MANUAL_SOURCE};
use caskade_discovery::{InstallState, SourceId};
use std::{collections::BTreeMap, path::PathBuf};

fn release() -> GameEntry {
    let mut entry = GameEntry::new("game", SourceId::new("fixture"), "entry");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: "/games/fixture".into(),
    });
    entry.launch = Some(exe("/games/fixture/game"));
    entry
}
fn exe(path: &str) -> LaunchSpec {
    LaunchSpec::Executable {
        path: path.into(),
        args: vec![],
        cwd: None,
        env: BTreeMap::new(),
    }
}
fn action(id: &str, kind: ActionKind) -> GameAction {
    GameAction {
        id: id.into(),
        entry_id: release().id,
        name: id.into(),
        kind,
        target: exe("/games/fixture/tool"),
        tracking: ActionTracking::default(),
        proton: None,
    }
}
fn ids(actions: &[GameAction]) -> Vec<&str> {
    actions.iter().map(|action| action.id.as_str()).collect()
}

#[test]
fn a_release_s_own_launch_is_its_built_in_default_play_action() {
    let entry = release();
    let settings = EntryActionSettings::default();
    let all = entry_actions(&entry, vec![action("tool", ActionKind::Utility)], &settings);
    assert_eq!(ids(&all), [built_in_action_id(&entry.id).as_str(), "tool"]);
    assert_eq!(all[0].name, "Play");
    assert_eq!(all[0].target, entry.launch.clone().unwrap());
    let chosen = select(&entry, all, &settings, &LaunchChoice::default()).unwrap();
    assert_eq!(chosen.id, built_in_action_id(&entry.id));
    // Launching it changes nothing about the entry.
    assert_eq!(apply(&entry, &chosen), entry);
}

#[test]
fn the_built_in_action_keeps_the_user_s_name_and_tracking() {
    let entry = release();
    let tracking = ActionTracking {
        mode: TrackingMode::ProcessName,
        folder: None,
        names: vec!["game.bin".into()],
    };
    let settings = EntryActionSettings {
        built_in: BuiltInAction {
            name: Some("Play (Vulkan)".into()),
            tracking: Some(tracking.clone()),
            proton: None,
        },
        ..Default::default()
    };
    let built_in = built_in_action(&entry, &settings).unwrap();
    assert_eq!(built_in.name, "Play (Vulkan)");
    assert_eq!(built_in.tracking, tracking);
}

#[test]
fn the_default_is_the_chosen_play_action_or_the_built_in_one_when_it_is_gone() {
    let entry = release();
    let added = vec![
        action("tool", ActionKind::Utility),
        action("modded", ActionKind::Play),
    ];
    let mut settings = EntryActionSettings {
        default_action: Some("modded".into()),
        ..Default::default()
    };
    let all = entry_actions(&entry, added.clone(), &settings);
    let chosen = select(&entry, all, &settings, &LaunchChoice::default()).unwrap();
    assert_eq!(chosen.id, "modded");
    let launched = apply(&entry, &chosen);
    assert_eq!(launched.launch, Some(chosen.target));
    assert_eq!(launched.id, entry.id);

    settings.default_action = Some("deleted".into());
    let all = entry_actions(&entry, added, &settings);
    let chosen = select(&entry, all, &settings, &LaunchChoice::default()).unwrap();
    assert_eq!(chosen.id, built_in_action_id(&entry.id));
}

#[test]
fn ask_at_launch_lists_only_play_actions_and_marks_the_default() {
    let entry = release();
    let added = vec![
        action("tool", ActionKind::Utility),
        action("modded", ActionKind::Play),
    ];
    let settings = EntryActionSettings {
        default_action: Some("modded".into()),
        ask: true,
        ..Default::default()
    };
    let all = entry_actions(&entry, added.clone(), &settings);
    let Err(LaunchError::NeedsChoice(prompt)) =
        select(&entry, all, &settings, &LaunchChoice::default())
    else {
        panic!("expected a prompt");
    };
    assert_eq!(prompt.entry_id, entry.id);
    assert!(prompt.profiles.is_empty() && prompt.discs.is_empty());
    let listed: Vec<_> = prompt
        .actions
        .iter()
        .map(|option| (option.id.as_str(), option.default))
        .collect();
    assert_eq!(
        listed,
        [
            (built_in_action_id(&entry.id).as_str(), false),
            ("modded", true)
        ]
    );
    // The answer starts it, and a tool or a deleted action is refused.
    let all = entry_actions(&entry, added, &settings);
    let answer = |id: &str| LaunchChoice {
        action: Some(id.into()),
        ..Default::default()
    };
    assert_eq!(
        select(&entry, all.clone(), &settings, &answer("modded"))
            .unwrap()
            .id,
        "modded"
    );
    assert!(matches!(
        select(&entry, all.clone(), &settings, &answer("tool")),
        Err(LaunchError::Invalid(_))
    ));
    assert!(matches!(
        select(&entry, all, &settings, &answer("deleted")),
        Err(LaunchError::Invalid(_))
    ));
}

#[test]
fn one_play_action_never_asks_and_none_explains_itself() {
    let mut entry = release();
    let settings = EntryActionSettings {
        ask: true,
        ..Default::default()
    };
    let all = entry_actions(&entry, vec![], &settings);
    assert!(select(&entry, all, &settings, &LaunchChoice::default()).is_ok());
    entry.launch = None;
    entry.source_id = SourceId::new(MANUAL_SOURCE);
    let all = entry_actions(&entry, vec![action("tool", ActionKind::Utility)], &settings);
    let Err(LaunchError::Invalid(why)) = select(&entry, all, &settings, &LaunchChoice::default())
    else {
        panic!("expected an explanation");
    };
    assert!(why.contains("add a play action"), "{why}");
}

#[test]
fn a_native_tool_for_a_windows_game_runs_without_wine() {
    let mut entry = release();
    entry.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    let mut tool = action("script", ActionKind::Utility);
    tool.target = exe("/games/fixture/configure.sh");
    assert_eq!(apply(&entry, &tool).runtime, GameRuntime::Native);
    tool.target = exe("/games/fixture/ModOrganizer.exe");
    assert_eq!(apply(&entry, &tool).runtime, entry.runtime);
}

#[test]
fn an_action_s_proton_build_goes_to_the_runner_and_none_leaves_it_automatic() {
    use caskade_discovery::RunnerHint;
    let umu = |name: Option<&str>| GameRuntime::Windows {
        prefix: None,
        runner: Some(RunnerHint {
            kind: RunnerKind::Umu,
            name: name.map(Into::into),
            path: None,
        }),
    };
    let mut entry = release();
    entry.runtime = umu(None);
    let settings = EntryActionSettings {
        built_in: BuiltInAction {
            proton: Some("GE-Proton10-34".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    // The built-in action keeps its launch and takes its build.
    let built_in = built_in_action(&entry, &settings).unwrap();
    let launched = apply(&entry, &built_in);
    assert_eq!(launched.runtime, umu(Some("GE-Proton10-34")));
    assert_eq!(launched.launch, entry.launch);
    // An added Windows program takes its own; without one, the best
    // installed build runs.
    let mut modded = action("modded", ActionKind::Play);
    modded.target = exe("/games/fixture/game-modded.exe");
    assert_eq!(apply(&entry, &modded).runtime, umu(None));
    modded.proton = Some("Proton 10.0".into());
    assert_eq!(apply(&entry, &modded).runtime, umu(Some("Proton 10.0")));
    // A Wine prefix has no Proton build to choose.
    let wine = GameRuntime::Windows {
        prefix: None,
        runner: Some(RunnerHint {
            kind: RunnerKind::Wine,
            name: None,
            path: None,
        }),
    };
    entry.runtime = wine.clone();
    assert_eq!(apply(&entry, &modded).runtime, wine);
}

#[test]
fn tracking_that_cannot_work_names_what_to_choose_instead() {
    let entry = release();
    let mut play = action("play", ActionKind::Play);
    let check = |play: &GameAction| validate_tracking(&entry, play);
    play.tracking.mode = TrackingMode::ProcessName;
    let Err(LaunchError::Invalid(why)) = check(&play) else {
        panic!("names are needed");
    };
    assert!(why.contains("process names"), "{why}");
    play.tracking.names = vec!["game.exe".into()];
    assert!(check(&play).is_ok());

    play.target = LaunchSpec::Uri {
        uri: "steam://rungameid/1".into(),
    };
    play.tracking.mode = TrackingMode::OriginalProcess;
    assert!(check(&play).is_err());
    play.tracking.mode = TrackingMode::Folder;
    assert!(check(&play).is_ok());

    play.target = LaunchSpec::Flatpak {
        app_id: "org.example.Game".into(),
        args: vec![],
    };
    assert!(check(&play).is_err());
    play.target = LaunchSpec::Web {
        url: "https://example.com/".into(),
    };
    play.tracking.mode = TrackingMode::ProcessTree;
    assert!(check(&play).is_err());

    // A tool's tracking is never used, so it never fails.
    let mut tool = play.clone();
    tool.kind = ActionKind::Utility;
    assert!(check(&tool).is_ok());

    if !cfg!(windows) {
        let mut windows = entry.clone();
        windows.runtime = GameRuntime::Windows {
            prefix: None,
            runner: None,
        };
        play.target = exe("/games/fixture/game.exe");
        play.tracking.mode = TrackingMode::OriginalProcess;
        assert!(validate_tracking(&windows, &play).is_err());
        play.tracking.mode = TrackingMode::ProcessTree;
        assert!(validate_tracking(&windows, &play).is_ok());
    }
}

#[test]
fn targets_are_checked_before_they_are_saved() {
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("tool with spaces & $quotes'");
    std::fs::write(&program, "").unwrap();
    let mut entry = release();
    assert!(validate_target(&entry, &exe(program.to_str().unwrap())).is_ok());
    let Err(LaunchError::Invalid(why)) = validate_target(&entry, &exe("/no/such/tool")) else {
        panic!("a missing program fails");
    };
    assert!(why.contains("/no/such/tool"), "{why}");
    assert!(validate_target(&entry, &exe("")).is_err());
    // A relative program needs an install folder to be relative to.
    assert!(validate_target(&entry, &exe("bin/tool")).is_ok());
    entry.location = None;
    assert!(validate_target(&entry, &exe("bin/tool")).is_err());
    assert!(
        validate_target(
            &entry,
            &LaunchSpec::Uri {
                uri: "steam://rungameid/1".into()
            }
        )
        .is_ok()
    );
    assert!(
        validate_target(
            &entry,
            &LaunchSpec::Uri {
                uri: "ms-msdt:/id".into()
            }
        )
        .is_err()
    );
    assert!(
        validate_target(
            &entry,
            &LaunchSpec::Flatpak {
                app_id: "--command=sh".into(),
                args: vec![]
            }
        )
        .is_err()
    );
}

#[test]
fn tracking_follows_the_action_s_mode() {
    let command = crate::launch::LaunchCommand::executable("/games/fixture/mo2");
    let target = crate::tracking::TrackingTarget {
        real_dir: Some(PathBuf::from("/games/fixture")),
        alias_dir: Some(PathBuf::from("/aliases/fixture")),
        ..Default::default()
    };
    let launch = || PreparedLaunch::new(command.clone(), target.clone(), true);

    let mut names = launch();
    track(
        &mut names,
        &ActionTracking {
            mode: TrackingMode::ProcessName,
            folder: None,
            names: vec!["game.exe".into()],
        },
    );
    // The mod manager isn't the game: only the process it starts is.
    assert!(!names.direct);
    assert_eq!(names.target.mode, TrackingMode::ProcessName);
    assert_eq!(names.target.exe_names, ["game.exe"]);

    let mut folder = launch();
    track(
        &mut folder,
        &ActionTracking {
            mode: TrackingMode::Folder,
            folder: Some("bin".into()),
            names: vec![],
        },
    );
    assert!(!folder.direct);
    assert_eq!(
        folder.target.real_dir.as_deref(),
        Some(std::path::Path::new("/games/fixture/bin"))
    );
    assert_eq!(folder.target.alias_dir, None);

    let mut original = launch();
    original.direct = false;
    track(
        &mut original,
        &ActionTracking {
            mode: TrackingMode::OriginalProcess,
            ..Default::default()
        },
    );
    assert!(original.direct);

    let mut automatic = launch();
    track(&mut automatic, &ActionTracking::default());
    assert_eq!(automatic.target.mode, TrackingMode::Automatic);
    assert_eq!(automatic.target.alias_dir, target.alias_dir);
}

mod editing {
    use super::*;
    use crate::{db::LibraryStore, model::Game};

    async fn store_with(entry: Option<GameEntry>) -> (LibraryStore, Game) {
        let store = LibraryStore::in_memory().await.unwrap();
        let game = Game::new("Fixture");
        store.upsert_game(&game).await.unwrap();
        if let Some(mut entry) = entry {
            entry.game_id = game.id.clone();
            store.upsert_entry(&entry).await.unwrap();
        }
        (store, game)
    }
    fn new_action(kind: ActionKind, target: LaunchSpec) -> GameAction {
        GameAction {
            id: String::new(),
            entry_id: String::new(),
            name: "Mine".into(),
            kind,
            target,
            tracking: ActionTracking::default(),
            proton: None,
        }
    }

    #[tokio::test]
    async fn a_title_only_game_gets_a_manual_release_for_its_first_action() {
        let (store, game) = store_with(None).await;
        let uri = LaunchSpec::Uri {
            uri: "steam://rungameid/10".into(),
        };
        let saved = save_action(&store, &game.id, &new_action(ActionKind::Play, uri.clone()))
            .await
            .unwrap();
        let entries = store.entries_for_game(&game.id).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source_id.as_str(), MANUAL_SOURCE);
        assert_eq!(entries[0].launch, Some(uri.clone()));
        assert_eq!(saved.id, built_in_action_id(&entries[0].id));
        assert_eq!(saved.name, "Mine");
        // Its target is the user's to change.
        let mut changed = saved.clone();
        changed.target = LaunchSpec::Uri {
            uri: "steam://rungameid/20".into(),
        };
        save_action(&store, &game.id, &changed).await.unwrap();
        let releases = game_actions(&store, &game.id).await.unwrap();
        assert!(releases[0].editable_built_in);
        assert_eq!(releases[0].actions[0].target, changed.target);
        // The next action joins the same release.
        let tool = save_action(
            &store,
            &game.id,
            &new_action(ActionKind::Utility, uri.clone()),
        )
        .await
        .unwrap();
        assert_eq!(tool.entry_id, entries[0].id);
        assert!(!tool.id.is_empty());
        // A bad target is refused before anything is made.
        let (store, game) = store_with(None).await;
        let bad = new_action(
            ActionKind::Play,
            LaunchSpec::Uri {
                uri: "ms-msdt:/id".into(),
            },
        );
        assert!(save_action(&store, &game.id, &bad).await.is_err());
        assert!(store.entries_for_game(&game.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_found_release_s_own_target_stays_but_its_name_and_tracking_change() {
        let (store, game) = store_with(Some(release())).await;
        let releases = game_actions(&store, &game.id).await.unwrap();
        assert!(!releases[0].editable_built_in);
        let mut built_in = releases[0].actions[0].clone();
        built_in.name = "Play (DX11)".into();
        built_in.tracking = ActionTracking {
            mode: TrackingMode::ProcessName,
            folder: None,
            names: vec!["game.exe".into()],
        };
        built_in.proton = Some(" GE-Proton10-34 ".into());
        let saved = save_action(&store, &game.id, &built_in).await.unwrap();
        built_in.proton = Some("GE-Proton10-34".into());
        assert_eq!(saved, built_in);
        // A build is named, not given by path.
        let mut by_path = built_in.clone();
        by_path.proton = Some("/opt/proton".into());
        assert!(save_action(&store, &game.id, &by_path).await.is_err());
        // An external release's programs don't run through Caskade's Proton.
        assert!(!releases[0].proton);
        let mut retarget = built_in.clone();
        retarget.target = exe("/elsewhere");
        let Err(crate::Error::Invalid(why)) = save_action(&store, &game.id, &retarget).await else {
            panic!("a found release's launch stays its source's");
        };
        assert!(why.contains("duplicate"), "{why}");
        let mut as_tool = built_in.clone();
        as_tool.kind = ActionKind::Utility;
        assert!(save_action(&store, &game.id, &as_tool).await.is_err());
        assert!(delete_action(&store, &game.id, &built_in.id).await.is_err());
        // Another game's release isn't this game's to change.
        let (other, _) = store_with(None).await;
        let _ = other;
        let mut elsewhere = new_action(ActionKind::Play, exe("/bin/true"));
        elsewhere.entry_id = built_in.entry_id.clone();
        assert!(
            save_action(&store, "another-game", &elsewhere)
                .await
                .is_err()
        );
        let added = save_action(&store, &game.id, &elsewhere).await.unwrap();
        assert!(
            delete_action(&store, "another-game", &added.id)
                .await
                .is_err()
        );
        assert!(delete_action(&store, &game.id, &added.id).await.unwrap());
    }
}
