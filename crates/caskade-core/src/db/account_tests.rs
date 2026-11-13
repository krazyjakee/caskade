//! Accounts in the store (#202) and collections per account (#206).
use super::*;
use crate::{accounts::AccountRole, model::*};

const ACCOUNTS_MIGRATION: i64 = 14;

#[tokio::test]
async fn an_existing_library_gets_one_admin_owning_its_collections() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = Game::new("Kept");
    store.upsert_game(&game).await.unwrap();
    // The library as 0013 left it, with a collection.
    MIGRATOR
        .undo(&store.pool, ACCOUNTS_MIGRATION - 1)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collections(id,name,kind,description,pinned,updated_at_ms) VALUES('old','Old shelf','manual','From before',1,5)")
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collection_members(collection_id,game_id,position) VALUES('old',?,0)")
        .bind(&game.id)
        .execute(&store.pool)
        .await
        .unwrap();
    MIGRATOR.run(&store.pool).await.unwrap();

    let accounts = store.accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    let admin = &accounts[0];
    assert_eq!(admin.role, AccountRole::Admin);
    assert_eq!(admin.avatar, crate::accounts::DEFAULT_AVATAR);
    assert!(!admin.has_pin && admin.locked_until_ms.is_none());
    assert!(admin.created_at_ms > 0);
    let old = store
        .get_collection(&admin.id, "old")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (old.name.as_str(), old.pinned, old.game_ids.clone()),
        ("Old shelf", true, vec![game.id.clone()])
    );

    // And back: collections and their members survive the down migration.
    MIGRATOR
        .undo(&store.pool, ACCOUNTS_MIGRATION - 1)
        .await
        .unwrap();
    let members: i64 =
        sqlx::query_scalar("SELECT count(*) FROM collection_members WHERE collection_id='old'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(members, 1);
    MIGRATOR.run(&store.pool).await.unwrap();
    let admin = store.accounts().await.unwrap()[0].id.clone();
    assert!(store.get_collection(&admin, "old").await.unwrap().is_some());
}

/// A second account, written directly: the service's rules are tested in
/// `accounts::tests`.
async fn second_account(store: &LibraryStore) -> String {
    let id = new_id();
    sqlx::query("INSERT INTO accounts(id,name,avatar,role,permissions_json,created_at_ms,updated_at_ms) VALUES(?,'Second','avatar:default','admin','{}',1,1)")
        .bind(&id)
        .execute(&store.pool)
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn each_account_sees_only_its_own_collections() {
    let store = LibraryStore::in_memory().await.unwrap();
    let first = store.accounts().await.unwrap()[0].id.clone();
    let second = second_account(&store).await;
    let game = Game::new("Shared game");
    store.upsert_game(&game).await.unwrap();
    let save = |name: &str| SaveCollection {
        id: None,
        name: name.into(),
        description: String::new(),
        kind: CollectionKind::Manual,
    };
    // The same name in both.
    let mine = store.save_collection(&first, &save("Couch")).await.unwrap();
    let theirs = store
        .save_collection(&second, &save("Couch"))
        .await
        .unwrap();
    store
        .add_to_collection(&first, &mine.id, std::slice::from_ref(&game.id))
        .await
        .unwrap();
    store
        .add_to_collection(&second, &theirs.id, std::slice::from_ref(&game.id))
        .await
        .unwrap();

    let names = |list: Vec<CollectionSummary>| list.into_iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(
        names(store.collection_summaries(&first).await.unwrap()),
        std::slice::from_ref(&mine.id)
    );
    assert_eq!(
        names(store.collection_summaries(&second).await.unwrap()),
        std::slice::from_ref(&theirs.id)
    );
    assert_eq!(
        store.game_collection_ids(&first, &game.id).await.unwrap(),
        std::slice::from_ref(&mine.id)
    );

    // Another account's id is not found, and nothing of it changes.
    assert!(
        store
            .get_collection(&first, &theirs.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .collection_games(&first, &theirs.id, 0, 10)
            .await
            .is_err()
    );
    let rename = SaveCollection {
        id: Some(theirs.id.clone()),
        ..save("Taken")
    };
    assert!(store.save_collection(&first, &rename).await.is_err());
    assert!(
        store
            .set_collection_pinned(&first, &theirs.id, true)
            .await
            .is_err()
    );
    assert!(
        store
            .remove_from_collection(&first, &theirs.id, std::slice::from_ref(&game.id))
            .await
            .is_err()
    );
    let mut stolen = Collection::manual("Stolen");
    stolen.id = theirs.id.clone();
    assert!(store.upsert_collection(&first, &stolen).await.is_err());
    assert!(!store.delete_collection(&first, &theirs.id).await.unwrap());
    let kept = store
        .get_collection(&second, &theirs.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (kept.name.as_str(), kept.pinned, kept.game_ids),
        ("Couch", false, vec![game.id.clone()])
    );

    // Deleting an account takes its collections and their members along.
    assert!(store.delete_account(&second).await.unwrap());
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM collection_members WHERE collection_id=?")
            .bind(&theirs.id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(left, 0);
    assert_eq!(store.collection_summaries(&first).await.unwrap().len(), 1);
}

#[tokio::test]
async fn smart_collections_still_skip_hidden_games() {
    let store = LibraryStore::in_memory().await.unwrap();
    let account = store.accounts().await.unwrap()[0].id.clone();
    let shown = Game::new("Shown");
    let mut hidden = Game::new("Hidden");
    hidden.hidden = true;
    store.upsert_game(&shown).await.unwrap();
    store.upsert_game(&hidden).await.unwrap();
    let all = store
        .save_collection(
            &account,
            &SaveCollection {
                id: None,
                name: "All".into(),
                description: String::new(),
                kind: CollectionKind::Smart {
                    query: ListGames::default(),
                },
            },
        )
        .await
        .unwrap();
    let page = store
        .collection_games(&account, &all.id, 0, 10)
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].title, "Shown");
}
