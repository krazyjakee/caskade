//! Epic achievements against a local GraphQL endpoint (#188), with
//! sanitized answers in the store's shapes.

use super::*;
use crate::achievements::{AccountAccess, Achievements, SetStatus};
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    model::{Game, GameEntry, SourceId},
    store_accounts::SyncState,
};
use std::sync::Arc;
use wiremock::{
    Mock, MockServer, Request, Respond, ResponseTemplate,
    matchers::{header, method, path},
};

fn credentials() -> StoreCredentials {
    StoreCredentials {
        access_token: "TOKEN".into(),
        refresh_token: Some("REFRESH".into()),
        expires_at_ms: None,
        extra: [("account_id".to_owned(), "acc-1".to_owned())].into(),
    }
}
fn account() -> StoreAccount {
    StoreAccount {
        id: "epic-account".into(),
        provider: "epic".into(),
        external_id: "acc-1".into(),
        display_name: "Player".into(),
        owner_id: None,
        enabled: true,
        sync: SyncState::default(),
        created_at_ms: 0,
    }
}
fn definition(name: &str, hidden: bool) -> serde_json::Value {
    json!({"achievement": {
        "name": name, "hidden": hidden,
        "unlockedDisplayName": format!("Got {name}"), "lockedDisplayName": if hidden { "Hidden".to_owned() } else { format!("Get {name}") },
        "unlockedDescription": format!("You did {name}"), "lockedDescription": "",
        "unlockedIconLink": format!("https://img.example/{name}.png"),
        "lockedIconLink": format!("https://img.example/{name}_l.png"),
        "XP": 10, "rarity": {"percent": 33.3}
    }})
}

/// Answers the definitions query by sandbox and the player query by
/// product, from a script.
struct Graph {
    definitions: std::collections::BTreeMap<String, serde_json::Value>,
    players: std::collections::BTreeMap<String, serde_json::Value>,
}
impl Respond for Graph {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let variables = &body["variables"];
        if let Some(sandbox) = variables["sandboxId"].as_str() {
            let record = self
                .definitions
                .get(sandbox)
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            return ResponseTemplate::new(200).set_body_json(json!({
                "data": {"Achievement": {"productAchievementsRecordBySandbox": record}}
            }));
        }
        let product = variables["productId"].as_str().unwrap_or_default();
        assert_eq!(variables["epicAccountId"], "acc-1");
        let answer = self
            .players
            .get(product)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        ResponseTemplate::new(200).set_body_json(json!({
            "data": {"PlayerProfile": {"playerProfile": {"productAchievements": answer}}}
        }))
    }
}
fn record(product: &str, achievements: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"productId": product, "sandboxId": "ignored", "achievements": achievements})
}
fn success(unlocked: &[(&str, &str)]) -> serde_json::Value {
    json!({"__typename": "PlayerProductAchievementsResponseSuccess", "data": {"playerAchievements":
        unlocked.iter().map(|(name, date)| json!({"playerAchievement": {
            "achievementName": name, "unlocked": true, "unlockDate": date
        }})).collect::<Vec<_>>()
    }})
}
async fn epic(graph: Graph) -> (MockServer, EpicAchievements) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(header("authorization", "Bearer TOKEN"))
        .respond_with(graph)
        .mount(&server)
        .await;
    let epic =
        EpicAchievements::with_endpoint(Url::parse(&format!("{}/graphql", server.uri())).unwrap());
    (server, epic)
}

#[tokio::test]
async fn each_namespace_has_its_own_definitions_and_the_account_s_unlocks() {
    let (_server, epic) = epic(Graph {
        definitions: [
            (
                "ns-a".to_owned(),
                record(
                    "prod-a",
                    vec![definition("WIN", false), definition("SECRET", true)],
                ),
            ),
            (
                "ns-b".to_owned(),
                record("prod-b", vec![definition("OTHER", false)]),
            ),
        ]
        .into(),
        players: [
            (
                "prod-a".to_owned(),
                success(&[("WIN", "2023-05-01T10:00:00.000Z")]),
            ),
            ("prod-b".to_owned(), success(&[])),
        ]
        .into(),
    })
    .await;
    let cancel = CancellationToken::new();
    let a = epic
        .fetch(&account(), Some(&credentials()), "ns-a", &cancel)
        .await
        .unwrap();
    assert!(a.complete);
    assert_eq!(a.definitions.len(), 2);
    assert_eq!(a.definitions[0].name, "Got WIN");
    assert_eq!(a.definitions[0].points, Some(10));
    assert_eq!(a.definitions[0].rarity_percent, Some(33.3));
    assert!(a.definitions[1].hidden);
    assert_eq!(a.states["WIN"].unlocked_at_ms, Some(1_682_935_200_000));
    assert!(!a.states.contains_key("SECRET"), "missing is locked");
    assert!(a.definitions.iter().all(|d| d.progress_max.is_none()));
    let b = epic
        .fetch(&account(), Some(&credentials()), "ns-b", &cancel)
        .await
        .unwrap();
    assert_eq!(b.definitions.len(), 1);
    assert!(b.states.is_empty());
    assert_eq!(
        epic.product(&ExternalId::Epic {
            namespace: "ns-a".into(),
            catalog_item: "item".into(),
            app_name: String::new(),
        }),
        Some("ns-a".into())
    );
}

#[tokio::test]
async fn unsupported_empty_private_and_expired_differ() {
    let (_server, epic) = epic(Graph {
        definitions: [
            ("empty".to_owned(), record("prod-e", vec![])),
            (
                "private".to_owned(),
                record("prod-p", vec![definition("WIN", false)]),
            ),
        ]
        .into(),
        players: [(
            "prod-p".to_owned(),
            json!({"__typename": "PlayerProductAchievementsResponseFailure"}),
        )]
        .into(),
    })
    .await;
    let cancel = CancellationToken::new();
    let creds = credentials();
    assert!(matches!(
        epic.fetch(&account(), Some(&creds), "none", &cancel).await,
        Err(FetchError::Unsupported(_))
    ));
    let empty = epic
        .fetch(&account(), Some(&creds), "empty", &cancel)
        .await
        .unwrap();
    assert!(empty.complete && empty.definitions.is_empty());
    assert!(matches!(
        epic.fetch(&account(), Some(&creds), "private", &cancel)
            .await,
        Err(FetchError::Provider(ProviderError::Unavailable(_)))
    ));
    let mut stale = credentials();
    stale.access_token = "OLD".into();
    // The mock only accepts the current token: Epic says 401.
    let refusing = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&refusing)
        .await;
    let expired = EpicAchievements::with_endpoint(
        Url::parse(&format!("{}/graphql", refusing.uri())).unwrap(),
    );
    assert!(matches!(
        expired
            .fetch(&account(), Some(&stale), "private", &cancel)
            .await,
        Err(FetchError::Provider(ProviderError::Expired(_)))
    ));
}

/// The store account, signed in.
struct Access(LibraryStore);
#[async_trait]
impl AccountAccess for Access {
    async fn accounts(&self) -> caskade_core::Result<Vec<StoreAccount>> {
        self.0.store_accounts().await
    }
    async fn credentials(
        &self,
        _: &StoreAccount,
    ) -> Result<Option<StoreCredentials>, ProviderError> {
        Ok(Some(credentials()))
    }
    fn provider_name(&self, _: &str) -> Option<String> {
        Some(EPIC.into())
    }
}

#[tokio::test]
async fn editions_in_two_namespaces_keep_two_sets_and_a_later_unlock_lands() {
    let store = LibraryStore::in_memory().await.unwrap();
    store.save_store_account(&account()).await.unwrap();
    let game = Game::new("Same Title");
    let entry = |namespace: &str| {
        let mut entry = GameEntry::new(&game.id, SourceId::new("epic"), namespace);
        entry.ids = vec![ExternalId::Epic {
            namespace: namespace.into(),
            catalog_item: format!("{namespace}-item"),
            app_name: String::new(),
        }];
        entry
    };
    let game = store
        .import_group(&game, &[entry("ns-a"), entry("ns-b")], None)
        .await
        .unwrap()
        .game_id;
    let (server, epic) = epic(Graph {
        definitions: [
            (
                "ns-a".to_owned(),
                record("prod-a", vec![definition("WIN", false)]),
            ),
            (
                "ns-b".to_owned(),
                record("prod-b", vec![definition("WIN", false)]),
            ),
        ]
        .into(),
        players: [
            ("prod-a".to_owned(), success(&[])),
            ("prod-b".to_owned(), success(&[])),
        ]
        .into(),
    })
    .await;
    let service = Achievements::new(
        store.clone(),
        Bus::default(),
        Arc::new(Access(store.clone())),
    );
    service.register_provider(Arc::new(epic));
    let cancel = CancellationToken::new();
    let sets = service.refresh_game(&game, None, &cancel).await.unwrap();
    assert_eq!(sets.len(), 2, "two namespaces, two sets");
    assert!(
        sets.iter()
            .all(|set| set.sync.status == SetStatus::Ready && set.unlocked == 0)
    );
    // The account unlocks WIN in namespace a only.
    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(Graph {
            definitions: [
                (
                    "ns-a".to_owned(),
                    record("prod-a", vec![definition("WIN", false)]),
                ),
                (
                    "ns-b".to_owned(),
                    record("prod-b", vec![definition("WIN", false)]),
                ),
            ]
            .into(),
            players: [
                (
                    "prod-a".to_owned(),
                    success(&[("WIN", "2024-01-01T00:00:00Z")]),
                ),
                ("prod-b".to_owned(), success(&[])),
            ]
            .into(),
        })
        .mount(&server)
        .await;
    let sets = service.refresh_game(&game, None, &cancel).await.unwrap();
    let unlocked: Vec<_> = sets
        .iter()
        .map(|set| (set.product_id.as_str(), set.unlocked))
        .collect();
    assert!(unlocked.contains(&("ns-a", 1)));
    assert!(unlocked.contains(&("ns-b", 0)));
}

/// Against Epic itself, with a launcher token (`CASKADE_EPIC_TOKEN`), the
/// account id (`CASKADE_EPIC_ACCOUNT`) and a sandbox (`CASKADE_EPIC_NAMESPACE`).
/// Read only.
#[tokio::test]
#[ignore = "needs an Epic sign-in: CASKADE_EPIC_TOKEN, CASKADE_EPIC_ACCOUNT, CASKADE_EPIC_NAMESPACE"]
async fn live() {
    let credentials = StoreCredentials {
        access_token: std::env::var("CASKADE_EPIC_TOKEN").expect("CASKADE_EPIC_TOKEN"),
        refresh_token: None,
        expires_at_ms: None,
        extra: [(
            "account_id".to_owned(),
            std::env::var("CASKADE_EPIC_ACCOUNT").expect("CASKADE_EPIC_ACCOUNT"),
        )]
        .into(),
    };
    let namespace = std::env::var("CASKADE_EPIC_NAMESPACE").expect("CASKADE_EPIC_NAMESPACE");
    let fetched = EpicAchievements::new()
        .fetch(
            &account(),
            Some(&credentials),
            &namespace,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    println!(
        "{} achievements, {} unlocked",
        fetched.definitions.len(),
        fetched.states.values().filter(|s| s.unlocked).count()
    );
}
