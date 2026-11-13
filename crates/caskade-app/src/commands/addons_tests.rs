use super::*;
use caskade_addons::{ItemState, ReasonCode};

const CATALOGUE: &[u8] = include_bytes!("../../../caskade-addons/src/testdata/catalogue.json");
const SIGNATURE: &[u8] = include_bytes!("../../../caskade-addons/src/testdata/catalogue.json.sig");
const KEY: &str = include_str!("../../../caskade-addons/src/testdata/test-signing.key.pub");

#[tokio::test]
async fn the_catalogue_is_typed_and_offline() {
    let temp = tempfile::tempdir().unwrap();
    let service = CatalogueService::new(
        temp.path(),
        "https://example.org/addons/catalogue.json",
        Some(KEY),
    )
    .unwrap();
    service
        .accept(CATALOGUE, Some(SIGNATURE), 42)
        .await
        .unwrap();
    let view = addons_catalogue(Some(&service), None, None, AppMode::Console)
        .await
        .unwrap();
    assert_eq!(view.status.fetched_at_ms, Some(42));
    assert!(view.status.verified);
    assert_eq!(view.items.len(), 2);
    let stats = &view.items[0];
    assert_eq!(stats.entry.kind, AddonKind::Plugin);
    assert_eq!(stats.compatibility.state, ItemState::Available);
    assert_eq!(
        stats
            .compatibility
            .release
            .as_ref()
            .map(|release| release.version.as_str()),
        Some("0.1.0")
    );
    assert_eq!(stats.compatibility.reasons[0].code, ReasonCode::Mode);
    // Without an image cache, images just don't load.
    let media = addons_media(
        Some(&service),
        &AddonMedia(None),
        "dev.caskade.playtime-stats",
        0,
    )
    .await
    .unwrap();
    assert!(media.is_none());
}
