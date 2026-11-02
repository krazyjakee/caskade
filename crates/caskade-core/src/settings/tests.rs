use super::*;
use crate::db::LibraryStore;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

fn definition(version: u32) -> SectionDefinition {
    SectionDefinition::new("test.options", version, json!({"type":"object","properties":{"enabled":{"type":"boolean"}},"required":["enabled"],"additionalProperties":false}), json!({"enabled":true})).unwrap()
}

#[tokio::test]
async fn validation_persistence_reset_and_subscriber_round_trip_after_commit() {
    let store = LibraryStore::in_memory().await.unwrap();
    let published = Arc::new(Mutex::new(Vec::new()));
    let output = published.clone();
    let service = SettingsService::with_publisher(store.clone(), move |event| {
        output.lock().unwrap().push(event)
    });
    service.register(definition(1)).unwrap();
    let mut subscriber = service.subscribe();
    let defaults = service.get("test.options").await.unwrap();
    assert_eq!(defaults.revision, 0);
    assert_eq!(defaults.value, json!({"enabled":true}));
    assert!(matches!(
        service
            .set("test.options", json!({"enabled":"yes"}), Some(0))
            .await,
        Err(SettingsError::Validation { .. })
    ));
    assert!(
        store
            .get_setting(&service::key("test.options"))
            .await
            .unwrap()
            .is_none()
    );
    assert!(subscriber.try_recv().is_err());
    let changed = service
        .set("test.options", json!({"enabled":false}), Some(0))
        .await
        .unwrap();
    let event = subscriber.recv().await.unwrap();
    assert_eq!(event.revision, changed.revision);
    let persisted = store
        .get_setting(&service::key("test.options"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted["value"], changed.value);
    let restarted = SettingsService::new(store.clone());
    restarted.register(definition(1)).unwrap();
    assert_eq!(restarted.get("test.options").await.unwrap(), changed);
    assert!(matches!(
        restarted
            .set("test.options", json!({"enabled":true}), Some(0))
            .await,
        Err(SettingsError::Conflict)
    ));
    assert_eq!(
        service
            .reset("test.options", Some(changed.revision))
            .await
            .unwrap()
            .value,
        defaults.value
    );
    assert_eq!(subscriber.recv().await.unwrap().revision, 2);
    assert_eq!(published.lock().unwrap().len(), 2);
    store.close().await;
}

#[tokio::test]
async fn migration_chain_is_atomic_idempotent_and_failure_preserves_old_value() {
    let store = LibraryStore::in_memory().await.unwrap();
    let original = json!({"schemaVersion":1,"revision":7,"value":{"old":false}});
    store
        .set_setting(&service::key("test.options"), &original)
        .await
        .unwrap();
    let service = SettingsService::new(store.clone());
    service
        .register(
            definition(3)
                .migration(1, |v| Ok(json!({"intermediate":v["old"]})))
                .unwrap()
                .migration(2, |v| Ok(json!({"enabled":v["intermediate"]})))
                .unwrap(),
        )
        .unwrap();
    let mut subscriber = service.subscribe();
    let migrated = service.get("test.options").await.unwrap();
    assert_eq!(migrated.version, 3);
    assert_eq!(migrated.revision, 8);
    assert_eq!(migrated.value, json!({"enabled":false}));
    assert_eq!(subscriber.recv().await.unwrap().version, 3);
    assert_eq!(service.get("test.options").await.unwrap(), migrated);
    assert!(subscriber.try_recv().is_err());
    store
        .set_setting(&service::key("test.options"), &original)
        .await
        .unwrap();
    let missing = SettingsService::new(store.clone());
    missing
        .register(definition(3).migration(1, Ok).unwrap())
        .unwrap();
    assert!(matches!(
        missing.get("test.options").await,
        Err(SettingsError::Migration)
    ));
    assert_eq!(
        store
            .get_setting(&service::key("test.options"))
            .await
            .unwrap(),
        Some(original)
    );
    let future = json!({"schemaVersion":4,"revision":9,"value":{"enabled":false}});
    store
        .set_setting(&service::key("test.options"), &future)
        .await
        .unwrap();
    assert!(matches!(
        service.get("test.options").await,
        Err(SettingsError::Version)
    ));
    assert_eq!(
        store
            .get_setting(&service::key("test.options"))
            .await
            .unwrap(),
        Some(future)
    );
    store.close().await;
}

#[tokio::test]
async fn compare_and_set_prevents_lost_updates_across_independent_services() {
    let store = LibraryStore::in_memory().await.unwrap();
    let one = SettingsService::new(store.clone());
    let two = SettingsService::new(store.clone());
    one.register(definition(1)).unwrap();
    two.register(definition(1)).unwrap();
    let (a, b) = tokio::join!(
        one.set("test.options", json!({"enabled":false}), Some(0)),
        two.set("test.options", json!({"enabled":false}), Some(0))
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(a, Err(SettingsError::Conflict)) || matches!(b, Err(SettingsError::Conflict)));
    assert_eq!(one.get("test.options").await.unwrap().revision, 1);
    store.close().await;
}

#[tokio::test]
async fn registered_schemas_reject_unknown_values_limits_and_unsupported_sections() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = SettingsService::new(store.clone());
    service
        .register_core(&crate::AppPaths::under(std::path::Path::new("fixture")))
        .unwrap();
    assert_eq!(service.sections().unwrap().len(), 16);
    assert_eq!(
        service.get("core.appUpdates").await.unwrap().value,
        json!({"check": true})
    );
    service
        .set("core.appUpdates", json!({"check": false}), Some(0))
        .await
        .unwrap();
    let restarted = SettingsService::new(store.clone());
    restarted
        .register_core(&crate::AppPaths::under(std::path::Path::new("fixture")))
        .unwrap();
    assert_eq!(
        restarted.get("core.appUpdates").await.unwrap().value,
        json!({"check": false})
    );
    let mut appearance = service.get("core.appearance").await.unwrap().value;
    appearance["cardSize"] = json!(4096);
    assert!(matches!(
        service.set("core.appearance", appearance, None).await,
        Err(SettingsError::Validation { .. })
    ));
    assert!(matches!(
        service.get("not.registered").await,
        Err(SettingsError::Unknown(_))
    ));
    assert!(service.register(definition(1)).is_ok());
    assert!(service.register(definition(1)).is_err());
    assert!(SectionDefinition::new("invalid", 1, json!({}), json!({})).is_err());
    assert!(
        SectionDefinition::new(
            "bad.schema",
            1,
            json!({"$ref":"https://invalid.example/private"}),
            json!({})
        )
        .is_err()
    );
    assert!(matches!(
        service
            .set("test.options", Value::String("x".repeat(65537)), None)
            .await,
        Err(SettingsError::Limit)
    ));
    store.close().await;
}

#[tokio::test]
async fn committed_setting_reaches_the_shared_hook_bus() {
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = crate::bus::Bus::default();
    let mut events = bus.stream();
    let service = SettingsService::with_bus(store.clone(), bus);
    service.register(definition(1)).unwrap();
    service
        .set("test.options", json!({"enabled":false}), Some(0))
        .await
        .unwrap();
    let event = events.recv().await.unwrap();
    assert!(
        matches!(event, crate::bus::Event::SettingsChanged { namespace, version:1 } if namespace == "test.options")
    );
    assert_eq!(
        store
            .get_setting(&service::key("test.options"))
            .await
            .unwrap()
            .unwrap()["value"]["enabled"],
        false
    );
    store.close().await;
}

#[tokio::test]
async fn a_value_near_the_size_budget_can_be_read_with_its_version_envelope() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = SettingsService::new(store.clone());
    service.register(SectionDefinition::new("test.large", 1,
        json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false}),
        json!({"text":""})).unwrap()).unwrap();
    let snapshot = service
        .set("test.large", json!({"text":"x".repeat(65520)}), Some(0))
        .await
        .unwrap();
    assert_eq!(service.get("test.large").await.unwrap(), snapshot);
    assert!(matches!(
        service
            .set("test.large", json!({"text":"x".repeat(65537)}), Some(1))
            .await,
        Err(SettingsError::Limit)
    ));
    assert_eq!(service.get("test.large").await.unwrap(), snapshot);
    store.close().await;
}

/// A plugin's schema: two properties, the second dropped in "version 2".
fn plugin_section(version: u32, with_limit: bool) -> SectionDefinition {
    let mut properties = json!({"enabled": {"type": "boolean"}});
    let mut defaults = json!({"enabled": true});
    if with_limit {
        properties["limit"] = json!({"type": "integer", "minimum": 1});
        defaults["limit"] = json!(5);
    }
    SectionDefinition::new(
        "plugins.dev.caskade.test",
        version,
        json!({"type": "object", "properties": properties, "additionalProperties": false}),
        defaults,
    )
    .unwrap()
    .revalidating()
}

#[tokio::test]
async fn plugin_sections_revalidate_across_versions_and_can_be_replaced_or_removed() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = SettingsService::new(store.clone());
    let namespace = "plugins.dev.caskade.test";
    service.register(plugin_section(3, true)).unwrap();
    service
        .set(namespace, json!({"enabled": false, "limit": 9}), None)
        .await
        .unwrap();

    // An update without `limit`: the property that no longer fits is
    // dropped, the rest is kept, with no migration registered.
    service.replace(plugin_section(4, false)).unwrap();
    let updated = service.get(namespace).await.unwrap();
    assert_eq!(
        (updated.version, updated.value.clone()),
        (4, json!({"enabled": false}))
    );
    // A downgrade revalidates the same way instead of failing; the value
    // is still valid there, so it is kept as it is.
    service.replace(plugin_section(2, true)).unwrap();
    let older = service.get(namespace).await.unwrap();
    assert_eq!(
        (older.version, older.value.clone()),
        (2, json!({"enabled": false}))
    );

    // Removing without forgetting keeps the stored value for a re-register.
    assert!(service.remove(namespace, false).await.unwrap());
    assert!(matches!(
        service.get(namespace).await,
        Err(SettingsError::Unknown(_))
    ));
    service.register(plugin_section(2, true)).unwrap();
    assert_eq!(service.get(namespace).await.unwrap().value, older.value);
    // Uninstall forgets it.
    assert!(service.remove(namespace, true).await.unwrap());
    assert!(
        store
            .get_setting(&format!("settings.section.{namespace}"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn sections_without_revalidation_still_refuse_unknown_versions() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = SettingsService::new(store.clone());
    service.register(definition(2)).unwrap();
    service
        .set("test.options", json!({"enabled": false}), None)
        .await
        .unwrap();
    service.replace(definition(1)).unwrap();
    assert!(matches!(
        service.get("test.options").await,
        Err(SettingsError::Version)
    ));
}

#[tokio::test]
async fn appearance_accent_is_a_hex_colour_or_the_theme_default() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = SettingsService::new(store.clone());
    service
        .register_core(&crate::AppPaths::under(std::path::Path::new("fixture")))
        .unwrap();
    let mut appearance = service.get("core.appearance").await.unwrap().value;
    assert_eq!(appearance.get("accent"), Some(&serde_json::Value::Null));
    appearance["accent"] = json!("#ff8800");
    let saved = service
        .set("core.appearance", appearance.clone(), None)
        .await
        .unwrap();
    assert_eq!(saved.value["accent"], "#ff8800");
    assert_eq!(
        service.get("core.appearance").await.unwrap().value["accent"],
        "#ff8800"
    );
    for bad in [json!("orange"), json!("#ff88"), json!("#gg8800")] {
        appearance["accent"] = bad;
        assert!(matches!(
            service
                .set("core.appearance", appearance.clone(), None)
                .await,
            Err(SettingsError::Validation { .. })
        ));
    }
    appearance["accent"] = serde_json::Value::Null;
    assert!(
        service
            .set("core.appearance", appearance, None)
            .await
            .is_ok()
    );
}
