//! The notification feed's commands: listed, paged and marked.
use super::*;
use caskade_achievements::notifications::{MemberState, NotificationCategory, NotificationTarget};
use caskade_core::{
    db::LibraryStore,
    model::{Game, GameEntry, SourceId},
};

#[tokio::test]
async fn the_feed_is_listed_paged_and_marked() {
    let store = LibraryStore::in_memory().await.unwrap();
    for (title, source) in [("Celeste", "steam"), ("Metroid", "romm:home")] {
        let game = Game::new(title);
        let entry = GameEntry::new(&game.id, SourceId::new(source), title);
        store.import_group(&game, &[entry], None).await.unwrap();
    }
    let notifications = Notifications::new(store.clone());
    assert!(notifications.ingest_additions().await.unwrap());

    let page = notifications_list(&notifications, false, 0, 20)
        .await
        .unwrap();
    assert_eq!((page.total, page.unread), (1, 1));
    let item = &page.items[0];
    assert_eq!(item.category, NotificationCategory::GamesAdded);
    assert_eq!(item.title, "2 games added");
    assert_eq!(item.target, NotificationTarget::Members);
    assert!(item.unread);
    let id = item.id.clone();

    let members = notification_members(&notifications, id.clone(), 0, 1)
        .await
        .unwrap();
    assert_eq!(members.total, 2);
    assert_eq!(members.items.len(), 1);
    assert_eq!(members.items[0].state, MemberState::Present);

    assert!(
        notification_mark_read(&notifications, id.clone(), item.version)
            .await
            .unwrap()
    );
    assert_eq!(notifications_unread(&notifications).await.unwrap(), 0);
    let unread = notifications_list(&notifications, true, 0, 20)
        .await
        .unwrap();
    assert_eq!(unread.total, 0);

    notification_mark_unread(&notifications, id).await.unwrap();
    assert_eq!(notifications_unread(&notifications).await.unwrap(), 1);
    assert_eq!(
        notifications_mark_all_read(&notifications, page.revision)
            .await
            .unwrap(),
        1
    );
    assert_eq!(notifications_unread(&notifications).await.unwrap(), 0);
    store.close().await;
}
