use super::*;
use caskade_core::model::LaunchSpec;

#[tokio::test]
async fn a_game_is_added_by_hand() {
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("game.sh");
    std::fs::write(&program, b"#!/bin/sh\n").unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let mut events = bus.stream();
    let input = |path: &std::path::Path| path.to_string_lossy().into_owned();

    assert!(manual_game_target(input(temp.path())).await.is_err());
    let target = manual_game_target(input(&program)).await.unwrap();
    assert_eq!(target.title, "game");
    assert!(matches!(target.launch, LaunchSpec::Executable { .. }));
    let none = manual_game_duplicates(&store, "game", Some(&target))
        .await
        .unwrap();
    assert!(none.is_empty());
    let id = manual_game_add(
        &store,
        &bus,
        ManualGame {
            title: "My Game".into(),
            target: Some(target),
            link_to: None,
            keep_apart: false,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        events.try_recv(),
        Ok(Event::LibraryGameAdded { game_id }) if game_id == id
    ));
    let found = manual_game_duplicates(&store, "my game", None)
        .await
        .unwrap();
    assert_eq!(found[0].game_id, id);
    store.close().await;
}
