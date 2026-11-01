#![cfg(feature = "fetch")]
#[cfg(feature = "cli")]
use caskade_installer::manifest::{InstallManifest, ROOT_MANIFEST_PATH};
use caskade_installer::{
    Question,
    exec::CancellationToken,
    fetch::{
        DownloadRequest, FetchConfig, FetchEngine, FetchError, Resolution, ResolverConfig,
        ResolverRegistry,
    },
    host::HostCaps,
    testing::{FakeDiskSpace, RecordingSink, TestRoot},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, sync::Arc};
#[cfg(feature = "cli")]
use std::{path::Path, process::Command};
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};

fn host(name: &str) -> HostCaps {
    toml::from_str(match name {
        "windows" => include_str!("hosts/windows.toml"),
        "linux" => include_str!("hosts/linux.toml"),
        "linux-no-umu" => include_str!("hosts/linux-no-umu.toml"),
        "linux-exfat-root" => include_str!("hosts/linux-exfat-root.toml"),
        "macos" => include_str!("hosts/macos.toml"),
        "macos-no-wine" => include_str!("hosts/macos-no-wine.toml"),
        _ => panic!("unknown profile"),
    })
    .unwrap()
}
fn config(server: &MockServer) -> ResolverConfig {
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    ResolverConfig {
        github_web: base.join("github/").unwrap(),
        github_api: base.join("api/").unwrap(),
        dropbox_web: base.join("dropbox/").unwrap(),
        drive_web: base.join("drive/").unwrap(),
        drive_download: base.join("drive/uc").unwrap(),
        drive_content: base.join("content/").unwrap(),
        ..Default::default()
    }
}
fn engine(config: ResolverConfig) -> FetchEngine {
    FetchEngine::new(
        FetchConfig {
            retries: 0,
            disk_margin_bytes: 0,
            ..Default::default()
        },
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .unwrap()
    .with_resolver_config(config)
    .unwrap()
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn recorded(server: &MockServer) -> Value {
    let mut value: Value =
        serde_json::from_str(include_str!("fetch-responses/github-7zip-26.05.json")).unwrap();
    for asset in value["assets"].as_array_mut().unwrap() {
        let mut url = Url::parse(&format!("{}/assets/", server.uri())).unwrap();
        url.path_segments_mut()
            .unwrap()
            .pop_if_empty()
            .push(asset["name"].as_str().unwrap());
        asset["browser_download_url"] = url.to_string().into();
    }
    value
}
fn snapshot(resolution: Resolution, server: &MockServer) -> Value {
    let value = match resolution {
        Resolution::Downloads(requests) => json!({"downloads":requests}),
        Resolution::NeedsUser(question) => json!({"needs_user":question}),
    };
    serde_json::from_str(
        &serde_json::to_string(&value)
            .unwrap()
            .replace(&server.uri(), "https://fixture.invalid"),
    )
    .unwrap()
}
fn one(resolution: Resolution) -> DownloadRequest {
    match resolution {
        Resolution::Downloads(mut files) => {
            assert_eq!(files.len(), 1);
            files.remove(0)
        }
        Resolution::NeedsUser(question) => panic!("unexpected question: {question:?}"),
    }
}
async fn release(server: &MockServer, endpoint: &str, value: Value) {
    Mock::given(path(endpoint))
        .respond_with(ResponseTemplate::new(200).set_body_json(value))
        .mount(server)
        .await;
}

#[tokio::test]
async fn recorded_github_release_has_stable_six_host_and_arm_asset_choices() {
    let server = MockServer::start().await;
    release(
        &server,
        "/api/repos/ip7z/7zip/releases/latest",
        recorded(&server),
    )
    .await;
    let resolver = engine(config(&server));
    let url = Url::parse(&format!(
        "{}/github/ip7z/7zip/releases/latest",
        server.uri()
    ))
    .unwrap();
    let mut snapshots = BTreeMap::new();
    for name in [
        "windows",
        "linux",
        "linux-no-umu",
        "linux-exfat-root",
        "macos",
        "macos-no-wine",
    ] {
        snapshots.insert(
            name.to_owned(),
            snapshot(resolver.resolve(&url, &host(name)).await.unwrap(), &server),
        );
    }
    for name in ["windows", "linux", "macos"] {
        let mut profile = host(name);
        profile.arch = "aarch64".into();
        snapshots.insert(
            format!("{name}-arm64"),
            snapshot(resolver.resolve(&url, &profile).await.unwrap(), &server),
        );
    }
    insta::assert_json_snapshot!("recorded_host_choices", snapshots);
}

#[tokio::test]
async fn github_repo_latest_release_and_encoded_tag_routes_are_distinct_from_direct_assets() {
    let server = MockServer::start().await;
    let cfg = config(&server);
    let mut value = recorded(&server);
    value["assets"]
        .as_array_mut()
        .unwrap()
        .retain(|a| a["name"] == "7z2605-linux-x64.tar.xz");
    for route in [
        "/api/repos/ip7z/7zip/releases/latest",
        "/api/repos/ip7z/7zip/releases/tags/v1%2Fbranch",
    ] {
        release(&server, route, value.clone()).await;
    }
    let registry = ResolverRegistry::with_builtins(cfg).unwrap();
    for route in [
        "ip7z/7zip",
        "ip7z/7zip.git",
        "ip7z/7zip/releases",
        "ip7z/7zip/releases/latest",
        "ip7z/7zip/releases/tag/v1%2Fbranch",
    ] {
        let url = Url::parse(&format!("{}/github/{route}", server.uri())).unwrap();
        assert_eq!(
            one(registry.resolve(&url, &host("linux")).await.unwrap())
                .file_name
                .as_deref(),
            Some("7z2605-linux-x64.tar.xz")
        );
    }
    let direct = Url::parse(&format!(
        "{}/github/ip7z/7zip/releases/download/26.05/game.zip",
        server.uri()
    ))
    .unwrap();
    assert_eq!(
        one(registry.resolve(&direct, &host("linux")).await.unwrap()).url,
        direct
    );
}

#[tokio::test]
async fn github_sidecars_support_unicode_spaces_bsd_and_conflicts() {
    let server = MockServer::start().await;
    let hash = digest(b"fixture");
    let resolver = engine(config(&server));
    for (index, text) in [
        format!("{hash}  *Café Game-linux-x64.zip\n"),
        format!("SHA256 (Café Game-linux-x64.zip) = {hash}\n"),
        format!("{hash}\n"),
    ]
    .into_iter()
    .enumerate()
    {
        let value = json!({"assets":[{"name":"Café Game-linux-x64.zip","browser_download_url":format!("{}/file/{index}",server.uri()),"size":7,"state":"uploaded"},{"name":"Café Game-linux-x64.zip.sha256","browser_download_url":format!("{}/sum/{index}",server.uri()),"size":text.len(),"state":"uploaded"}]});
        release(
            &server,
            &format!("/api/repos/owner/game{index}/releases/latest"),
            value,
        )
        .await;
        Mock::given(path(format!("/sum/{index}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(text))
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/github/owner/game{index}", server.uri())).unwrap();
        assert_eq!(
            one(resolver.resolve(&url, &host("linux")).await.unwrap())
                .sha256
                .as_deref(),
            Some(hash.as_str())
        );
    }
    let bad = format!(
        "{hash}  game-linux-x64.zip\n{}  game-linux-x64.zip\n",
        "a".repeat(64)
    );
    release(&server,"/api/repos/owner/bad/releases/latest",json!({"assets":[{"name":"game-linux-x64.zip","browser_download_url":format!("{}/file",server.uri()),"size":7},{"name":"SHA256SUMS","browser_download_url":format!("{}/bad-sums",server.uri()),"size":bad.len()}]})).await;
    Mock::given(path("/bad-sums"))
        .respond_with(ResponseTemplate::new(200).set_body_string(bad))
        .mount(&server)
        .await;
    assert!(matches!(
        resolver
            .resolve(
                &Url::parse(&format!("{}/github/owner/bad", server.uri())).unwrap(),
                &host("linux")
            )
            .await,
        Err(FetchError::InvalidResponse { .. })
    ));
}

#[tokio::test]
async fn equal_github_assets_ask_instead_of_picking_a_filename() {
    let server = MockServer::start().await;
    release(&server,"/api/repos/owner/tie/releases/latest",json!({"assets":[
        {"name":"Game-linux-x64.zip","browser_download_url":format!("{}/a",server.uri()),"size":1},
        {"name":"Game-other-linux-x64.zip","browser_download_url":format!("{}/b",server.uri()),"size":1}]})).await;
    let result = engine(config(&server))
        .resolve(
            &Url::parse(&format!("{}/github/owner/tie", server.uri())).unwrap(),
            &host("linux"),
        )
        .await
        .unwrap();
    assert!(
        matches!(&result,Resolution::NeedsUser(Question::ChooseAsset{assets}) if assets.len()==2)
    );
    insta::assert_json_snapshot!("equal_asset_choice", snapshot(result, &server));
}

#[tokio::test]
async fn metadata_requests_retry_server_errors_and_refuse_web_pages() {
    let server = MockServer::start().await;
    Mock::given(path("/api/repos/owner/busy/releases/latest"))
        .respond_with(ResponseTemplate::new(503))
        .expect(3)
        .mount(&server)
        .await;
    Mock::given(path("/api/repos/owner/portal/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "<html>Sign in to the network</html>",
            "text/html; charset=utf-8",
        ))
        .mount(&server)
        .await;
    let resolve = |repo: &str| {
        let url = Url::parse(&format!("{}/github/owner/{repo}", server.uri())).unwrap();
        let engine = engine(config(&server));
        async move { engine.resolve(&url, &host("linux")).await }
    };
    let busy = resolve("busy").await;
    assert!(
        matches!(busy, Err(FetchError::Status { status: 503 })),
        "{busy:?}"
    );
    let portal = resolve("portal").await;
    let Err(FetchError::InvalidResponse { message }) = portal else {
        panic!("a web page is not release metadata: {portal:?}");
    };
    assert!(message.contains("web page"), "{message}");
}

#[tokio::test]
async fn rate_limited_github_api_falls_back_to_the_release_page_assets() {
    let server = MockServer::start().await;
    Mock::given(path("/api/repos/owner/game/releases/latest"))
        .respond_with(ResponseTemplate::new(403).set_body_string("API rate limit exceeded"))
        .mount(&server)
        .await;
    Mock::given(path("/github/owner/game/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "/github/owner/game/releases/tag/v1.2"),
        )
        .mount(&server)
        .await;
    Mock::given(path("/github/owner/game/releases/tag/v1.2"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
        .mount(&server)
        .await;
    // What github.com serves: asset links, source archives and other links.
    let page = r#"<ul>
        <li><a href="/github/owner/game/releases/download/v1.2/Game-1.2-windows-x64.zip">Game</a></li>
        <li><a href="/github/owner/game/releases/download/v1.2/Game-1.2-linux-x64.tar.gz">Game</a></li>
        <li><a href="/github/owner/game/releases/download/v1.2/Game-1.2-linux-x64.tar.gz">again</a></li>
        <li><a href="/github/owner/game/archive/refs/tags/v1.2.zip">Source code</a></li>
        <li><a href="/github/owner/other/releases/download/v1.2/Other-linux.zip">elsewhere</a></li>
    </ul>"#;
    Mock::given(path("/github/owner/game/releases/expanded_assets/v1.2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(page))
        .mount(&server)
        .await;
    let request = one(engine(config(&server))
        .resolve(
            &Url::parse(&format!("{}/github/owner/game", server.uri())).unwrap(),
            &host("linux"),
        )
        .await
        .unwrap());
    assert_eq!(
        request.file_name.as_deref(),
        Some("Game-1.2-linux-x64.tar.gz")
    );
    assert_eq!(
        request.url.as_str(),
        format!(
            "{}/github/owner/game/releases/download/v1.2/Game-1.2-linux-x64.tar.gz",
            server.uri()
        )
    );
    // The page has no sizes or digests.
    assert_eq!((request.size, request.sha256), (None, None));
}

#[tokio::test]
async fn dropbox_keeps_resource_keys_and_replaces_all_download_switches() {
    let server = MockServer::start().await;
    let registry = ResolverRegistry::with_builtins(config(&server)).unwrap();
    let mut snapshots = vec![];
    for route in [
        "s/token/Caf%C3%A9.zip?dl=0&dl=0&raw=1&key=keep",
        "scl/fi/token/game.zip?rlkey=resource&dl=0#sha256=abcdef",
    ] {
        let source = Url::parse(&format!("{}/dropbox/{route}", server.uri())).unwrap();
        let request = one(registry.resolve(&source, &host("linux")).await.unwrap());
        assert_eq!(
            request
                .url
                .query_pairs()
                .filter(|(k, _)| k == "dl")
                .collect::<Vec<_>>(),
            vec![("dl".into(), "1".into())]
        );
        assert!(!request.url.query_pairs().any(|(key, _)| key == "raw"));
        snapshots.push(snapshot(Resolution::Downloads(vec![request]), &server));
    }
    insta::assert_json_snapshot!("dropbox_links", snapshots);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn default_builtins_and_custom_resolver_priority_need_no_network() {
    use caskade_installer::fetch::{ResolveFuture, UrlResolver};
    struct Custom;
    impl UrlResolver for Custom {
        fn id(&self) -> &str {
            "custom"
        }
        fn claims(&self, _: &Url) -> bool {
            true
        }
        fn resolve<'a>(&'a self, _: &'a Url, _: &'a HostCaps) -> ResolveFuture<'a> {
            Box::pin(async {
                Ok(Resolution::Downloads(vec![DownloadRequest::new(
                    Url::parse("https://custom.invalid/game.zip").unwrap(),
                )]))
            })
        }
    }
    let mut engine = FetchEngine::new(
        FetchConfig::default(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .unwrap();
    let request = one(engine
        .resolve(
            &Url::parse("https://dropbox.com/s/token/game.zip?dl=0&rlkey=keep").unwrap(),
            &host("linux"),
        )
        .await
        .unwrap());
    assert!(
        request
            .url
            .query_pairs()
            .any(|(key, value)| key == "dl" && value == "1")
    );
    engine.register(Arc::new(Custom)).unwrap();
    let engine = engine
        .with_resolver_config(ResolverConfig::default())
        .unwrap();
    let request = one(engine
        .resolve(
            &Url::parse("https://github.com/owner/repo").unwrap(),
            &host("linux"),
        )
        .await
        .unwrap());
    assert_eq!(request.url.as_str(), "https://custom.invalid/game.zip");
}

#[tokio::test]
async fn drive_form_confirmation_downloads_a_real_body_with_safe_decoded_names() {
    let server = MockServer::start().await;
    let cfg = config(&server);
    let html = include_str!("fetch-responses/drive-confirmation.html")
        .replace("{content_base}", cfg.drive_content.as_str());
    Mock::given(path("/drive/uc"))
        .and(query_param("id", "fixture_file"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html; charset=utf-8")
                .set_body_raw(html, "text/html")
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&server)
        .await;
    Mock::given(path("/content/download"))
        .and(query_param("id", "fixture_file"))
        .and(query_param("confirm", "t"))
        .and(query_param("uuid", "anonymous-fixture-uuid"))
        .and(query_param("resourcekey", "key&part"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(
                    "content-disposition",
                    "attachment; filename*=UTF-8''Caf%C3%A9.zip",
                )
                .set_body_bytes(b"fixture".to_vec()),
        )
        .mount(&server)
        .await;
    let engine = engine(cfg);
    let url = Url::parse(&format!("{}/drive/file/d/fixture_file/view", server.uri())).unwrap();
    let request = one(engine.resolve(&url, &host("linux")).await.unwrap());
    insta::assert_json_snapshot!(
        "drive_confirmation",
        snapshot(Resolution::Downloads(vec![request.clone()]), &server)
    );
    let root = TestRoot::new().unwrap();
    let file = engine
        .download(root.path(), "drive", &request, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(file.path.file_name().unwrap(), "Café.zip");
    assert_eq!(fs::read(&file.path).unwrap(), b"fixture");
    engine.after_install(root.path(), "drive").await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn drive_warning_cookie_and_direct_download_preserve_fragment_checksums() {
    let server = MockServer::start().await;
    let cfg = config(&server);
    let hash = digest(b"fixture");
    Mock::given(path("/drive/uc"))
        .and(query_param("id", "legacy"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html")
                .insert_header(
                    "set-cookie",
                    "download_warning_legacy=confirmation_token; Path=/; HttpOnly",
                )
                .set_body_raw("<html>Large file</html>", "text/html")
                .insert_header("content-type", "text/html"),
        )
        .mount(&server)
        .await;
    let source = Url::parse(&format!(
        "{}/drive/open?id=legacy#sha256={hash}",
        server.uri()
    ))
    .unwrap();
    let request = one(engine(cfg.clone())
        .resolve(&source, &host("linux"))
        .await
        .unwrap());
    assert_eq!(
        request.url.fragment(),
        Some(format!("sha256={hash}").as_str())
    );
    assert!(
        request
            .url
            .query_pairs()
            .any(|(k, v)| k == "confirm" && v == "confirmation_token")
    );
    assert_eq!(
        request.headers["cookie"],
        "download_warning_legacy=confirmation_token"
    );
    Mock::given(path("/drive/uc"))
        .and(query_param("id", "small"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-disposition", "attachment; filename=small.zip")
                .set_body_bytes(b"fixture".to_vec()),
        )
        .mount(&server)
        .await;
    let request = one(engine(cfg)
        .resolve(
            &Url::parse(&format!("{}/drive/file/d/small/view", server.uri())).unwrap(),
            &host("linux"),
        )
        .await
        .unwrap());
    assert_eq!(request.file_name.as_deref(), Some("small.zip"));
    assert_eq!(request.size, Some(7));
}

#[tokio::test]
async fn drive_rejects_login_pages_unsafe_actions_wrong_ids_and_oversized_fields() {
    for html in ["<html>Sign in</html>".into(),
        "<form id='download-form' action='https://evil.invalid/download'><input name='id' value='fixture_file'><input name='confirm' value='t'></form>".into(),
        "<form id='download-form' action='{content_base}download'><input name='id' value='different_file'><input name='confirm' value='t'></form>".into(),
        "<form id='download-form' method='post' action='{content_base}download'><input name='id' value='fixture_file'><input name='confirm' value='t'></form>".into(),
        format!("<form id='download-form' action='{{content_base}}download'><input name='id' value='fixture_file'><input name='confirm' value='{}'></form>","x".repeat(9000)),
        "<form id='download-form' action='{content_base}download'><input name='id' value='fixture_file'><input name='id' value='fixture_file'><input name='confirm' value='t'></form>".into()] {
        let server=MockServer::start().await;let cfg=config(&server);
        Mock::given(path("/drive/uc")).respond_with(ResponseTemplate::new(200).insert_header("content-type","text/html").set_body_raw(html.replace("{content_base}",cfg.drive_content.as_str()),"text/html").insert_header("content-type","text/html")).mount(&server).await;
        assert!(matches!(engine(cfg).resolve(&Url::parse(&format!("{}/drive/file/d/fixture_file/view",server.uri())).unwrap(),&host("linux")).await,Err(FetchError::InvalidResponse{..})));
    }
}

#[tokio::test]
async fn metadata_limits_redirect_loops_and_bad_checksums_are_explicit_errors() {
    let server = MockServer::start().await;
    let mut cfg = config(&server);
    cfg.metadata_max_bytes = 128;
    Mock::given(path("/api/repos/owner/large/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(1024)))
        .mount(&server)
        .await;
    assert!(matches!(
        engine(cfg)
            .resolve(
                &Url::parse(&format!("{}/github/owner/large", server.uri())).unwrap(),
                &host("linux")
            )
            .await,
        Err(FetchError::TooLarge { limit: 128 })
    ));
    Mock::given(path("/api/repos/owner/loop/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "/api/repos/owner/loop/releases/latest"),
        )
        .mount(&server)
        .await;
    assert!(matches!(
        engine(config(&server))
            .resolve(
                &Url::parse(&format!("{}/github/owner/loop", server.uri())).unwrap(),
                &host("linux")
            )
            .await,
        Err(FetchError::RedirectLimit)
    ));
    release(&server,"/api/repos/owner/bad-hash/releases/latest",json!({"assets":[{"name":"game-linux-x64.zip","browser_download_url":format!("{}/file",server.uri()),"size":7,"digest":"sha256:not-hex"}]})).await;
    assert!(matches!(
        engine(config(&server))
            .resolve(
                &Url::parse(&format!("{}/github/owner/bad-hash", server.uri())).unwrap(),
                &host("linux")
            )
            .await,
        Err(FetchError::InvalidResponse { .. })
    ));
}

#[tokio::test]
async fn foreign_assets_and_https_downgrade_links_are_rejected() {
    let server = MockServer::start().await;
    release(&server, "/api/repos/owner/foreign/releases/latest", json!({"assets":[{
        "name":"game-windows-x64.exe", "browser_download_url":format!("{}/asset", server.uri()), "size":7
    }]})).await;
    let cfg = config(&server);
    assert!(matches!(
        engine(cfg.clone())
            .resolve(
                &Url::parse(&format!("{}/github/owner/foreign", server.uri())).unwrap(),
                &host("linux")
            )
            .await,
        Err(FetchError::InvalidResponse { .. })
    ));
    release(&server, "/api/repos/owner/downgrade/releases/latest", json!({"assets":[{
        "name":"game-linux-x64.zip", "browser_download_url":format!("{}/asset", server.uri()), "size":7
    }]})).await;
    let mut cfg = cfg;
    cfg.github_web = Url::parse("https://github.example/").unwrap();
    assert!(matches!(
        engine(cfg.clone())
            .resolve(
                &Url::parse("https://github.example/owner/downgrade").unwrap(),
                &host("linux")
            )
            .await,
        Err(FetchError::InsecureRedirect)
    ));
    release(&server, "/api/repos/owner/sidecar/releases/latest", json!({"assets":[{
        "name":"game-linux-x64.zip", "browser_download_url":"https://assets.example/game.zip", "size":7
    }, {"name":"game-linux-x64.zip.sha256", "browser_download_url":format!("{}/checksum", server.uri()), "size":64}]})).await;
    assert!(matches!(
        engine(cfg)
            .resolve(
                &Url::parse("https://github.example/owner/sidecar").unwrap(),
                &host("linux")
            )
            .await,
        Err(FetchError::InsecureRedirect)
    ));
    assert!(
        !server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|request| request.url.path() == "/asset" || request.url.path() == "/checksum")
    );
}

#[tokio::test]
async fn drive_legacy_links_keep_resource_keys_and_reject_conflicting_ids() {
    for (link, valid) in [
        ("/content/download?id=legacy&amp;confirm=token", true),
        (
            "/content/download?id=legacy&amp;id=other&amp;confirm=token",
            false,
        ),
        (
            "https://evil.invalid/download?id=legacy&amp;confirm=token",
            false,
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/drive/uc"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                format!("<a id='uc-download-link' href='{link}'>Download</a>"),
                "text/html",
            ))
            .mount(&server)
            .await;
        let result = engine(config(&server))
            .resolve(
                &Url::parse(&format!(
                    "{}/drive/file/d/legacy/view?resourcekey=key",
                    server.uri()
                ))
                .unwrap(),
                &host("linux"),
            )
            .await;
        if valid {
            let request = one(result.unwrap());
            assert!(
                request
                    .url
                    .query_pairs()
                    .any(|(key, value)| key == "confirm" && value == "token")
            );
            assert!(
                request
                    .url
                    .query_pairs()
                    .any(|(key, value)| key == "resourcekey" && value == "key")
            );
        } else {
            assert!(matches!(result, Err(FetchError::InvalidResponse { .. })));
        }
    }
}

#[cfg(feature = "cli")]
#[tokio::test]
async fn a_recorded_github_release_installs_the_host_asset_through_the_real_cli() {
    let server = MockServer::start().await;
    let actual = caskade_installer::host::HostCaps::detect();
    let (input, asset_name) = match actual.os {
        caskade_installer::OperatingSystem::Windows => (
            "bin/unity.zip",
            if actual.arch == "aarch64" {
                "7z2605-arm64.exe"
            } else {
                "7z2605-x64.exe"
            },
        ),
        caskade_installer::OperatingSystem::Linux => (
            "bin/linux.tar.xz",
            if actual.arch == "aarch64" {
                "7z2605-linux-arm64.tar.xz"
            } else {
                "7z2605-linux-x64.tar.xz"
            },
        ),
        caskade_installer::OperatingSystem::Macos => ("bin/macos-app.zip", "7z2605-mac.tar.xz"),
    };
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(input);
    let bytes = fs::read(fixture).unwrap();
    let hash = digest(&bytes);
    let mut value = recorded(&server);
    value["assets"].as_array_mut().unwrap().retain(|asset| {
        matches!(
            asset["name"].as_str().unwrap(),
            "7z2605-x64.exe"
                | "7z2605-arm64.exe"
                | "7z2605-linux-x64.tar.xz"
                | "7z2605-linux-arm64.tar.xz"
                | "7z2605-mac.tar.xz"
        )
    });
    for asset in value["assets"].as_array_mut().unwrap() {
        asset["size"] = bytes.len().into();
        asset["digest"] = format!("sha256:{hash}").into();
    }
    release(&server, "/api/repos/ip7z/7zip/releases/latest", value).await;
    Mock::given(path(format!("/assets/{asset_name}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
        .expect(1)
        .mount(&server)
        .await;
    let scratch = tempfile::tempdir().unwrap();
    let cfg_path = scratch.path().join("resolvers.toml");
    fs::write(&cfg_path, toml::to_string(&config(&server)).unwrap()).unwrap();
    let root = TestRoot::new().unwrap();
    let root_path = root.path().to_owned();
    let source = format!("{}/github/ip7z/7zip/releases/latest", server.uri());
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .env("CASKADE_RESOLVERS_CONFIG", cfg_path)
            .args([
                "--json",
                "install",
                &source,
                "--extract",
                "--root",
                root_path.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(outcome["type"], "installed");
    let game = root.path().join(outcome["value"]["dir"].as_str().unwrap());
    let manifest = InstallManifest::read(&game).unwrap();
    assert!(manifest.verify(&game).unwrap().changed.is_empty());
    assert_eq!(
        manifest.record().source.checksum.as_ref().unwrap().as_str(),
        hash
    );
    assert!(
        manifest
            .record()
            .source
            .final_url
            .as_ref()
            .unwrap()
            .path()
            .ends_with(asset_name)
    );
    let game_clone = game.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .args(["--json", "uninstall", game_clone.to_str().unwrap()])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(output.status.success());
    assert!(!game.exists());
    fs::remove_file(root.path().join(ROOT_MANIFEST_PATH)).unwrap();
    fs::remove_dir(root.path().join(".caskade")).unwrap();
    root.assert_clean();
}
