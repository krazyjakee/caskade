//! The collection commands over the LibraryStore, as the signed-in account.
use super::*;
use caskade_core::model::{CollectionKind, Game};

#[tokio::test]
async fn collections_are_made_filled_and_listed() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = Game::new("Shelved");
    store.upsert_game(&game).await.unwrap();
    let accounts = AccountService::open(store.clone(), caskade_core::bus::Bus::default())
        .await
        .unwrap();
    let game_ids = [game.id.clone()];

    let made = save_collection(
        &store,
        &accounts,
        &SaveCollection {
            id: None,
            name: "Couch".into(),
            description: "Co-op".into(),
            kind: CollectionKind::Manual,
        },
    )
    .await
    .unwrap();
    let id = made.id;
    add_to_collection(&store, &accounts, &id, &game_ids)
        .await
        .unwrap();
    assert_eq!(
        game_collections(&store, &accounts, &game.id).await.unwrap(),
        std::slice::from_ref(&id)
    );
    let stored = get_collection(&store, &accounts, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.game_ids, game_ids);
    assert_eq!(stored.description, "Co-op");
    let page = collection_games(&store, &accounts, &id, 0, 10)
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].title, "Shelved");

    let smart = save_collection(
        &store,
        &accounts,
        &SaveCollection {
            id: None,
            name: "Favourites".into(),
            description: String::new(),
            kind: CollectionKind::Smart {
                query: serde_json::from_value(serde_json::json!({"filter": "favorites"})).unwrap(),
            },
        },
    )
    .await
    .unwrap();
    set_collection_pinned(&store, &accounts, &smart.id, true)
        .await
        .unwrap();
    let list = list_collections(&store, &accounts).await.unwrap();
    assert_eq!(list[0].name, "Favourites");
    assert_eq!(list[0].games, 0);
    assert_eq!(list[1].games, 1);
    assert!(
        add_to_collection(&store, &accounts, &smart.id, &game_ids)
            .await
            .is_err(),
        "a smart collection chooses its own games"
    );

    remove_from_collection(&store, &accounts, &id, &game_ids)
        .await
        .unwrap();
    assert!(delete_collection(&store, &accounts, &id).await.unwrap());
    let list = list_collections(&store, &accounts).await.unwrap();
    assert_eq!(list.len(), 1);
}
