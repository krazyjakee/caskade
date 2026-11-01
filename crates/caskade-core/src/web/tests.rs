use super::{
    GameSize, WEB_SOURCE, WebGames,
    page::{self, Source},
};
use crate::{
    artwork::ArtworkCache,
    db::LibraryStore,
    model::{ArtworkKind, GameFilter, LaunchSpec, ListGames},
};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn png() -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255])));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[test]
fn titles_lose_the_site_after_them() {
    for (raw, title) in [
        (
            "Penalty Shooters 2 🕹️ Play on CrazyGames",
            "Penalty Shooters 2",
        ),
        (
            "Celeste Classic by Maddy | itch.io",
            "Celeste Classic by Maddy",
        ),
        (
            "Subway Surfers - Play Online for Free! | Poki",
            "Subway Surfers",
        ),
        ("  2048  ", "2048"),
        ("Who Is Lying?", "Who Is Lying?"),
        ("Tic Tac Toe — Free Game", "Tic Tac Toe"),
        ("| leading bar", "| leading bar"),
    ] {
        assert_eq!(page::clean_title(raw), title, "{raw}");
    }
}

#[test]
fn pages_give_their_own_picture_before_their_icons() {
    let base = Url::parse("https://games.example/play/needle/").unwrap();
    let info = page::parse(
        r#"<html><head>
        <title>Needle | Example Games</title>
        <meta property="og:title" content="Needle in a Haystack">
        <meta name="description" content="  Find the
           needle. ">
        <meta property="og:image" content="/art/needle.jpg">
        <link rel="icon" href="favicon.ico">
        <link rel="icon" sizes="32x32 192x192" href="/icon-192.png">
        <link rel="apple-touch-icon" href="/touch.png">
        <link rel="mask-icon" href="/mask.svg">
        <link rel="icon" sizes="512x512" href="/icon.svg">
        </head></html>"#,
        &base,
    );
    assert_eq!(info.title.as_deref(), Some("Needle in a Haystack"));
    assert_eq!(info.description.as_deref(), Some("Find the needle."));
    let images: Vec<_> = info
        .pictures
        .iter()
        .map(|picture| picture.url.as_str())
        .collect();
    assert_eq!(
        images,
        [
            "https://games.example/art/needle.jpg",
            "https://games.example/icon-192.png",
            "https://games.example/touch.png",
            "https://games.example/play/needle/favicon.ico",
            "https://games.example/favicon.ico",
        ]
    );
    let bare = page::parse("<title>Just a title</title>", &base);
    assert_eq!(bare.title.as_deref(), Some("Just a title"));
    assert!(bare.description.is_none());
    let favicon: Vec<_> = bare.pictures.iter().map(|p| p.url.as_str()).collect();
    assert_eq!(favicon, ["https://games.example/favicon.ico"], "unlinked");
}

#[test]
fn the_game_frame_is_itchs_placeholder_or_the_largest_iframe() {
    let base = Url::parse("https://maddy.itch.io/celeste").unwrap();
    let itch = page::parse(
        r#"<div class="iframe_placeholder" data-iframe="&lt;iframe frameborder=&quot;0&quot; src=&quot;https://html-classic.itch.zone/html/123/index.html&quot; width=&quot;640&quot; height=&quot;360&quot;&gt;&lt;/iframe&gt;"></div>
        <iframe src="https://ads.example/banner" width="300" height="250"></iframe>"#,
        &base,
    );
    assert_eq!(
        itch.frame.as_ref().map(Url::as_str),
        Some("https://html-classic.itch.zone/html/123/index.html")
    );
    assert_eq!(itch.frame_size, GameSize::new(640, 360));
    let portal = page::parse(
        r#"<iframe src="/ad" width="300" height="250"></iframe>
        <iframe src="https://games.example/g/needle/" width="960px" height="600px"></iframe>"#,
        &base,
    );
    assert_eq!(
        portal.frame.as_ref().map(Url::as_str),
        Some("https://games.example/g/needle/")
    );
    let only = page::parse(r#"<iframe src="game/index.html"></iframe>"#, &base);
    assert_eq!(
        only.frame.as_ref().map(Url::as_str),
        Some("https://maddy.itch.io/game/index.html")
    );
    let none = page::parse(
        r#"<iframe src="a"></iframe><iframe src="b"></iframe>"#,
        &base,
    );
    assert_eq!(none.frame, None);
    let small = page::parse(
        r#"<iframe src="a" width="10" height="10"></iframe><iframe src="b"></iframe>"#,
        &base,
    );
    assert_eq!(small.frame, None);
    // totaljerkface.com: a script adds the game's frame; the page's own is
    // a 200×150 teaser.
    let teaser = page::parse(
        r#"<iframe src="/portrait_teaser.html" width="200" height="150"></iframe>"#,
        &base,
    );
    assert_eq!(teaser.frame, None);
    let banner = page::parse(
        r#"<iframe src="/ad" width="300" height="250"></iframe>
        <iframe src="/leaderboard" style="width: 728px; height: 90px"></iframe>"#,
        &base,
    );
    assert_eq!(banner.frame, None, "ad sizes aren't games");
}

#[test]
fn a_lazy_iframe_or_a_flash_movie_is_the_game() {
    let base = Url::parse("https://armorgames.com/black-game/18545").unwrap();
    let lazy = page::parse(
        r#"<iframe src="/ad" width="300" height="250"></iframe>
        <iframe data-src="https://cache.armorgames.com/files/games/black-18545/index.html?v=1"
            style="display: none; width:960px; height:720px;" id="html-game-frame"></iframe>"#,
        &base,
    );
    assert_eq!(
        lazy.frame.as_ref().map(Url::as_str),
        Some("https://cache.armorgames.com/files/games/black-18545/index.html?v=1")
    );
    assert_eq!(lazy.frame_size, GameSize::new(960, 720));
    let blank = page::parse(
        r#"<iframe src="about:blank" data-src="game/" style="width: 800px"></iframe>"#,
        &base,
    );
    assert_eq!(
        blank.frame.as_ref().map(Url::as_str),
        Some("https://armorgames.com/black-game/game/")
    );
    assert_eq!(blank.frame_size, None, "no height");
    let flash = page::parse(
        r#"<iframe src="https://ads.example/banner" width="300" height="250"></iframe>
        <object classid="clsid:d27cdb6e-ae6d-11cf-96b8-444553540000" width="640" height="480">
            <param name="allowScriptAccess" value="always" />
            <param name="movie" value="https://cache.armorgames.com/files/games/gemcraft-1716.swf?v=1373587520" />
            <embed src="https://cache.armorgames.com/files/games/gemcraft-1716.swf?v=1373587520" />
        </object>"#,
        &base,
    );
    assert_eq!(
        flash.frame.as_ref().map(Url::as_str),
        Some("https://cache.armorgames.com/files/games/gemcraft-1716.swf?v=1373587520")
    );
    assert_eq!(flash.frame_size, GameSize::new(640, 480), "the <object>'s");
    let embed = page::parse(r#"<embed src="/files/Game.SWF" width="550">"#, &base);
    assert_eq!(
        embed.frame.as_ref().map(Url::as_str),
        Some("https://armorgames.com/files/Game.SWF")
    );
    assert_eq!(embed.frame_size, None);
    let not_flash = page::parse(r#"<embed src="/video.mp4">"#, &base);
    assert_eq!(not_flash.frame, None);
}

#[test]
fn pages_present_themselves_through_metadata_json_ld_and_microdata() {
    let base = Url::parse("https://games.example/play/").unwrap();
    let info = page::parse(
        r#"<head>
        <meta property="og:image" content="/og.jpg">
        <meta property="og:image:width" content="1200">
        <meta property="og:image:height" content="630">
        <link rel="image_src" href="/src.png">
        <meta itemprop="image" content="/micro.png">
        <script type="application/ld+json">{"@type":"VideoGame","name":"Needle",
          "image":{"@type":"ImageObject","url":"/ld.png"},
          "screenshot":["/shot1.jpg"],
          "publisher":{"@type":"Organization","logo":"/publisher-logo.png"}}</script>
        <link rel="manifest" href="app.webmanifest">
        <link rel="alternate" type="application/json+oembed" href="/oembed?url=x">
        <link rel="icon" href="/favicon.png">
        </head>"#,
        &base,
    );
    let pictures: Vec<_> = info
        .pictures
        .iter()
        .map(|picture| (picture.url.path(), picture.source))
        .collect();
    assert_eq!(
        pictures,
        [
            ("/og.jpg", Source::Preview),
            ("/micro.png", Source::Preview),
            ("/src.png", Source::Preview),
            ("/ld.png", Source::Preview),
            ("/shot1.jpg", Source::Preview),
            ("/favicon.png", Source::Icon),
            ("/favicon.ico", Source::Icon),
        ]
    );
    assert!(info.pictures[0].wide(), "1200 x 630 is a banner");
    assert!(info.has_preview());
    assert_eq!(
        info.manifest.as_ref().map(Url::as_str),
        Some("https://games.example/play/app.webmanifest")
    );
    assert_eq!(
        info.oembed.as_ref().map(Url::as_str),
        Some("https://games.example/oembed?url=x")
    );
}

#[test]
fn manifests_offer_their_square_icons_largest_first_then_the_rest() {
    let base = Url::parse("https://yp3d.com/tanks3d/manifest.json").unwrap();
    let pictures = page::manifest(
        r#"{"icons":[
            {"src":"https://yp3d.com/tanks3d/tanks3d-icon-256.png","sizes":"256x256"},
            {"src":"tanks3d-icon-1080-2.png","sizes":"1920x1080"},
            {"src":"tanks3d-icon-512.png","sizes":"512x512"},
            {"src":"mono.png","sizes":"512x512","purpose":"monochrome"}],
          "screenshots":[{"src":"/shot.webp","sizes":"1280x720"}]}"#,
        &base,
    );
    let urls: Vec<_> = pictures.iter().map(|picture| picture.url.path()).collect();
    assert_eq!(
        urls,
        [
            "/tanks3d/tanks3d-icon-512.png",
            "/tanks3d/tanks3d-icon-256.png",
            "/tanks3d/tanks3d-icon-1080-2.png",
            "/shot.webp",
        ]
    );
    assert!(pictures.iter().all(|picture| picture.source == Source::App));
    assert!(pictures[0].square() && pictures[2].wide());
    assert!(page::manifest("not json", &base).is_empty());

    let thumbnail = page::oembed(
        r#"{"type":"rich","thumbnail_url":"https://cdn.example/t.jpg","thumbnail_width":480,"thumbnail_height":360}"#,
        &base,
    )
    .unwrap();
    assert_eq!(thumbnail.source, Source::Embed);
    assert_eq!(thumbnail.size, Some((480, 360)));
    assert!(page::oembed("{}", &base).is_none());
}

/// A PNG of `width` × `height` in one colour.
fn png_of(width: u32, height: u32, colour: [u8; 4]) -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, Rgba(colour)));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[tokio::test]
async fn games_sharing_a_site_icon_get_their_manifests_own_pictures() {
    let server = MockServer::start().await;
    // Two games by one studio: the same favicon, a manifest each.
    for game in ["salty", "tanks"] {
        Mock::given(method("GET"))
            .and(path(format!("/{game}/")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"<title>Game</title><link rel="icon" href="/favicon.png"><link rel="manifest" href="manifest.json">"#,
                "text/html",
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/{game}/manifest.json")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                format!(r#"{{"icons":[{{"src":"{game}-512.png","sizes":"512x512"}},{{"src":"{game}-wide.png","sizes":"1920x1080"}}]}}"#),
                "application/json",
            ))
            .mount(&server)
            .await;
    }
    let colours = [("salty", [1, 0, 0, 255]), ("tanks", [0, 1, 0, 255])];
    for (game, colour) in colours {
        Mock::given(method("GET"))
            .and(path(format!("/{game}/{game}-512.png")))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(png_of(8, 8, colour), "image/png"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/{game}/{game}-wide.png")))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(png_of(16, 9, colour), "image/png"),
            )
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/favicon.png"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(png(), "image/png"))
        .mount(&server)
        .await;
    let store = LibraryStore::in_memory().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let web = WebGames::new(store.clone()).with_artwork(ArtworkCache::new(dir.path()).unwrap());
    let kinds = |art: &[crate::model::Artwork], kind| {
        art.iter()
            .filter(|art| art.kind == kind)
            .map(|art| (art.width, art.height, art.content_hash.clone()))
            .collect::<Vec<_>>()
    };
    let mut covers = Vec::new();
    for game in ["salty", "tanks"] {
        let id = web.add(&format!("{}/{game}/", server.uri())).await.unwrap();
        let art = store.artwork_for_game(&id).await.unwrap();
        let cover = kinds(&art, ArtworkKind::Cover);
        assert_eq!(cover.len(), 1, "{game}: one tile picture");
        assert_eq!(
            (cover[0].0, cover[0].1),
            (8, 8),
            "{game}: the manifest's square icon"
        );
        assert_eq!(
            kinds(&art, ArtworkKind::Hero)[0].0,
            16,
            "{game}: the wide one is the hero"
        );
        covers.push(cover[0].2.clone());
    }
    assert_ne!(covers[0], covers[1]);
}

#[tokio::test]
async fn a_picture_another_game_has_is_skipped_and_adding_again_looks_again() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/a/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"<link rel="icon" sizes="64x64" href="/shared.png">"#,
            "text/html",
        ))
        .mount(&server)
        .await;
    // B's page has the shared icon first, then one of its own.
    Mock::given(method("GET"))
        .and(path("/b/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"<link rel="icon" sizes="64x64" href="/shared.png"><link rel="icon" sizes="32x32" href="/b.png">"#,
            "text/html",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/shared.png"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(png_of(8, 8, [9, 9, 9, 255]), "image/png"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/b.png"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(png_of(4, 4, [7, 7, 7, 255]), "image/png"),
        )
        .mount(&server)
        .await;
    let store = LibraryStore::in_memory().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let web = WebGames::new(store.clone()).with_artwork(ArtworkCache::new(dir.path()).unwrap());
    let cover = |id: String| {
        let store = store.clone();
        async move {
            store
                .artwork_for_game(&id)
                .await
                .unwrap()
                .into_iter()
                .filter(|art| art.kind == ArtworkKind::Cover)
                .map(|art| art.width)
                .collect::<Vec<_>>()
        }
    };
    let a = web.add(&format!("{}/a/", server.uri())).await.unwrap();
    assert_eq!(cover(a.clone()).await, [8]);
    let b = web.add(&format!("{}/b/", server.uri())).await.unwrap();
    assert_eq!(cover(b.clone()).await, [4], "A has the shared icon");
    // Adding A again finds the same picture and keeps one tile picture.
    assert_eq!(web.add(&format!("{}/a/", server.uri())).await.unwrap(), a);
    assert_eq!(cover(a).await, [8]);
}

#[tokio::test]
async fn a_page_without_a_picture_gets_its_favicon() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/happy_wheels.tjf"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"<title>Happy Wheels</title><link rel="icon" href="img/favicon.ico">"#,
            "text/html",
        ))
        .mount(&server)
        .await;
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(16, 16, Rgba([1, 2, 3, 255])));
    let mut ico = std::io::Cursor::new(Vec::new());
    image.write_to(&mut ico, ImageFormat::Ico).unwrap();
    Mock::given(method("GET"))
        .and(path("/img/favicon.ico"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(ico.into_inner(), "image/x-icon"))
        .mount(&server)
        .await;
    let store = LibraryStore::in_memory().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let web = WebGames::new(store.clone()).with_artwork(ArtworkCache::new(dir.path()).unwrap());

    let id = web
        .add(&format!("{}/happy_wheels.tjf", server.uri()))
        .await
        .unwrap();
    let art = store.artwork_for_game(&id).await.unwrap();
    let cover = art.iter().find(|art| art.kind == ArtworkKind::Cover);
    assert_eq!(cover.map(|art| (art.width, art.height)), Some((16, 16)));
    assert!(art.iter().any(|art| art.kind == ArtworkKind::Icon));
}

#[tokio::test]
async fn adding_a_page_makes_a_web_game_with_its_picture() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/g/needle"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"<title>Needle 🕹️ Play on Example</title>
            <meta property="og:image" content="/missing.png">
            <link rel="apple-touch-icon" href="/touch.png">"#,
            "text/html",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/touch.png"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(png(), "image/png"))
        .expect(1)
        .mount(&server)
        .await;
    let store = LibraryStore::in_memory().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let web = WebGames::new(store.clone()).with_artwork(ArtworkCache::new(dir.path()).unwrap());

    let page = format!("{}/g/needle#top", server.uri());
    let id = web.add(&page).await.unwrap();
    let game = store.game(&id).await.unwrap().unwrap();
    assert_eq!(game.title, "Needle");
    assert_eq!(game.platforms, ["browser"]);
    let entries = store.entries_for_game(&id).await.unwrap();
    assert_eq!(entries.len(), 1);
    let url = format!("{}/g/needle", server.uri());
    assert_eq!(entries[0].source_id, WEB_SOURCE);
    assert_eq!(entries[0].external_id, url);
    assert_eq!(
        entries[0].launch,
        Some(LaunchSpec::Web { url: url.clone() })
    );
    let kinds: Vec<_> = store
        .artwork_for_game(&id)
        .await
        .unwrap()
        .into_iter()
        .map(|art| art.kind)
        .collect();
    assert!(kinds.contains(&ArtworkKind::Cover) && kinds.contains(&ArtworkKind::Icon));

    assert_eq!(
        web.add(&url).await.unwrap(),
        id,
        "the same page is the same game"
    );
    let listed = store
        .list_games(&ListGames {
            filter: GameFilter::Web,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(listed.total, 1);
    assert_eq!(store.library_stats().await.unwrap().web, 1);
}

/// itch.io's page for a game sold as a download: its trailer, Buy Now,
/// and no game.
const ITCH_FOR_SALE: &str = r#"<html><head><title>Lazy Kickers by SOS</title></head>
<body data-host="itch.io" data-page_name="view_game" class="game_layout_widget">
<div class="video_embed_widget"><iframe frameborder="0" style="position: absolute; width: 100%; height: 100%" src="//www.youtube.com/embed/eoVoNPJ3TlA" allowfullscreen></iframe></div>
<div class="buy_row"><a href="https://sos.itch.io/lazy-kickers/purchase" class="button buy_btn">Buy Now</a></div>
</body></html>"#;

#[test]
fn a_store_page_without_its_game_says_why_and_no_trailer_is_the_game() {
    let base = Url::parse("https://sos.itch.io/lazy-kickers").unwrap();
    let sold = page::parse(ITCH_FOR_SALE, &base);
    assert_eq!(sold.frame, None, "not the trailer");
    assert_eq!(sold.unplayable, Some(page::Unplayable::MustBuy));
    let download = page::parse(&ITCH_FOR_SALE.replace("buy_btn", "download_btn"), &base);
    assert_eq!(download.unplayable, Some(page::Unplayable::Download));
    let free = page::parse(
        r#"<body data-host="itch.io" data-page_name="view_game" class="embedded_game">
        <div class="html_embed_widget"><iframe id="game_drop" src="https://html-classic.itch.zone/html/235259/Celeste/index.html"></iframe></div>
        <a class="button buy_btn">Download</a></body>"#,
        &base,
    );
    assert_eq!(free.unplayable, None);
    assert_eq!(
        free.frame.as_ref().map(Url::as_str),
        Some("https://html-classic.itch.zone/html/235259/Celeste/index.html")
    );
    let trailer = page::parse(
        r#"<iframe src="https://www.youtube-nocookie.com/embed/x" width="960" height="540"></iframe>
        <iframe src="https://player.vimeo.com/video/1" width="960" height="540"></iframe>"#,
        &Url::parse("https://games.example/g").unwrap(),
    );
    assert_eq!((trailer.frame, trailer.unplayable), (None, None));
}

#[tokio::test]
async fn a_game_that_has_to_be_bought_isnt_added() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(ITCH_FOR_SALE, "text/html"))
        .mount(&server)
        .await;
    let store = LibraryStore::in_memory().await.unwrap();
    let error = WebGames::new(store.clone())
        .add(&server.uri())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("has to be bought"), "{error}");
    let listed = store.list_games(&ListGames::default()).await.unwrap();
    assert_eq!(listed.total, 0);
}

#[tokio::test]
async fn a_page_that_cant_be_read_is_named_after_its_host() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let store = LibraryStore::in_memory().await.unwrap();
    let web = WebGames::new(store.clone());
    let id = web.add(&server.uri()).await.unwrap();
    let game = store.game(&id).await.unwrap().unwrap();
    assert_eq!(game.title, "127.0.0.1");
    assert!(web.add("ftp://example.com/").await.is_err());
}

#[tokio::test]
async fn removing_a_web_game_keeps_a_game_with_other_entries() {
    let store = LibraryStore::in_memory().await.unwrap();
    let web = WebGames::new(store.clone());
    let alone = web.add("http://127.0.0.1:9/alone").await.unwrap();
    let merged = web.add("http://127.0.0.1:9/merged").await.unwrap();
    let mut other =
        crate::model::GameEntry::new(&merged, caskade_discovery::SourceId::new("steam"), "4000");
    other.state = caskade_discovery::InstallState::Installed;
    store.upsert_entry(&other).await.unwrap();

    web.remove(&alone).await.unwrap();
    assert!(store.game(&alone).await.unwrap().is_none());
    web.remove(&merged).await.unwrap();
    let entries = store.entries_for_game(&merged).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_ne!(entries[0].source_id, WEB_SOURCE);
    assert!(web.remove(&merged).await.is_err(), "no web entry left");
}

/// Two real games from one studio that share a favicon: each gets its own
/// manifest icon. Needs the network.
#[tokio::test]
#[ignore = "needs saltyseas.io and yp3d.com"]
async fn live() {
    let store = LibraryStore::in_memory().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let web = WebGames::new(store.clone()).with_artwork(ArtworkCache::new(dir.path()).unwrap());
    let mut covers = Vec::new();
    for page in ["https://saltyseas.io/app/", "https://yp3d.com/tanks3d/"] {
        let id = web.add(page).await.unwrap();
        let art = store.artwork_for_game(&id).await.unwrap();
        for art in &art {
            println!("{page}: {:?} {}x{}", art.kind, art.width, art.height);
        }
        let cover = art
            .iter()
            .find(|art| art.kind == ArtworkKind::Cover)
            .expect("a tile picture");
        covers.push(cover.content_hash.clone());
    }
    assert_ne!(covers[0], covers[1]);
}
