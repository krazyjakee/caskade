use super::*;
use caskade_core::model::Game;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

async fn services() -> (SettingsService, LibraryStore) {
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&caskade_core::AppPaths::under(std::path::Path::new(
            "fixture",
        )))
        .unwrap();
    (settings, store)
}

async fn set(settings: &SettingsService, namespace: &str, value: serde_json::Value) {
    let current = settings.get(namespace).await.unwrap().value;
    let mut merged = current.as_object().unwrap().clone();
    merged.extend(value.as_object().unwrap().clone());
    settings.set(namespace, merged.into(), None).await.unwrap();
}

async fn until(what: &str, count: &AtomicUsize, wanted: usize) {
    for _ in 0..200 {
        if count.load(Ordering::SeqCst) == wanted {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn a_new_library_goes_through_setup() {
    tauri::async_runtime::block_on(async {
        let (settings, store) = services().await;
        settle_existing(&settings, &store).await.unwrap();
        assert!(!completed(&settings).await);
        assert_eq!(settings.get(ONBOARDING).await.unwrap().revision, 0);
    });
}

#[test]
fn a_library_with_games_from_before_setup_skips_it() {
    tauri::async_runtime::block_on(async {
        let (settings, store) = services().await;
        store
            .upsert_game(&Game::new("Hollow Knight"))
            .await
            .unwrap();
        settle_existing(&settings, &store).await.unwrap();
        assert!(completed(&settings).await);
    });
}

#[test]
fn setup_asked_for_again_is_left_alone() {
    tauri::async_runtime::block_on(async {
        let (settings, store) = services().await;
        store
            .upsert_game(&Game::new("Hollow Knight"))
            .await
            .unwrap();
        // "Run setup again" in Settings saves `completed: false`.
        set(
            &settings,
            ONBOARDING,
            serde_json::json!({ "completed": false }),
        )
        .await;
        settle_existing(&settings, &store).await.unwrap();
        assert!(!completed(&settings).await);
    });
}

#[test]
fn online_lookups_wait_for_setup_and_follow_the_switch() {
    tauri::async_runtime::block_on(async {
        let (settings, _) = services().await;
        let started = Arc::new(AtomicUsize::new(0));
        let alive = Arc::new(AtomicUsize::new(0));
        supervise_online(settings.clone(), {
            let (started, alive) = (started.clone(), alive.clone());
            move || {
                started.fetch_add(1, Ordering::SeqCst);
                let alive = alive.clone();
                vec![tokio::spawn(async move {
                    struct Alive(Arc<AtomicUsize>);
                    impl Drop for Alive {
                        fn drop(&mut self) {
                            self.0.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                    alive.fetch_add(1, Ordering::SeqCst);
                    let _alive = Alive(alive);
                    std::future::pending::<()>().await;
                })]
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(started.load(Ordering::SeqCst), 0, "started before setup");

        set(
            &settings,
            ONBOARDING,
            serde_json::json!({ "completed": true }),
        )
        .await;
        until("the start", &alive, 1).await;

        set(&settings, METADATA, serde_json::json!({ "online": false })).await;
        until("the stop", &alive, 0).await;

        set(&settings, METADATA, serde_json::json!({ "online": true })).await;
        until("the restart", &alive, 1).await;
        assert_eq!(started.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn online_lookups_that_panic_start_again() {
    use caskade_core::settings::SectionDefinition;
    tauri::async_runtime::block_on(async {
        // Only the two sections the supervisor reads.
        let settings = SettingsService::new(LibraryStore::in_memory().await.unwrap());
        settings
            .register(SectionDefinition::typed::<OnboardingSettings>(ONBOARDING, 1).unwrap())
            .unwrap();
        settings
            .register(SectionDefinition::typed::<MetadataSettings>(METADATA, 1).unwrap())
            .unwrap();
        set(
            &settings,
            ONBOARDING,
            serde_json::json!({ "completed": true }),
        )
        .await;
        let started = Arc::new(AtomicUsize::new(0));
        let alive = Arc::new(AtomicUsize::new(0));
        supervise_online_every(
            settings.clone(),
            {
                let (started, alive) = (started.clone(), alive.clone());
                move || {
                    let first = started.fetch_add(1, Ordering::SeqCst) == 0;
                    let alive = alive.clone();
                    vec![tokio::spawn(async move {
                        assert!(!first, "the first run panics");
                        alive.fetch_add(1, Ordering::SeqCst);
                        std::future::pending::<()>().await;
                    })]
                }
            },
            Duration::from_millis(10),
        );
        until("the restart", &alive, 1).await;
        assert_eq!(started.load(Ordering::SeqCst), 2);
    });
}
