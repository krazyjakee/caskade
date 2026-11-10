//! The backup settings section, which the core's registration fixture in
//! `caskade-core/tests/sync_policy.rs` can't register, has its
//! `sync::policy` classification.

use caskade_core::{AppPaths, db::LibraryStore, settings::SettingsService, sync::policy};
use std::path::Path;

#[tokio::test]
async fn every_registered_namespace_is_classified() {
    let settings = SettingsService::new(LibraryStore::in_memory().await.unwrap());
    settings
        .register_core(&AppPaths::under(Path::new("fixture")))
        .unwrap();
    caskade_backup::register_settings(&settings).unwrap();
    let missing: Vec<String> = settings
        .sections()
        .unwrap()
        .into_iter()
        .filter(|section| policy::setting(&section.namespace).is_none())
        .map(|section| section.namespace)
        .collect();
    assert!(missing.is_empty(), "unclassified namespaces: {missing:?}");
    assert!(policy::setting(caskade_backup::NAMESPACE).is_some());
}
