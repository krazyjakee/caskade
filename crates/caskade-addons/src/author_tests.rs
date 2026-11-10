use super::author::{self, permission_words};
use super::{AddonKind, package, parse};
use serde_json::{Value, json};
use std::path::Path;
use url::Url;

fn write(root: &Path, files: &[(&str, &[u8])]) {
    for (path, bytes) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}

fn plugin_folder(root: &Path, version: &str) {
    let manifest = json!({
        "id": "dev.example.stats", "name": "Stats", "version": version, "api": "^0.1",
        "description": "Charts.", "author": "Example", "ui": "dist/ui.js",
        "permissions": {"library": "read", "network": ["stats.example"], "process": true}
    });
    write(
        root,
        &[
            ("plugin.json", manifest.to_string().as_bytes()),
            ("dist/ui.js", b"export default {}"),
            ("dist/.DS_Store", b"junk"),
            ("src/index.tsx", b"source stays out"),
            ("README.md", b"# Stats"),
        ],
    );
}

fn theme_folder(root: &Path) {
    write(
        root,
        &[
            (
                "theme.json",
                br#"{"id": "dev.example.ember", "name": "Ember", "version": "1.0.0", "author": "Example", "extends": "caskade.default"}"#,
            ),
            ("tokens.json", br##"{"color": {"accent": {"$root": {"$value": "#c2410c"}}}}"##),
        ],
    );
}

#[test]
fn a_package_holds_only_what_ships_and_packs_the_same_every_time() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("stats");
    plugin_folder(&folder, "0.1.0");
    let packed = author::pack(&folder, AddonKind::Plugin).unwrap();
    assert_eq!(
        (
            packed.id.as_str(),
            packed.version.as_str(),
            packed.api.as_deref()
        ),
        ("dev.example.stats", "0.1.0", Some("^0.1"))
    );
    assert_eq!(
        packed.permissions,
        ["library.read", "network:stats.example", "process"]
    );
    // Touching the files changes nothing.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(folder.join("README.md"), b"# Stats").unwrap();
    let again = author::pack(&folder, AddonKind::Plugin).unwrap();
    assert_eq!(packed.sha256(), again.sha256());

    let mut names: Vec<String> = {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&packed.archive)).unwrap();
        (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned())
            .collect()
    };
    names.sort();
    assert_eq!(
        names,
        [
            "dev.example.stats/README.md",
            "dev.example.stats/dist/ui.js",
            "dev.example.stats/plugin.json"
        ]
    );

    // It unpacks the way an install does, as the release it is.
    let archive = temp.path().join("stats.zip");
    std::fs::write(&archive, &packed.archive).unwrap();
    let root = package::extract(&archive, &temp.path().join("staging")).unwrap();
    assert_eq!(
        package::identity(AddonKind::Plugin, &root).unwrap(),
        ("dev.example.stats".to_owned(), "0.1.0".to_owned())
    );
}

#[test]
fn credentials_unbuilt_plugins_and_links_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("stats");
    plugin_folder(&folder, "0.1.0");
    write(&folder, &[("dist/.env", b"TOKEN=1")]);
    assert!(
        author::pack(&folder, AddonKind::Plugin)
            .unwrap_err()
            .contains("credentials")
    );
    std::fs::remove_file(folder.join("dist/.env")).unwrap();

    write(&folder, &[("assets/signing.key", b"x")]);
    assert!(
        author::pack(&folder, AddonKind::Plugin)
            .unwrap_err()
            .contains("credentials")
    );
    std::fs::remove_file(folder.join("assets/signing.key")).unwrap();

    write(
        &folder,
        &[(
            "dist/config.js",
            b"const key = `untrusted comment: minisign encrypted secret key`",
        )],
    );
    assert!(
        author::pack(&folder, AddonKind::Plugin)
            .unwrap_err()
            .contains("minisign encrypted secret key")
    );
    std::fs::remove_file(folder.join("dist/config.js")).unwrap();

    std::fs::remove_file(folder.join("dist/ui.js")).unwrap();
    assert!(
        author::pack(&folder, AddonKind::Plugin)
            .unwrap_err()
            .contains("build the plugin first")
    );

    #[cfg(unix)]
    {
        write(&folder, &[("dist/ui.js", b"export default {}")]);
        std::os::unix::fs::symlink("/etc/hostname", folder.join("dist/link.js")).unwrap();
        assert!(
            author::pack(&folder, AddonKind::Plugin)
                .unwrap_err()
                .contains("is a link")
        );
    }
}

#[test]
fn a_plugin_providing_metadata_is_refused_and_other_providers_are_declared() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("stats");
    plugin_folder(&folder, "0.1.0");
    let with = |providers: Value| {
        let mut manifest: Value =
            serde_json::from_slice(&std::fs::read(folder.join("plugin.json")).unwrap()).unwrap();
        manifest["contributes"] = json!({"providers": providers});
        write(&folder, &[("plugin.json", manifest.to_string().as_bytes())]);
    };
    with(json!(["library-source", "metadata-provider"]));
    let refused = author::pack(&folder, AddonKind::Plugin).unwrap_err();
    assert!(refused.contains("metadata-provider"), "{refused}");
    with(json!(["library-source"]));
    let packed = author::pack(&folder, AddonKind::Plugin).unwrap();
    assert_eq!(packed.provides, ["library-source"]);
}

#[test]
fn permission_words_follow_the_manifest() {
    assert_eq!(
        permission_words(&json!({"permissions": {
            "library": "write", "fs": ["/usr/bin:ro"], "network": []
        }})),
        ["library.write", "fs:/usr/bin:ro"]
    );
    assert!(permission_words(&json!({})).is_empty());
}

#[test]
fn a_built_catalogue_reads_as_valid_and_keeps_earlier_releases() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    plugin_folder(&src.join("stats"), "0.1.0");
    theme_folder(&src.join("ember"));
    write(&src, &[("media/stats.png", b"png bytes")]);
    let source = src.join("catalogue.json");
    std::fs::write(
        &source,
        json!({
            "format": 1, "generated": "2026-10-30T00:00:00Z",
            "packages": [
                {"path": "stats", "kind": "plugin", "summary": "Charts of your play time.",
                 "license": "MIT", "homepage": "https://example.org/stats",
                 "screenshots": [{"file": "media/stats.png", "alt": "The chart"}],
                 "notes": "First.", "published": "2026-10-30"},
                {"path": "ember", "kind": "theme", "summary": "Warm accents.", "license": "MIT",
                 "os": [], "modes": ["desktop", "console"]}
            ]
        })
        .to_string(),
    )
    .unwrap();
    let out = temp.path().join("out");
    let built = author::build(&source, &out, None).unwrap();
    assert_eq!(built.packages.len(), 2);
    let bytes = std::fs::read(out.join("catalogue.json")).unwrap();
    let catalogue = parse(
        &bytes,
        &Url::parse("https://example.org/catalogue.json").unwrap(),
    )
    .unwrap();
    assert!(catalogue.problems.is_empty(), "{:?}", catalogue.problems);
    let stats = catalogue.entry("dev.example.stats").unwrap();
    assert_eq!(stats.description, "Charts.");
    assert_eq!(stats.releases[0].permissions.len(), 3);
    assert_eq!(
        stats.screenshots[0].url.as_str(),
        "https://example.org/dev.example.stats-1.png"
    );
    let archive = out.join("dev.example.stats-0.1.0.zip");
    package::verify(&archive, &stats.releases[0]).unwrap();
    assert!(out.join("dev.example.stats-1.png").exists());
    let ember = catalogue.entry("dev.example.ember").unwrap();
    assert!(ember.releases[0].api.is_none());

    // The next version keeps 0.1.0's record.
    let previous = temp.path().join("published.json");
    std::fs::copy(out.join("catalogue.json"), &previous).unwrap();
    plugin_folder(&src.join("stats"), "0.2.0");
    let built = author::build(&source, &out, Some(&previous)).unwrap();
    assert_eq!(built.carried, 1);
    let doc: Value =
        serde_json::from_slice(&std::fs::read(out.join("catalogue.json")).unwrap()).unwrap();
    let versions: Vec<&str> = doc["packages"][0]["releases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|release| release["version"].as_str().unwrap())
        .collect();
    assert_eq!(versions, ["0.2.0", "0.1.0"]);

    // Republishing a version with other bytes is refused.
    std::fs::copy(out.join("catalogue.json"), &previous).unwrap();
    write(
        &src.join("stats"),
        &[("dist/ui.js", b"export default {changed: 1}")],
    );
    assert!(
        author::build(&source, &out, Some(&previous))
            .unwrap_err()
            .contains("raise its version")
    );
}

#[test]
fn utc_times_are_iso_8601() {
    assert_eq!(author::utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(author::utc(1_793_318_400), "2026-10-30T00:00:00Z");
    assert_eq!(author::utc(951_782_400 + 3_661), "2000-02-29T01:01:01Z");
}

#[tokio::test]
async fn check_reads_a_staged_catalogue_as_the_app_does_and_finds_broken_assets() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    plugin_folder(&src.join("stats"), "0.1.0");
    theme_folder(&src.join("ember"));
    let mut png = Vec::new();
    image::RgbImage::new(4, 3)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    write(
        &src,
        &[
            ("media/stats.png", &png),
            ("media/broken.png", b"not an image"),
        ],
    );
    let source = src.join("catalogue.json");
    let spec = |shot: &str| {
        json!({
            "format": 1, "generated": "2026-10-30T00:00:00Z",
            "packages": [
                {"path": "stats", "kind": "plugin", "summary": "Charts.", "license": "MIT",
                 "screenshots": [{"file": shot, "alt": "The chart"}]},
                {"path": "ember", "kind": "theme", "summary": "Warm.", "license": "MIT"}
            ]
        })
        .to_string()
    };
    std::fs::write(&source, spec("media/stats.png")).unwrap();
    let out = temp.path().join("out");
    author::build(&source, &out, None).unwrap();
    let location = out.join("catalogue.json");
    let location = location.to_str().unwrap();

    let checked = author::check(location, None, &temp.path().join("work1"))
        .await
        .unwrap();
    assert!(checked.problems.is_empty(), "{:?}", checked.problems);
    assert_eq!(checked.releases.len(), 2);
    assert!(checked.releases[0].root.join("plugin.json").is_file());
    assert!(checked.warnings.iter().any(|w| w.contains("signature")));
    assert!(
        checked
            .warnings
            .iter()
            .any(|w| w == "dev.example.ember: no screenshots")
    );

    // A changed archive, a missing one and a screenshot that isn't one.
    std::fs::write(&source, spec("media/broken.png")).unwrap();
    author::build(&source, &out, None).unwrap();
    let mut archive = std::fs::read(out.join("dev.example.stats-0.1.0.zip")).unwrap();
    *archive.last_mut().unwrap() ^= 1;
    std::fs::write(out.join("dev.example.stats-0.1.0.zip"), archive).unwrap();
    std::fs::remove_file(out.join("dev.example.ember-1.0.0.zip")).unwrap();
    let checked = author::check(location, None, &temp.path().join("work2"))
        .await
        .unwrap();
    assert!(checked.releases.is_empty());
    let problems = checked.problems.join("\n");
    assert!(
        problems.contains("dev.example.stats 0.1.0: the download's checksum"),
        "{problems}"
    );
    assert!(problems.contains("dev.example.ember 1.0.0: "), "{problems}");
    assert!(problems.contains("screenshot 1 isn't a PNG"), "{problems}");

    // A signature is required once a key is.
    assert!(
        author::check(location, Some("RWQ"), &temp.path().join("work3"))
            .await
            .is_err()
    );
}

#[test]
fn build_refuses_what_the_catalogue_would_leave_out() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    plugin_folder(&src.join("stats"), "0.1.0");
    theme_folder(&src.join("ember"));
    let source = src.join("catalogue.json");
    let build = |packages: Value| {
        std::fs::write(
            &source,
            json!({"format": 1, "generated": "2026-10-30T00:00:00Z", "packages": packages})
                .to_string(),
        )
        .unwrap();
        author::build(&source, &temp.path().join("out"), None)
    };
    let stats = |extra: Value| {
        let mut record = json!({"path": "stats", "kind": "plugin", "summary": "Charts.",
            "license": "MIT"});
        if let (Some(record), Some(extra)) = (record.as_object_mut(), extra.as_object()) {
            record.extend(extra.clone());
        }
        record
    };
    build(json!([stats(json!({}))])).unwrap();
    let refused = |packages: Value, reason: &str| {
        let error = build(packages).unwrap_err();
        assert!(error.contains(reason), "{reason}: {error}");
    };
    refused(json!([stats(json!({})), stats(json!({}))]), "listed twice");
    refused(
        json!([stats(json!({"os": ["linux", "beos"]}))]),
        "unknown OS",
    );
    refused(json!([stats(json!({"modes": ["tv"]}))]), "unknown mode");
    refused(
        json!([stats(json!({"license": "All rights reserved"}))]),
        "isn't an SPDX license expression",
    );
    refused(json!([stats(json!({"license": ""}))]), "“license” is empty");
    // The manifest gives the author and the version.
    let manifest = src.join("ember/theme.json");
    std::fs::write(
        &manifest,
        br#"{"id": "dev.example.ember", "name": "Ember", "version": "1.0.0", "extends": "caskade.default"}"#,
    )
    .unwrap();
    let ember = json!({"path": "ember", "kind": "theme", "summary": "Warm.", "license": "MIT"});
    refused(json!([ember.clone()]), "“author” is empty");
    std::fs::write(
        &manifest,
        br#"{"id": "dev.example.ember", "name": "Ember", "version": "1", "author": "Example"}"#,
    )
    .unwrap();
    refused(json!([ember]), "isn't a semantic version");
}

#[cfg(feature = "net")]
#[tokio::test]
async fn check_fetches_every_asset_and_names_what_is_unreachable_or_not_the_release() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    async fn serve(server: &MockServer, route: &str, body: Vec<u8>) {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(server)
            .await;
    }
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    plugin_folder(&src.join("stats"), "0.1.0");
    theme_folder(&src.join("ember"));
    let mut png = Vec::new();
    image::RgbImage::new(4, 3)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    write(&src, &[("media/stats.png", &png)]);
    let source = src.join("catalogue.json");
    std::fs::write(
        &source,
        json!({
            "format": 1, "generated": "2026-10-30T00:00:00Z",
            "packages": [
                {"path": "stats", "kind": "plugin", "summary": "Charts.", "license": "MIT",
                 "screenshots": [{"file": "media/stats.png", "alt": "The chart"}]},
                {"path": "ember", "kind": "theme", "summary": "Warm.", "license": "MIT"}
            ]
        })
        .to_string(),
    )
    .unwrap();
    let out = temp.path().join("out");
    author::build(&source, &out, None).unwrap();
    let file = |name: &str| std::fs::read(out.join(name)).unwrap();
    let stats_zip = "/dev.example.stats-0.1.0.zip";
    let ember_zip = "/dev.example.ember-1.0.0.zip";
    let shot = "/dev.example.stats-1.png";

    // Everything served: no problems.
    let server = MockServer::start().await;
    serve(&server, "/catalogue.json", file("catalogue.json")).await;
    serve(&server, stats_zip, file(&stats_zip[1..])).await;
    serve(&server, ember_zip, file(&ember_zip[1..])).await;
    serve(&server, shot, file(&shot[1..])).await;
    let origin = format!("{}/catalogue.json", server.uri());
    let checked = author::check(&origin, None, &temp.path().join("work1"))
        .await
        .unwrap();
    assert!(checked.problems.is_empty(), "{:?}", checked.problems);
    assert_eq!(checked.releases.len(), 2);
    let asked: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| request.url.path().to_owned())
        .collect();
    for route in [stats_zip, ember_zip, shot] {
        assert!(asked.iter().any(|path| path == route), "{route}: {asked:?}");
    }

    // The record claims a version the archive isn't, and the theme's
    // archive and the screenshot are gone (404).
    server.reset().await;
    let mut catalogue: Value = serde_json::from_slice(&file("catalogue.json")).unwrap();
    catalogue["packages"][0]["releases"][0]["version"] = json!("0.1.1");
    serve(
        &server,
        "/catalogue.json",
        catalogue.to_string().into_bytes(),
    )
    .await;
    serve(&server, stats_zip, file(&stats_zip[1..])).await;
    let checked = author::check(&origin, None, &temp.path().join("work2"))
        .await
        .unwrap();
    assert!(checked.releases.is_empty());
    let problems = checked.problems.join("\n");
    assert!(
        problems.contains(
            "dev.example.stats 0.1.1: the package is dev.example.stats 0.1.0, not dev.example.stats 0.1.1"
        ),
        "{problems}"
    );
    assert!(
        problems.contains("dev.example.ember 1.0.0: the server answered 404"),
        "{problems}"
    );
    assert!(
        problems.contains("dev.example.stats: screenshot 1: the server answered 404"),
        "{problems}"
    );
}
