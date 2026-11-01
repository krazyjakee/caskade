use super::*;
use crate::model::{Artwork, Game, ListGames};
use caskade_discovery::SourceId;
use caskade_installer::testing::pe_with_icon;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

fn png() -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(16, 16, Rgba([20, 80, 160, 255])));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

struct Fixture {
    store: LibraryStore,
    icons: ExeIcons,
    dir: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = LibraryStore::in_memory().await.unwrap();
        let artwork = ArtworkCache::new(dir.path().join("artwork")).unwrap();
        let icons = ExeIcons::new(store.clone(), Bus::default(), artwork);
        Self { store, icons, dir }
    }

    /// An installed game in `folder` (under the fixture's dir) that launches
    /// with `launch`.
    async fn add(&self, title: &str, folder: &str, launch: Option<LaunchSpec>) -> (Game, PathBuf) {
        let game = self.store.upsert_game(&Game::new(title)).await.unwrap();
        let dir = self.dir.path().join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), title);
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::External { path: dir.clone() });
        entry.launch = launch;
        self.store.upsert_entry(&entry).await.unwrap();
        (game, dir)
    }

    async fn icons(&self, game_id: &str) -> Vec<Artwork> {
        self.store
            .artwork_for_game(game_id)
            .await
            .unwrap()
            .into_iter()
            .filter(|art| art.kind == ArtworkKind::Icon)
            .collect()
    }
}

fn exe(path: &str) -> Option<LaunchSpec> {
    Some(LaunchSpec::Executable {
        path: path.into(),
        args: vec![],
        cwd: None,
        env: Default::default(),
    })
}

#[tokio::test]
async fn the_launch_executables_icon_becomes_the_games() {
    let fixture = Fixture::new().await;
    let (game, dir) = fixture
        .add("Windows Game", "windows", exe("bin/Game.exe"))
        .await;
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::write(dir.join("bin/Game.exe"), pe_with_icon(&png())).unwrap();

    assert_eq!(fixture.icons.run().await.unwrap(), 1);
    let icons = fixture.icons(&game.id).await;
    assert_eq!(icons.len(), 1);
    assert_eq!(icons[0].source, SOURCE);
    let page = fixture
        .store
        .list_games(&ListGames::default())
        .await
        .unwrap();
    assert_eq!(page.items[0].icon.as_ref(), Some(&icons[0]));
    // A game with an icon isn't looked at again.
    assert_eq!(fixture.icons.run().await.unwrap(), 0);
    assert_eq!(fixture.icons(&game.id).await.len(), 1);
}

#[tokio::test]
async fn a_game_is_read_again_only_when_its_executables_change() {
    let fixture = Fixture::new().await;
    let (game, dir) = fixture
        .add("Native Game", "native", exe("game.x86_64"))
        .await;
    std::fs::write(dir.join("game.x86_64"), b"\x7fELF\x02\x01\x01").unwrap();
    assert_eq!(fixture.icons.run().await.unwrap(), 0);
    let checked = fixture.store.icon_check(&game.id).await.unwrap();
    assert!(checked.is_some());
    assert_eq!(fixture.icons.run().await.unwrap(), 0);
    assert_eq!(fixture.store.icon_check(&game.id).await.unwrap(), checked);

    // An update that brings an icon is read.
    std::fs::write(dir.join("game.x86_64"), pe_with_icon(&png())).unwrap();
    assert_eq!(fixture.icons.run().await.unwrap(), 1);
}

#[tokio::test]
async fn a_store_launched_game_uses_its_folders_main_exe() {
    let fixture = Fixture::new().await;
    let uri = Some(LaunchSpec::Uri {
        uri: "steam://rungameid/620".into(),
    });
    let (game, dir) = fixture.add("Store Game", "store", uri).await;
    // The uninstaller is larger, but never the game.
    std::fs::write(dir.join("unins000.exe"), vec![0; 64 * 1024]).unwrap();
    std::fs::write(dir.join("Game.exe"), pe_with_icon(&png())).unwrap();

    assert_eq!(fixture.icons.run().await.unwrap(), 1);
    assert_eq!(fixture.icons(&game.id).await.len(), 1);
}

#[tokio::test]
async fn an_installs_saved_icon_comes_first_and_igdbs_is_left_alone() {
    let fixture = Fixture::new().await;
    let (saved, dir) = fixture.add("Installed", "installed", exe("Game.exe")).await;
    std::fs::create_dir_all(dir.join(".caskade")).unwrap();
    std::fs::write(dir.join(".caskade/icon.png"), png()).unwrap();
    // No readable executable: the saved icon is all there is.
    std::fs::write(dir.join("Game.exe"), b"MZ").unwrap();
    let (matched, dir) = fixture.add("Matched", "matched", exe("Game.exe")).await;
    std::fs::write(dir.join("Game.exe"), pe_with_icon(&png())).unwrap();
    let artwork = ArtworkCache::new(fixture.dir.path().join("artwork")).unwrap();
    let igdb = artwork.import_bytes(png()).await.unwrap();
    fixture
        .store
        .upsert_artwork(&igdb.record(&matched.id, ArtworkKind::Icon, "igdb:ic1"))
        .await
        .unwrap();

    assert_eq!(fixture.icons.run().await.unwrap(), 1);
    assert_eq!(fixture.icons(&saved.id).await[0].source, SOURCE);
    let matched = fixture.icons(&matched.id).await;
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].source, "igdb:ic1");
}
