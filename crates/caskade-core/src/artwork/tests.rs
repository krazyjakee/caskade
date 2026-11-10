use super::*;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::{
    fs,
    io::Cursor,
    time::{Duration, SystemTime},
};

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        width,
        height,
        Rgba([21, 33, 55, 180]),
    ));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[tokio::test]
async fn shared_cache_bytes_round_trip_as_distinct_game_associations() {
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let cache = ArtworkCache::new(root.path()).unwrap();
    let first = crate::model::Game::new("Original One");
    let second = crate::model::Game::new("Original Two");
    store.upsert_game(&first).await.unwrap();
    store.upsert_game(&second).await.unwrap();
    let cached = cache.import_bytes(png(32, 48)).await.unwrap();
    let one = cached.record(&first.id, crate::model::ArtworkKind::Cover, "steam");
    let two = cached.record(&second.id, crate::model::ArtworkKind::Icon, "installer");
    store.upsert_artwork(&one).await.unwrap();
    store.upsert_artwork(&two).await.unwrap();
    assert_eq!(
        store.artwork_for_game(&first.id).await.unwrap(),
        std::slice::from_ref(&one)
    );
    assert_eq!(
        store.artwork_for_game(&second.id).await.unwrap(),
        std::slice::from_ref(&two)
    );
    assert_ne!(one.id, two.id);
    assert_eq!(one.cached_path, two.cached_path);
    store.delete_artwork(&one.id).await.unwrap();
    assert!(store.artwork_for_game(&first.id).await.unwrap().is_empty());
    assert_eq!(store.artwork_for_game(&second.id).await.unwrap(), [two]);
    assert_eq!(
        serve(&cache, &cached.thumbnail_uri(320).unwrap())
            .await
            .status,
        200
    );
    store.close().await;
}

#[tokio::test]
async fn thumbnails_are_bounded_keep_alpha_and_are_generated_only_once() {
    let root = tempfile::tempdir().unwrap();
    let bytes = png(900, 1200);
    let cache = ArtworkCache::new(root.path()).unwrap();
    let art = cache.import_bytes(bytes.clone()).await.unwrap();
    assert_eq!((art.width, art.height), (900, 1200));
    assert_eq!(art.content_hash, cache::digest(&bytes));
    let cover = art.record("game-one", crate::model::ArtworkKind::Cover, "steam");
    let icon = art.record("game-two", crate::model::ArtworkKind::Icon, "installer");
    assert_ne!(cover.id, icon.id);
    assert_eq!(cover.content_hash, icon.content_hash);
    assert_eq!(cover.thumbnails[1].path, art.thumbnails[1].path);
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(17);
    for thumbnail in &art.thumbnails {
        let path = root.path().join(&thumbnail.path);
        let image = image::open(&path).unwrap();
        assert_eq!(
            (image.width(), image.height()),
            (thumbnail.width, thumbnail.height)
        );
        assert_eq!(image.width().max(image.height()), thumbnail.size);
        assert_eq!(image.to_rgba8().get_pixel(0, 0).0[3], 180);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();
    }
    // Separate cache instances prove that the on-disk manifest, not a memory
    // map, provides restart reuse. Concurrent imports exercise the OS lock.
    let other = ArtworkCache::new(root.path()).unwrap();
    let (one, two) = tokio::join!(cache.import_bytes(bytes.clone()), other.import_bytes(bytes));
    assert_eq!(one.unwrap(), art);
    assert_eq!(two.unwrap(), art);
    for thumbnail in &art.thumbnails {
        assert_eq!(
            fs::metadata(root.path().join(&thumbnail.path))
                .unwrap()
                .modified()
                .unwrap(),
            old
        );
    }
    let grid = art.thumbnail_uri(320).unwrap();
    assert!(grid.ends_with("/thumb-320.webp"));
    assert!(!grid.contains("original"));
    assert!(art.thumbnail_uri(4000).is_none());
}

#[tokio::test]
async fn protocol_serves_only_known_cache_images_for_both_webview_url_forms() {
    let root = tempfile::tempdir().unwrap();
    let cache = ArtworkCache::new(root.path()).unwrap();
    let art = cache.import_bytes(png(80, 120)).await.unwrap();
    for prefix in [
        "caskade-asset://localhost/",
        "http://caskade-asset.localhost/",
        "/",
    ] {
        for path in [
            format!("{}/thumb-320.webp", art.content_hash),
            format!("{}%2Fthumb-320.webp", art.content_hash),
        ] {
            let response = serve(&cache, &format!("{prefix}{path}")).await;
            assert_eq!(response.status, 200);
            assert_eq!(response.mime, "image/webp");
            assert!(response.cache_control.contains("immutable"));
            assert!(image::load_from_memory(&response.bytes).is_ok());
        }
    }
    for uri in [
        "/../private",
        "/%2e%2e/private",
        "/%252e%252e/private",
        "/%FF/private",
        "/%/private",
        "file:///etc/passwd",
        "caskade-asset://evilhost/private",
        "caskade-asset://localhost.evil/private",
        "http://evil/caskade-asset.localhost/private",
        "//server/share",
        "/C:/private",
    ] {
        assert_ne!(serve(&cache, uri).await.status, 200, "{uri}");
    }
    for tail in [
        "../private",
        "entry.json",
        ".lock",
        "thumb-4096.webp",
        "thumb-320.webp?x=1",
        "thumb-320.webp#x",
        "thumb-320.webp/extra",
        "..%5Cprivate",
    ] {
        let uri = format!("caskade-asset://localhost/{}/{tail}", art.content_hash);
        assert_ne!(serve(&cache, &uri).await.status, 200);
    }
    assert_eq!(
        serve(&cache, &format!("/{}/thumb-320.webp", "0".repeat(64)))
            .await
            .status,
        404
    );
    assert_eq!(
        serve(&cache, &format!("/{}/thumb-320.webp", "A".repeat(64)))
            .await
            .status,
        400
    );
}

#[cfg(unix)]
#[tokio::test]
async fn protocol_and_import_cannot_follow_cache_links_outside_the_cache() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("private.webp");
    fs::write(&secret, b"outside sentinel").unwrap();
    let cache = ArtworkCache::new(root.path()).unwrap();
    let bytes = png(30, 40);
    let art = cache.import_bytes(bytes.clone()).await.unwrap();
    let target = root.path().join(&art.thumbnails[1].path);
    fs::remove_file(&target).unwrap();
    symlink(&secret, &target).unwrap();
    assert_ne!(
        serve(&cache, &art.thumbnail_uri(320).unwrap()).await.status,
        200
    );
    assert!(cache.import_bytes(bytes).await.is_err());
    let bytes = png(31, 40);
    symlink(outside.path(), root.path().join(cache::digest(&bytes))).unwrap();
    assert!(cache.import_bytes(bytes).await.is_err());
    assert_eq!(fs::read(secret).unwrap(), b"outside sentinel");
}

#[tokio::test]
async fn malformed_oversized_and_interrupted_artwork_is_bounded_or_recovered() {
    let root = tempfile::tempdir().unwrap();
    let cache = ArtworkCache::new(root.path()).unwrap();
    assert!(
        cache
            .import_bytes(b"<svg>not a supported raster</svg>".to_vec())
            .await
            .is_err()
    );
    assert!(matches!(
        cache
            .import_bytes(vec![0; cache::MAX_INPUT_BYTES + 1])
            .await,
        Err(ArtworkError::Limit(_))
    ));
    assert!(matches!(
        cache.import_bytes(png(8193, 1)).await,
        Err(ArtworkError::Limit(_))
    ));
    let bytes = png(30, 40);
    let art = cache.import_bytes(bytes.clone()).await.unwrap();
    fs::remove_file(root.path().join(&art.thumbnails[0].path)).unwrap();
    assert_eq!(cache.import_bytes(bytes).await.unwrap(), art);
    assert_eq!(
        serve(&cache, &art.thumbnail_uri(96).unwrap()).await.status,
        200
    );
}

#[cfg(feature = "net")]
#[tokio::test]
async fn http_artwork_is_downloaded_once_across_restarts_and_decoded_off_thread() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/art.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(png(30, 40)))
        .expect(1)
        .mount(&server)
        .await;
    let root = tempfile::tempdir().unwrap();
    let cache = ArtworkCache::new(root.path()).unwrap();
    let url = format!("{}/art.png", server.uri());
    let (one, two) = tokio::join!(cache.download(&url), cache.download(&url));
    let one = one.unwrap();
    assert_eq!(one, two.unwrap());
    assert_eq!(
        one,
        ArtworkCache::new(root.path())
            .unwrap()
            .download(&url)
            .await
            .unwrap()
    );
    for url in [
        "file:///etc/passwd",
        "https://user:secret@example.invalid/icon.png",
    ] {
        let error = cache.download(url).await.unwrap_err();
        assert!(!error.to_string().contains("secret"));
    }
}

#[cfg(feature = "net")]
#[tokio::test]
async fn rejected_downloads_never_publish_a_content_mapping() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
    let server = MockServer::start().await;
    Mock::given(path("/bad"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not an image"))
        .mount(&server)
        .await;
    Mock::given(path("/large"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", (cache::MAX_INPUT_BYTES + 1).to_string()),
        )
        .mount(&server)
        .await;
    let root = tempfile::tempdir().unwrap();
    let cache = ArtworkCache::new(root.path()).unwrap();
    for name in ["bad", "large", "missing"] {
        let url = format!("{}/{name}", server.uri());
        assert!(cache.download(&url).await.is_err());
        let source = root
            .path()
            .join("sources")
            .join(cache::digest(url.as_bytes()));
        assert!(!source.join("content.json").exists());
    }
}

#[cfg(feature = "net")]
#[tokio::test]
async fn download_failures_say_whether_trying_again_can_help() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
    let server = MockServer::start().await;
    Mock::given(path("/bad"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not an image"))
        .mount(&server)
        .await;
    Mock::given(path("/busy"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let root = tempfile::tempdir().unwrap();
    let cache = ArtworkCache::new(root.path()).unwrap();
    let fail = |name: &str| cache.download_error(format!("{}/{name}", server.uri()));
    let missing = fail("missing").await;
    assert!(missing.permanent() && !missing.unreachable(), "{missing}");
    assert!(missing.to_string().contains("404"), "{missing}");
    assert!(fail("bad").await.permanent());
    let busy = fail("busy").await;
    assert!(!busy.permanent() && !busy.unreachable(), "{busy}");

    // Nothing in the tests binds port 1 (it takes root on Unix); a port freed
    // by a dropped listener could go to another test's server (#119).
    let port = 1;
    let gone = cache
        .download_error(format!("http://127.0.0.1:{port}/art.png"))
        .await;
    assert!(gone.unreachable() && !gone.permanent(), "{gone}");
    assert!(gone.to_string().contains("can't reach 127.0.0.1"), "{gone}");
}

#[cfg(feature = "net")]
#[tokio::test]
async fn metadata_images_never_reach_local_or_private_addresses() {
    // CSK-11: a cover URL a compromised metadata server sends.
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(png(8, 8)))
        .mount(&server)
        .await;
    let port = server.address().port();
    let root = tempfile::tempdir().unwrap();
    let cache = ArtworkCache::new(root.path()).unwrap();
    for url in [
        format!("{}/cover.png", server.uri()),
        // A name that resolves to loopback.
        format!("http://localhost:{port}/cover.png"),
        format!("http://[::ffff:127.0.0.1]:{port}/cover.png"),
        "http://192.168.1.1/cgi-bin/reboot".to_owned(),
        "http://169.254.169.254/latest/meta-data/".to_owned(),
    ] {
        let error = cache.download_metadata(&url).await.unwrap_err();
        assert!(error.permanent() && !error.unreachable(), "{url}: {error}");
        assert!(
            error.to_string().contains("local or private"),
            "{url}: {error}"
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
    // The user's own URLs, and tests that serve metadata locally, still work.
    let url = format!("{}/cover.png", server.uri());
    cache.download(&url).await.unwrap();
    ArtworkCache::new(root.path().join("local"))
        .unwrap()
        .allowing_local_metadata()
        .download_metadata(&format!("{url}?local"))
        .await
        .unwrap();
}

#[cfg(feature = "net")]
#[test]
fn only_public_addresses_count_as_public() {
    use std::net::IpAddr;
    for local in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "0.0.0.0",
        "255.255.255.255",
        "224.0.0.1",
        "::1",
        "::",
        "fe80::1",
        "fd00::1",
        "::ffff:10.0.0.1",
    ] {
        assert!(
            !download::public_ip(local.parse::<IpAddr>().unwrap()),
            "{local}"
        );
    }
    for public in ["93.184.216.34", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(
            download::public_ip(public.parse::<IpAddr>().unwrap()),
            "{public}"
        );
    }
}

#[cfg(feature = "net")]
impl ArtworkCache {
    async fn download_error(&self, url: String) -> ArtworkError {
        self.download(&url).await.unwrap_err()
    }
}

#[cfg(feature = "net")]
#[tokio::test]
async fn downloads_of_different_urls_overlap() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(png(8, 8))
                .set_delay(Duration::from_secs(20)),
        )
        .mount(&server)
        .await;
    let root = tempfile::tempdir().unwrap();
    let cache = ArtworkCache::new(root.path()).unwrap();
    let (one, two) = (
        format!("{}/one.png", server.uri()),
        format!("{}/two.png", server.uri()),
    );
    let both = async {
        let _ = tokio::join!(cache.download(&one), cache.download(&two));
    };
    // Both requests reach the server while the first is still unanswered.
    let asked = async {
        while server.received_requests().await.unwrap_or_default().len() < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::select! {
        () = both => panic!("the downloads finished before both were asked for"),
        result = tokio::time::timeout(Duration::from_secs(10), asked) => {
            result.expect("the second download waited for the first");
        }
    }
}
