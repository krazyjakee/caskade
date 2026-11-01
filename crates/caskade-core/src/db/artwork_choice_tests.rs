//! Artwork the user chose (#175): kept through refreshes, imports
//! validated, Reset back to automatic precedence, cache bytes shared.
use super::LibraryStore;
use crate::{artwork::ArtworkCache, model::*};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

fn png(shade: u8) -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 8, Rgba([shade, 2, 3, 255])));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

struct Fixture {
    store: LibraryStore,
    cache: ArtworkCache,
    dir: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self {
            store: LibraryStore::in_memory().await.unwrap(),
            cache: ArtworkCache::new(dir.path().join("artwork")).unwrap(),
            dir,
        }
    }

    async fn game(&self, title: &str) -> String {
        self.store.upsert_game(&Game::new(title)).await.unwrap().id
    }

    /// An automatic image, as IGDB or RomM add one.
    async fn automatic(
        &self,
        game_id: &str,
        kind: ArtworkKind,
        source: &str,
        shade: u8,
    ) -> Artwork {
        let cached = self.cache.import_bytes(png(shade)).await.unwrap();
        self.store
            .upsert_artwork(&cached.record(game_id, kind, source))
            .await
            .unwrap()
    }

    async fn file(&self, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = self.dir.path().join(name);
        tokio::fs::write(&path, bytes).await.unwrap();
        path
    }

    async fn slot(&self, game_id: &str, kind: ArtworkKind) -> ArtworkSlot {
        self.store
            .artwork_slots(game_id)
            .await
            .unwrap()
            .unwrap()
            .into_iter()
            .find(|slot| slot.kind == kind)
            .unwrap()
    }

    /// The cover the library's cards and rows show, and the game page's.
    async fn covers(&self, game_id: &str) -> (Option<String>, Vec<String>) {
        let page = self.store.list_games(&ListGames::default()).await.unwrap();
        let card = page
            .items
            .into_iter()
            .find(|game| game.id == game_id)
            .unwrap()
            .cover
            .map(|art| art.content_hash);
        let details = self.store.get_game(game_id).await.unwrap().unwrap();
        let page = details
            .artwork
            .into_iter()
            .filter(|art| art.kind == ArtworkKind::Cover)
            .map(|art| art.content_hash)
            .collect();
        (card, page)
    }
}

#[tokio::test]
async fn a_chosen_image_stays_through_refreshes_and_reset_brings_automatic_back() {
    let f = Fixture::new().await;
    let game = f.game("Portal 2").await;
    let steam = f.automatic(&game, ArtworkKind::Cover, "steam", 1).await;
    let igdb = f.automatic(&game, ArtworkKind::Cover, "igdb:co1", 2).await;
    let slot = f.slot(&game, ArtworkKind::Cover).await;
    assert_eq!(slot.choice, ArtworkChoice::Automatic);
    assert_eq!(slot.shown.unwrap().id, steam.id);
    assert_eq!(slot.automatic.len(), 2);

    f.store
        .choose_artwork(&game, ArtworkKind::Cover, &igdb.id)
        .await
        .unwrap();
    // IGDB replaces its cover: the choice is a copy, so it stays.
    f.store.delete_artwork(&igdb.id).await.unwrap();
    let newer = f.automatic(&game, ArtworkKind::Cover, "igdb:co2", 3).await;
    let slot = f.slot(&game, ArtworkKind::Cover).await;
    assert_eq!(slot.choice, ArtworkChoice::Chosen);
    let shown = slot.shown.unwrap();
    assert_eq!(
        (shown.source.as_str(), shown.content_hash.as_str()),
        (USER_ARTWORK_SOURCE, igdb.content_hash.as_str())
    );
    assert_eq!(
        f.covers(&game).await,
        (
            Some(igdb.content_hash.clone()),
            vec![igdb.content_hash.clone()]
        )
    );
    // The user's copy isn't offered as an automatic image.
    assert!(
        slot.automatic
            .iter()
            .all(|art| art.source != USER_ARTWORK_SOURCE)
    );

    f.store
        .reset_artwork(&game, ArtworkKind::Cover)
        .await
        .unwrap();
    let slot = f.slot(&game, ArtworkKind::Cover).await;
    assert_eq!(slot.choice, ArtworkChoice::Automatic);
    assert_eq!(slot.shown.unwrap().id, steam.id);
    let (card, page) = f.covers(&game).await;
    assert_eq!(card, Some(steam.content_hash.clone()));
    assert_eq!(page, [steam.content_hash, newer.content_hash]);
}

#[tokio::test]
async fn an_imported_image_shows_and_a_bad_one_changes_nothing() {
    let f = Fixture::new().await;
    let game = f.game("Portal 2").await;
    f.automatic(&game, ArtworkKind::Hero, "steam", 1).await;
    let good = f.file("hero.png", &png(9)).await;
    let imported = f
        .store
        .import_user_artwork(&f.cache, &game, ArtworkKind::Hero, &good)
        .await
        .unwrap();
    // Thumbnails for every size the UI uses.
    assert_eq!(
        imported
            .thumbnails
            .iter()
            .map(|thumb| thumb.size)
            .collect::<Vec<_>>(),
        crate::artwork::THUMBNAIL_SIZES
    );
    let shown = || async { f.slot(&game, ArtworkKind::Hero).await.shown.unwrap().id };
    assert_eq!(shown().await, imported.id);

    let broken = f.file("broken.png", b"not an image").await;
    assert!(
        f.store
            .import_user_artwork(&f.cache, &game, ArtworkKind::Hero, &broken)
            .await
            .is_err()
    );
    let huge = f.dir.path().join("huge.png");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(crate::artwork::MAX_INPUT_BYTES as u64 + 1)
        .unwrap();
    assert!(
        f.store
            .import_user_artwork(&f.cache, &game, ArtworkKind::Hero, &huge)
            .await
            .is_err()
    );
    let gone = f.dir.path().join("gone.png");
    assert!(
        f.store
            .import_user_artwork(&f.cache, &game, ArtworkKind::Hero, &gone)
            .await
            .is_err()
    );
    assert_eq!(shown().await, imported.id);
    // Screenshots and logos aren't chosen here.
    assert!(
        f.store
            .import_user_artwork(&f.cache, &game, ArtworkKind::Screenshot, &good)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_removed_kind_shows_nothing_until_reset() {
    let f = Fixture::new().await;
    let game = f.game("Portal 2").await;
    let steam = f.automatic(&game, ArtworkKind::Cover, "steam", 1).await;
    f.store
        .remove_artwork(&game, ArtworkKind::Cover)
        .await
        .unwrap();
    // A refresh adds another automatic cover; none shows.
    f.automatic(&game, ArtworkKind::Cover, "igdb:co1", 2).await;
    assert_eq!(
        f.slot(&game, ArtworkKind::Cover).await.choice,
        ArtworkChoice::Removed
    );
    assert_eq!(f.covers(&game).await, (None, vec![]));
    f.store
        .reset_artwork(&game, ArtworkKind::Cover)
        .await
        .unwrap();
    assert_eq!(f.covers(&game).await.0, Some(steam.content_hash));
}

#[tokio::test]
async fn shared_images_stay_for_the_other_game() {
    let f = Fixture::new().await;
    let one = f.game("One").await;
    let two = f.game("Two").await;
    let path = f.file("cover.png", &png(5)).await;
    let first = f
        .store
        .import_user_artwork(&f.cache, &one, ArtworkKind::Cover, &path)
        .await
        .unwrap();
    let second = f
        .store
        .import_user_artwork(&f.cache, &two, ArtworkKind::Cover, &path)
        .await
        .unwrap();
    assert_eq!(first.content_hash, second.content_hash);
    f.store
        .reset_artwork(&one, ArtworkKind::Cover)
        .await
        .unwrap();
    assert_eq!(f.covers(&two).await.0, Some(second.content_hash.clone()));
    // The cache keeps the bytes and thumbnails.
    for thumb in &second.thumbnails {
        assert!(f.dir.path().join("artwork").join(&thumb.path).is_file());
    }
}

#[tokio::test]
async fn merges_and_separation_keep_the_choices() {
    let f = Fixture::new().await;
    let keep = f.game("Portal 2").await;
    let donor = f.game("Portal 2 (GOG)").await;
    let kept_cover = f.file("kept.png", &png(1)).await;
    let donor_cover = f.file("donor.png", &png(2)).await;
    let donor_icon = f.file("icon.png", &png(3)).await;
    let kept = f
        .store
        .import_user_artwork(&f.cache, &keep, ArtworkKind::Cover, &kept_cover)
        .await
        .unwrap();
    f.store
        .import_user_artwork(&f.cache, &donor, ArtworkKind::Cover, &donor_cover)
        .await
        .unwrap();
    let icon = f
        .store
        .import_user_artwork(&f.cache, &donor, ArtworkKind::Icon, &donor_icon)
        .await
        .unwrap();
    f.store.merge_games(&keep, &donor).await.unwrap();
    assert_eq!(
        f.slot(&keep, ArtworkKind::Cover)
            .await
            .shown
            .unwrap()
            .content_hash,
        kept.content_hash
    );
    let slot = f.slot(&keep, ArtworkKind::Icon).await;
    assert_eq!(
        (slot.choice, slot.shown.unwrap().content_hash),
        (ArtworkChoice::Chosen, icon.content_hash.clone())
    );
    assert_eq!(f.covers(&keep).await.1, std::slice::from_ref(&kept.content_hash));

    let first = GameEntry::new(&keep, SourceId::new("steam"), "620");
    let second = GameEntry::new(&keep, SourceId::new("gog"), "portal2");
    f.store.upsert_entry(&first).await.unwrap();
    let second = f.store.upsert_entry(&second).await.unwrap();
    let apart = f.store.separate_entry(&second.id, &[], &[]).await.unwrap();
    let slot = f.slot(&apart.id, ArtworkKind::Cover).await;
    assert_eq!(
        (slot.choice, slot.shown.unwrap().content_hash),
        (ArtworkChoice::Chosen, kept.content_hash)
    );
}

#[tokio::test]
async fn an_icon_the_user_chose_still_gets_an_automatic_one_beneath() {
    let f = Fixture::new().await;
    let game = f.game("Portal 2").await;
    let path = f.file("icon.png", &png(4)).await;
    f.store
        .import_user_artwork(&f.cache, &game, ArtworkKind::Icon, &path)
        .await
        .unwrap();
    assert_eq!(f.store.games_without_icon().await.unwrap(), [game]);
}
