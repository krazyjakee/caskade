//! D5 exit: empty DB -> real discovery -> expected library; delete -> rebuild.
//! Inputs are authored #70/#78/#90 machines, never the developer's live data.
#![cfg(feature = "sqlite")]
use caskade_core::{bus::Bus, db::LibraryStore, model::ListGames};
use caskade_discovery::{Cancel, Config, HostEnv, SourceId, SourceSelection, uri};
use caskade_sources::{DiscoveryService, ImportOptions};
use std::{collections::BTreeSet, fs, path::PathBuf};

async fn persisted(store: &LibraryStore) -> BTreeSet<BTreeSet<String>> {
    let page = store
        .list_games(&ListGames {
            limit: 1000,
            ..Default::default()
        })
        .await
        .unwrap();
    let mut groups = BTreeSet::new();
    for game in page.items {
        let entries = store.entries_for_game(&game.id).await.unwrap();
        groups.insert(
            entries
                .iter()
                .map(|e| format!("{}:{}", e.source_id, e.external_id))
                .collect(),
        );
    }
    groups
}
async fn rebuild(id: &str, sources: &[&str]) {
    let machine = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../caskade-discovery/tests/fixtures/machines")
        .join(id);
    let config = Config {
        sources: SourceSelection::only(sources.iter().map(|id| SourceId::from((*id).to_owned()))),
        ..Default::default()
    };
    let expected: serde_json::Value =
        serde_json::from_slice(&fs::read(machine.join("expected.json")).unwrap()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut before = None;
    for pass in 0..2 {
        let store = LibraryStore::open(&path).await.unwrap();
        assert_eq!(store.library_stats().await.unwrap().total, 0);
        let service = DiscoveryService::new(
            store.clone(),
            Bus::default(),
            HostEnv::fixture(&machine).unwrap(),
        );
        let result = service
            .scan(&config, &ImportOptions::default(), &Cancel::new())
            .await
            .unwrap();
        assert!(
            result.scan.sources.iter().all(|s| s.error.is_none()),
            "{:?}",
            result.scan.sources
        );
        assert!(
            result.import.warnings.is_empty(),
            "{:?}",
            result.import.warnings
        );
        assert!(
            result.import.review.is_empty(),
            "{:?}",
            result.import.review
        );
        let groups = persisted(&store).await;
        // Identity proxies (launcher links, uninstall aliases) fold into
        // their owner and are never stored as entries.
        let proxies: BTreeSet<_> = result
            .scan
            .groups
            .iter()
            .flat_map(|g| &g.entries)
            .filter(|e| uri::is_proxy(e))
            .map(|e| format!("{}:{}", e.source, e.key))
            .collect();
        let scanned: BTreeSet<BTreeSet<_>> = result
            .scan
            .groups
            .iter()
            .map(|g| {
                g.entries
                    .iter()
                    .map(|e| format!("{}:{}", e.source, e.key))
                    .filter(|key| !proxies.contains(key))
                    .collect()
            })
            .collect();
        assert_eq!(groups, scanned);
        if id == "linux-integration" {
            let inventory: BTreeSet<BTreeSet<String>> =
                serde_json::from_value::<Vec<Vec<String>>>(expected["groups"].clone())
                    .unwrap()
                    .into_iter()
                    .map(|g| g.into_iter().filter(|key| !proxies.contains(key)).collect())
                    .collect();
            assert_eq!(groups, inventory);
        } else {
            let titles: BTreeSet<_> = store
                .list_games(&ListGames {
                    limit: 1000,
                    ..Default::default()
                })
                .await
                .unwrap()
                .items
                .into_iter()
                .map(|g| g.title)
                .collect();
            let inventory: BTreeSet<String> =
                serde_json::from_value(expected["titles"].clone()).unwrap();
            assert_eq!(titles, inventory);
        }
        if let Some(before) = &before {
            assert_eq!(&groups, before);
        } else {
            before = Some(groups);
        }
        // A normal subsequent scan refreshes entries, never creates duplicates
        // or seeds playtime again.
        let repeated = service
            .scan(&config, &ImportOptions::default(), &Cancel::new())
            .await
            .unwrap();
        assert!(
            repeated
                .import
                .imported
                .iter()
                .all(|g| !g.created && !g.seeded_history)
        );
        assert_eq!(persisted(&store).await, before.clone().unwrap());
        drop(service);
        store.close().await;
        if pass == 0 {
            fs::remove_file(&path).unwrap();
            for suffix in ["-wal", "-shm"] {
                let _ = fs::remove_file(PathBuf::from(format!("{}{suffix}", path.display())));
            }
            assert!(!path.exists());
        }
    }
}
#[tokio::test]
async fn linux_machine_rebuilds_twenty_groups_after_database_deletion() {
    rebuild(
        "linux-integration",
        &[
            "steam",
            "steam.shortcuts",
            "heroic",
            "legendary",
            "lutris",
            "desktop",
            "flatpak",
            "snap",
        ],
    )
    .await;
}
#[tokio::test]
async fn windows_machine_rebuilds_expected_groups_using_wine_registry_fixtures() {
    rebuild(
        "windows-integration",
        &[
            "steam",
            "uninstall",
            "startmenu",
            "gameux",
            "epic",
            "gog",
            "ea",
            "ubisoft",
            "battlenet",
            "amazon",
            "riot",
            "rockstar",
            "xbox",
        ],
    )
    .await;
}
