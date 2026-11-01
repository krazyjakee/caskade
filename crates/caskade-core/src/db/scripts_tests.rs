//! Lifecycle scripts (#179) in the store: scopes, order, and what happens
//! to a game's scripts when the game merges or goes.
use super::LibraryStore;
use crate::{
    model::{Game, GameEntry, SourceId},
    scripts::{LifecycleScript, ScriptCommand, ScriptPhase, ScriptScope},
};

fn script(scope: ScriptScope, name: &str) -> LifecycleScript {
    let mut script = LifecycleScript::new(
        scope,
        ScriptPhase::BeforeLaunch,
        ScriptCommand::Direct {
            program: "/bin/true".into(),
            args: vec!["{InstallDir}".into()],
        },
    );
    script.name = name.into();
    script
}
fn names(scripts: &[LifecycleScript]) -> Vec<&str> {
    scripts.iter().map(|script| script.name.as_str()).collect()
}
fn game(id: &str) -> ScriptScope {
    ScriptScope::Game { game_id: id.into() }
}

#[tokio::test]
async fn each_scope_keeps_its_own_scripts_in_order() {
    let store = LibraryStore::in_memory().await.unwrap();
    let played = store.upsert_game(&Game::new("Played")).await.unwrap();
    let mount = store
        .save_lifecycle_script(&script(ScriptScope::Global, "Mount"))
        .await
        .unwrap();
    let vpn = store
        .save_lifecycle_script(&script(ScriptScope::Global, "VPN"))
        .await
        .unwrap();
    let mine = store
        .save_lifecycle_script(&script(game(&played.id), "Mods"))
        .await
        .unwrap();
    assert_eq!(
        names(&store.lifecycle_scripts(&ScriptScope::Global).await.unwrap()),
        ["Mount", "VPN"]
    );
    store
        .order_lifecycle_scripts(&ScriptScope::Global, &[vpn.id.clone(), mount.id.clone()])
        .await
        .unwrap();
    let set = store.scripts_for_launch(&played.id, None).await.unwrap();
    assert_eq!(names(&set.global), ["VPN", "Mount"]);
    assert_eq!(set.game, std::slice::from_ref(&mine));
    assert!(set.profile.is_empty());
    assert!(
        store
            .order_lifecycle_scripts(&ScriptScope::Global, std::slice::from_ref(&vpn.id))
            .await
            .is_err()
    );
    // Editing keeps its place; a script can't change scope.
    let mut renamed = mount.clone();
    renamed.name = "Mount ISO".into();
    store.save_lifecycle_script(&renamed).await.unwrap();
    assert_eq!(
        names(&store.lifecycle_scripts(&ScriptScope::Global).await.unwrap()),
        ["VPN", "Mount ISO"]
    );
    let mut moved = mine.clone();
    moved.scope = ScriptScope::Global;
    assert!(store.save_lifecycle_script(&moved).await.is_err());
    // A scope that doesn't exist is refused, with what was missing.
    assert!(
        store
            .save_lifecycle_script(&script(game("missing"), "Lost"))
            .await
            .is_err()
    );
    let Err(crate::Error::Invalid(why)) = store
        .save_lifecycle_script(&script(
            ScriptScope::Profile {
                profile_id: "nope".into(),
            },
            "Lost",
        ))
        .await
    else {
        panic!("a missing profile is refused");
    };
    assert!(why.contains("nope"), "{why}");
    assert!(store.delete_lifecycle_script(&vpn.id).await.unwrap());
    assert!(!store.delete_lifecycle_script(&vpn.id).await.unwrap());
}

#[tokio::test]
async fn a_game_s_scripts_follow_a_merge_and_go_with_the_game() {
    let store = LibraryStore::in_memory().await.unwrap();
    let first = Game::new("Same");
    let second = Game::new("Same");
    let a = GameEntry::new(&first.id, SourceId::new("fixture"), "a");
    let b = GameEntry::new(&second.id, SourceId::new("fixture"), "b");
    let a = store.import_group(&first, &[a], None).await.unwrap();
    let b = store.import_group(&second, &[b], None).await.unwrap();
    store
        .save_lifecycle_script(&script(game(&a.game_id), "Kept"))
        .await
        .unwrap();
    store
        .save_lifecycle_script(&script(game(&b.game_id), "Joined"))
        .await
        .unwrap();
    store.merge_games(&a.game_id, &b.game_id).await.unwrap();
    let merged = store.lifecycle_scripts(&game(&a.game_id)).await.unwrap();
    assert_eq!(names(&merged), ["Kept", "Joined"]);
    assert!(merged.iter().all(|script| script.scope == game(&a.game_id)));
    store.delete_game(&a.game_id).await.unwrap();
    assert!(
        store
            .lifecycle_scripts(&game(&a.game_id))
            .await
            .unwrap()
            .is_empty()
    );
}
