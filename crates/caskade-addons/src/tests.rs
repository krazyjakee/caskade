//! The add-on catalogue (#181): bounded validation, compatibility,
//! signatures, the offline snapshot and refreshes from a local server.
use super::*;
use url::Url;

const CATALOGUE: &[u8] = include_bytes!("testdata/catalogue.json");
const SIGNATURE: &[u8] = include_bytes!("testdata/catalogue.json.sig");
const PUBLIC_KEY: &str = include_str!("testdata/test-signing.key.pub");

fn base() -> Url {
    Url::parse("https://example.org/addons/catalogue.json").unwrap()
}

fn host(mode: AppMode) -> HostFacts {
    HostFacts {
        plugin_api: |req| req.matches(&semver::Version::new(0, 1, 1)),
        plugin_api_version: "0.1.1".into(),
        os: AddonOs::Linux,
        mode,
    }
}

#[test]
fn valid_records_stay_and_bad_ones_are_left_out_with_reasons() {
    let catalogue = parse(CATALOGUE, &base()).unwrap();
    let ids: Vec<_> = catalogue.entries.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["dev.caskade.playtime-stats", "caskade.paper"]);
    let stats = catalogue.entry("dev.caskade.playtime-stats").unwrap();
    // Newest first; the unreadable release is left out.
    let versions: Vec<_> = stats.releases.iter().map(|r| r.version.as_str()).collect();
    assert_eq!(versions, ["0.2.0", "0.1.0"]);
    assert_eq!(
        stats.releases[1].artifact.url,
        "https://example.org/addons/packages/stats-0.1.0.zip"
    );
    assert_eq!(
        stats.screenshots[0].url,
        "https://example.org/addons/media/stats.png"
    );
    // Markup is kept as text; nothing interprets it.
    assert!(stats.description.contains("<script>alert(1)</script>"));
    // The first of two records with one id stays.
    assert_eq!(catalogue.entry("caskade.paper").unwrap().name, "Paper");
    let problems: Vec<_> = catalogue
        .problems
        .iter()
        .map(|p| format!("{:?}: {}", p.id, p.message))
        .collect();
    let has = |text: &str| problems.iter().any(|p| p.contains(text));
    assert!(has("no valid id"), "{problems:?}");
    assert!(has("listed twice"), "{problems:?}");
    assert!(has("provides metadata"), "{problems:?}");
    assert!(has("isn't an HTTPS address"), "{problems:?}");
    assert!(has("isn't an object"), "{problems:?}");
    assert!(has("plugin API range"), "{problems:?}");
}

#[test]
fn documents_that_arent_catalogues_are_refused_whole() {
    let big = vec![b' '; MAX_CATALOGUE_BYTES + 1];
    assert!(parse(&big, &base()).is_err());
    assert!(parse(b"<html>", &base()).is_err());
    assert!(parse(br#"{"format": 2, "packages": []}"#, &base()).is_err());
    assert!(parse(br#"{"format": 1}"#, &base()).is_err());
    let empty = parse(br#"{"format": 1, "packages": []}"#, &base()).unwrap();
    assert!(empty.entries.is_empty());
}

fn package(id: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut record = serde_json::json!({
        "id": id, "kind": "plugin", "name": "N", "author": "A", "summary": "S", "license": "MIT",
        "releases": [{"version": "1.0.0", "api": "^0.1",
            "artifact": {"url": "p.zip", "sha256": "ab".repeat(32), "size": 10}}]
    });
    if let (Some(record), Some(extra)) = (record.as_object_mut(), extra.as_object()) {
        record.extend(extra.clone());
    }
    record
}

#[test]
fn metadata_sources_are_never_offered() {
    let category = package(
        "dev.example.meta",
        serde_json::json!({"categories": ["Metadata"]}),
    );
    let provider = package(
        "dev.example.provider",
        serde_json::json!({"releases": [{"version": "1.0.0", "api": "^0.1",
            "provides": ["metadata-provider"],
            "artifact": {"url": "p.zip", "sha256": "ab".repeat(32), "size": 10}}]}),
    );
    let doc = serde_json::json!({"format": 1, "packages": [category, provider,
        package("dev.example.ok", serde_json::json!({}))]})
    .to_string();
    let catalogue = parse(doc.as_bytes(), &base()).unwrap();
    assert!(catalogue.entry("dev.example.meta").is_none());
    assert!(
        catalogue
            .entry("dev.example.provider")
            .is_none_or(|entry| entry.releases.is_empty()),
        "{:?}",
        catalogue.entries
    );
    assert!(catalogue.entry("dev.example.ok").is_some());
    let problems: Vec<_> = catalogue
        .problems
        .iter()
        .map(|p| p.message.as_str())
        .collect();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("metadata sources aren't offered")),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|p| p.contains("provides metadata")),
        "{problems:?}"
    );
}

#[test]
fn ids_versions_declarations_artifacts_and_attribution_are_checked() {
    use serde_json::json;
    let release = |extra: serde_json::Value| {
        let mut release = json!({"version": "1.0.0", "api": "^0.1",
            "artifact": {"url": "p.zip", "sha256": "ab".repeat(32), "size": 10}});
        if let (Some(release), Some(extra)) = (release.as_object_mut(), extra.as_object()) {
            release.extend(extra.clone());
        }
        release
    };
    let one =
        |id: &str, extra: serde_json::Value| package(id, json!({"releases": [release(extra)]}));
    let refused = [
        (
            one("dev.example.version", json!({"version": "1.0"})),
            "isn't a semantic version",
        ),
        (
            one("dev.example.api", json!({"api": "about 0.1"})),
            "plugin API range",
        ),
        (
            one("dev.example.os", json!({"os": ["linux", "beos"]})),
            "unknown OS",
        ),
        (
            one("dev.example.mode", json!({"modes": ["desktop", "tv"]})),
            "unknown mode",
        ),
        (
            one(
                "dev.example.sha",
                json!({"artifact": {"url": "p.zip", "sha256": "xyz", "size": 10}}),
            ),
            "64 hex digits",
        ),
        (
            one(
                "dev.example.size",
                json!({"artifact": {"url": "p.zip", "sha256": "ab".repeat(32), "size": 0}}),
            ),
            "size is missing",
        ),
        (
            one(
                "dev.example.url",
                json!({"artifact": {"sha256": "ab".repeat(32), "size": 10}}),
            ),
            "“url” is missing",
        ),
        (
            package(
                "dev.example.free-text",
                json!({"license": "All rights reserved"}),
            ),
            "isn't an SPDX",
        ),
        (
            package("dev.example.noassertion", json!({"license": "NOASSERTION"})),
            "isn't an SPDX",
        ),
        (
            package("dev.example.no-license", json!({"license": " "})),
            "“license” is empty",
        ),
        (
            package("dev.example.unlicensed", json!({"license": null})),
            "“license” is missing",
        ),
        (
            package("dev.example.no-author", json!({"author": ""})),
            "“author” is empty",
        ),
        (
            package("dev.example.anonymous", json!({"author": null})),
            "“author” is missing",
        ),
    ];
    // One version twice, even with other build metadata: the first stays.
    let twice = package(
        "dev.example.twice",
        json!({"releases": [release(json!({})), release(json!({"version": "1.0.0+build.2"}))]}),
    );
    // The same id as a plugin and as a theme: the first stays.
    let theme = json!({"id": "dev.example.ok", "kind": "theme", "name": "T", "author": "A",
        "summary": "S", "license": "MIT",
        "releases": [{"version": "1.0.0", "artifact": {"url": "t.zip", "sha256": "ab".repeat(32), "size": 10}}]});
    let mut packages: Vec<_> = refused.iter().map(|(record, _)| record.clone()).collect();
    packages.extend([
        twice,
        package(
            "dev.example.ok",
            json!({"license": "(MIT OR Apache-2.0) AND CC-BY-4.0"}),
        ),
        theme,
    ]);
    let doc = json!({"format": 1, "packages": packages}).to_string();
    let catalogue = parse(doc.as_bytes(), &base()).unwrap();
    let problem = |id: &str, text: &str| {
        catalogue
            .problems
            .iter()
            .any(|p| p.id.as_deref() == Some(id) && p.message.contains(text))
    };
    for (record, reason) in &refused {
        let id = record["id"].as_str().unwrap();
        assert!(catalogue.entry(id).is_none(), "{id} was kept");
        assert!(problem(id, reason), "{id}: {:?}", catalogue.problems);
    }
    let twice = catalogue.entry("dev.example.twice").unwrap();
    assert_eq!(twice.releases.len(), 1);
    assert_eq!(twice.releases[0].version, "1.0.0");
    assert!(problem("dev.example.twice", "listed twice"));
    let ok = catalogue.entry("dev.example.ok").unwrap();
    assert_eq!(ok.kind, AddonKind::Plugin);
    assert!(problem("dev.example.ok", "the id is listed twice"));
    assert_eq!(catalogue.entries.len(), 2, "{:?}", catalogue.entries);
}

#[test]
fn licenses_are_spdx_expressions() {
    use super::validate::license_expression;
    for valid in [
        "MIT",
        "MIT OR Apache-2.0",
        "(MIT OR Apache-2.0) AND CC-BY-4.0",
        "GPL-2.0-or-later WITH Classpath-exception-2.0",
        "GPL-2.0+",
        "LicenseRef-Caskade-Art",
        "DocumentRef-assets:LicenseRef-Font",
        "mit or apache-2.0",
    ] {
        assert!(license_expression(valid), "{valid}");
    }
    for invalid in [
        "",
        "MIT OR",
        "OR MIT",
        "(MIT",
        "MIT)",
        "MIT AND AND Apache-2.0",
        "MIT, Apache-2.0",
        "MIT WITH",
        "see LICENSE",
        "NONE",
        "NOASSERTION",
        "Other:LicenseRef-x",
        "-MIT",
    ] {
        assert!(!license_expression(invalid), "{invalid}");
    }
}

#[test]
fn records_are_bounded() {
    let many: Vec<_> = (0..1005)
        .map(|i| package(&format!("dev.example.p{i}"), serde_json::json!({})))
        .collect();
    let doc = serde_json::json!({"format": 1, "packages": many}).to_string();
    let catalogue = parse(doc.as_bytes(), &base()).unwrap();
    assert_eq!(catalogue.entries.len(), 1000);
    assert!(catalogue.problems[0].message.contains("first 1000"));

    let long = package(
        "dev.example.long",
        serde_json::json!({"name": "x".repeat(81)}),
    );
    let control = package("dev.example.ctl", serde_json::json!({"summary": "a\u{7}b"}));
    let huge = package(
        "dev.example.huge",
        serde_json::json!({"releases": [{"version": "1.0.0", "api": "^0.1",
            "artifact": {"url": "p.zip", "sha256": "ab".repeat(32), "size": 1u64 << 40}}]}),
    );
    let theme_api = serde_json::json!({"id": "dev.example.theme", "kind": "theme", "name": "T",
        "author": "A", "summary": "S", "license": "MIT",
        "releases": [{"version": "1.0.0", "api": "^0.1",
            "artifact": {"url": "t.zip", "sha256": "ab".repeat(32), "size": 10}}]});
    let doc = serde_json::json!({"format": 1, "packages": [long, control, huge, theme_api,
        package("dev.example.ok", serde_json::json!({}))]})
    .to_string();
    let catalogue = parse(doc.as_bytes(), &base()).unwrap();
    let ids: Vec<_> = catalogue.entries.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["dev.example.ok"]);
    assert_eq!(catalogue.problems.len(), 6, "{:?}", catalogue.problems);
}

#[test]
fn compatibility_is_worked_out_before_anything_downloads() {
    let catalogue = parse(CATALOGUE, &base()).unwrap();
    let stats = catalogue.entry("dev.caskade.playtime-stats").unwrap();

    // Not installed: the newest release that fits is offered, and the
    // newer one's reason is not.
    let fresh = evaluate_entry(stats, &[], &host(AppMode::Desktop));
    assert_eq!(fresh.state, ItemState::Available);
    assert_eq!(fresh.release.as_ref().unwrap().version, "0.1.0");
    assert!(fresh.reasons.is_empty());

    // In console mode its desktop-only pages are a limit, not a refusal.
    let console = evaluate_entry(stats, &[], &host(AppMode::Console));
    assert_eq!(console.state, ItemState::Available);
    assert_eq!(console.reasons[0].code, ReasonCode::Mode);
    assert!(!console.reasons[0].blocking);

    let installed = |version: &str| InstalledAddon {
        id: stats.id.clone(),
        kind: AddonKind::Plugin,
        version: Some(version.into()),
    };
    let same = evaluate_entry(stats, &[installed("0.1.0")], &host(AppMode::Desktop));
    assert_eq!(
        (same.state, same.installed_version.as_deref()),
        (ItemState::Installed, Some("0.1.0"))
    );
    let older = evaluate_entry(stats, &[installed("0.0.9")], &host(AppMode::Desktop));
    assert_eq!(older.state, ItemState::UpdateAvailable);

    // No release fits this API: incompatible, with the newest one's reason.
    let api_only_9 = HostFacts {
        plugin_api: |req| req.matches(&semver::Version::new(0, 9, 0)),
        plugin_api_version: "0.9.0".into(),
        ..host(AppMode::Desktop)
    };
    let none = evaluate_entry(stats, &[], &api_only_9);
    assert_eq!(none.state, ItemState::Incompatible);
    assert_eq!(none.reasons[0].code, ReasonCode::Api);
    assert!(none.release.is_none());

    // The same id installed as a theme.
    let clash = InstalledAddon {
        kind: AddonKind::Theme,
        ..installed("0.1.0")
    };
    let clash = evaluate_entry(stats, &[clash], &host(AppMode::Desktop));
    assert_eq!(clash.state, ItemState::Incompatible);
    assert!(clash.reasons.iter().any(|r| r.code == ReasonCode::Kind));

    // An OS the release doesn't list.
    let mut windows_only = stats.clone();
    for release in &mut windows_only.releases {
        release.os = vec![AddonOs::Windows];
    }
    let other_os = evaluate_entry(&windows_only, &[], &host(AppMode::Desktop));
    assert_eq!(other_os.state, ItemState::Incompatible);
    assert!(other_os.reasons.iter().any(|r| r.code == ReasonCode::Os));
    assert_eq!(evaluate(&catalogue, &[], &host(AppMode::Desktop)).len(), 2);
}

#[tokio::test]
async fn only_a_signed_catalogue_replaces_the_last_good_one_and_it_stays_offline() {
    let dir = tempfile::tempdir().unwrap();
    let service = CatalogueService::new(dir.path(), base().as_str(), Some(PUBLIC_KEY)).unwrap();
    assert!(service.catalogue().entries.is_empty());
    assert_eq!(service.status().fetched_at_ms, None);

    let status = service
        .accept(CATALOGUE, Some(SIGNATURE), 1000)
        .await
        .unwrap();
    assert!(status.verified);
    assert_eq!(status.left_out, 8);
    assert_eq!(service.catalogue().entries.len(), 2);

    // Tampered, or unsigned: refused, and the last good one stays.
    let mut tampered = CATALOGUE.to_vec();
    let at = tampered.iter().position(|b| *b == b'P').unwrap();
    tampered[at] = b'Q';
    assert!(
        service
            .accept(&tampered, Some(SIGNATURE), 2000)
            .await
            .is_err()
    );
    assert!(service.accept(CATALOGUE, None, 2000).await.is_err());
    assert!(
        service
            .accept(CATALOGUE, Some(b"junk"), 2000)
            .await
            .is_err()
    );
    assert_eq!(service.catalogue().entries.len(), 2);
    assert_eq!(service.status().fetched_at_ms, Some(1000));

    // A restart reads the snapshot back for offline browsing.
    let again = CatalogueService::new(dir.path(), base().as_str(), Some(PUBLIC_KEY)).unwrap();
    assert_eq!(again.catalogue(), service.catalogue());
    assert!(again.status().verified);
    // Another origin's snapshot isn't used.
    let other = CatalogueService::new(dir.path(), "https://other.example/c.json", Some(PUBLIC_KEY))
        .unwrap();
    assert!(other.catalogue().entries.is_empty());
    // A damaged snapshot is set aside, and startup goes on.
    std::fs::write(dir.path().join("catalogue.snapshot.json"), b"{broken").unwrap();
    let damaged = CatalogueService::new(dir.path(), base().as_str(), Some(PUBLIC_KEY)).unwrap();
    assert!(damaged.catalogue().entries.is_empty());
}

/// Two versions of the catalogue, signed with a throwaway key.
struct Versions {
    public_key: String,
    old: (Vec<u8>, Vec<u8>),
    new: (Vec<u8>, Vec<u8>),
}

impl Versions {
    fn new() -> Self {
        let key = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let sign = |bytes: Vec<u8>| {
            let signature =
                minisign::sign(Some(&key.pk), &key.sk, bytes.as_slice(), None, None).unwrap();
            (bytes, signature.to_string().into_bytes())
        };
        let mut new: serde_json::Value = serde_json::from_slice(CATALOGUE).unwrap();
        new["generated"] = "2099-01-01T00:00:00Z".into();
        Self {
            public_key: key.pk.to_box().unwrap().to_string(),
            old: sign(CATALOGUE.to_vec()),
            new: sign(serde_json::to_vec(&new).unwrap()),
        }
    }

    fn open(&self, dir: &std::path::Path) -> CatalogueService {
        CatalogueService::new(dir, base().as_str(), Some(&self.public_key)).unwrap()
    }

    /// The old version as versions before the snapshot file saved it.
    fn lay_out_three_files(&self, dir: &std::path::Path) {
        let meta = serde_json::json!({ "origin": base(), "fetchedAtMs": 1000 });
        std::fs::write(dir.join("catalogue.json"), &self.old.0).unwrap();
        std::fs::write(dir.join("catalogue.json.sig"), &self.old.1).unwrap();
        std::fs::write(
            dir.join("catalogue.meta.json"),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();
    }
}

/// `accept` stopped before each of its disk changes in turn, as a crash or a
/// failing disk would, starting from a catalogue saved by this version or
/// laid out as older versions left it.
#[tokio::test]
async fn a_stop_anywhere_in_a_save_leaves_the_last_good_catalogue_or_the_new_one() {
    let versions = Versions::new();
    let (old, old_signature) = &versions.old;
    let (new, new_signature) = &versions.new;
    let mut failures = Vec::new();

    for three_files in [false, true] {
        let mut stops = 0;
        loop {
            let dir = tempfile::tempdir().unwrap();
            if three_files {
                versions.lay_out_three_files(dir.path());
            } else {
                versions
                    .open(dir.path())
                    .accept(old, Some(old_signature), 1000)
                    .await
                    .unwrap();
            }
            let running = versions.open(dir.path());
            assert_eq!(running.status().fetched_at_ms, Some(1000));
            let last_good = running.catalogue();

            running.stop_after(stops);
            let saved = running.accept(new, Some(new_signature), 2000).await;
            let reopened = versions.open(dir.path());
            if saved.is_ok() {
                assert_eq!(reopened.status().fetched_at_ms, Some(2000));
                assert_eq!(reopened.catalogue(), running.catalogue());
                assert!(reopened.status().verified);
                break;
            }
            // What was running keeps showing the last good one.
            assert_eq!(running.catalogue(), last_good);

            let status = reopened.status();
            let intact = match status.fetched_at_ms {
                Some(1000) => reopened.catalogue() == last_good,
                Some(2000) => {
                    reopened.catalogue().generated.as_deref() == Some("2099-01-01T00:00:00Z")
                }
                _ => false,
            };
            if !intact || !status.verified {
                failures.push(format!(
                    "three_files={three_files} stop={stops}: fetched {:?}, {} entries, generated {:?}",
                    status.fetched_at_ms,
                    reopened.catalogue().entries.len(),
                    reopened.catalogue().generated,
                ));
            }

            // The next fetch still lands, whatever the stop left behind.
            reopened
                .accept(new, Some(new_signature), 3000)
                .await
                .unwrap();
            assert_eq!(versions.open(dir.path()).status().fetched_at_ms, Some(3000));
            stops += 1;
        }
        assert!(stops > 1, "only {stops} stops were tried");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A cache from a version that kept three files is read, and its first save
/// clears them. What a stop left there on such a version can't be repaired,
/// since the signature that matched is gone: it waits for the next fetch.
#[tokio::test]
async fn a_catalogue_saved_in_three_files_still_loads() {
    let versions = Versions::new();
    let dir = tempfile::tempdir().unwrap();
    let (old, old_signature) = &versions.old;
    let (new, new_signature) = &versions.new;
    versions.lay_out_three_files(dir.path());

    let reopened = versions.open(dir.path());
    assert_eq!(reopened.status().fetched_at_ms, Some(1000));
    assert!(reopened.status().verified);
    assert_eq!(reopened.catalogue().entries.len(), 2);
    let other = CatalogueService::new(
        dir.path(),
        "https://other.example/c.json",
        Some(&versions.public_key),
    )
    .unwrap();
    assert!(other.catalogue().entries.is_empty());

    // Files a save stopped short of clearing don't outrank it.
    reopened
        .accept(new, Some(new_signature), 2000)
        .await
        .unwrap();
    for name in [
        "catalogue.json",
        "catalogue.json.sig",
        "catalogue.meta.json",
    ] {
        assert!(!dir.path().join(name).exists(), "{name}");
    }
    versions.lay_out_three_files(dir.path());
    assert_eq!(versions.open(dir.path()).status().fetched_at_ms, Some(2000));

    // A new signature beside the old catalogue, as a stop there left it.
    std::fs::remove_file(dir.path().join("catalogue.snapshot.json")).unwrap();
    std::fs::write(dir.path().join("catalogue.json.sig"), new_signature).unwrap();
    let torn = versions.open(dir.path());
    assert!(torn.catalogue().entries.is_empty());
    torn.accept(old, Some(old_signature), 3000).await.unwrap();
    assert_eq!(versions.open(dir.path()).status().fetched_at_ms, Some(3000));
}

/// A catalogue that can't be saved stays out, on disk and on screen.
#[tokio::test]
async fn a_failed_save_keeps_the_last_good_catalogue() {
    let versions = Versions::new();
    let dir = tempfile::tempdir().unwrap();
    let (old, old_signature) = &versions.old;
    let (new, new_signature) = &versions.new;
    let service = versions.open(dir.path());
    service
        .accept(old, Some(old_signature), 1000)
        .await
        .unwrap();

    std::fs::create_dir(dir.path().join("catalogue.snapshot.partial")).unwrap();
    assert!(
        service
            .accept(new, Some(new_signature), 2000)
            .await
            .is_err()
    );
    assert_eq!(service.status().fetched_at_ms, Some(1000));
    assert_eq!(versions.open(dir.path()).status().fetched_at_ms, Some(1000));
}

#[cfg(feature = "net")]
mod refresh {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    async fn serve(catalogue: &[u8], signature: &[u8]) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/addons/catalogue.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(catalogue.to_vec()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/addons/catalogue.json.sig"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(signature.to_vec()))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn a_refresh_fetches_verifies_and_keeps_the_last_good_one_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let server = serve(CATALOGUE, SIGNATURE).await;
        let origin = format!("{}/addons/catalogue.json", server.uri());
        let service = CatalogueService::new(dir.path(), &origin, Some(PUBLIC_KEY)).unwrap();
        let status = service.refresh().await;
        assert_eq!(status.error, None);
        assert!(status.verified && status.fetched_at_ms.is_some());
        // Relative addresses resolve against the local server.
        let stats = service.catalogue();
        assert!(
            stats.entries[0].releases[0]
                .artifact
                .url
                .starts_with(&server.uri())
        );
        // Only the catalogue and its signature were asked for, with nothing
        // about the user.
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|r| r.url.query().is_none() && r.body.is_empty())
        );

        // The server fails: the last good catalogue stays.
        server.reset().await;
        let status = service.refresh().await;
        assert!(status.error.is_some());
        assert!(!status.offline);
        assert_eq!(service.catalogue().entries.len(), 2);

        // Nothing listening: offline, and the snapshot still shows.
        let closed = tempfile::tempdir().unwrap();
        let offline = CatalogueService::new(
            closed.path(),
            "http://127.0.0.1:9/addons/catalogue.json",
            Some(PUBLIC_KEY),
        )
        .unwrap();
        offline.accept(CATALOGUE, Some(SIGNATURE), 5).await.unwrap();
        let status = offline.refresh().await;
        assert!(status.offline, "{status:?}");
        assert_eq!(offline.catalogue().entries.len(), 2);
    }

    #[tokio::test]
    async fn an_oversized_catalogue_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let big = vec![b' '; MAX_CATALOGUE_BYTES + 10];
        let server = serve(&big, SIGNATURE).await;
        let origin = format!("{}/addons/catalogue.json", server.uri());
        let service = CatalogueService::new(dir.path(), &origin, Some(PUBLIC_KEY)).unwrap();
        let status = service.refresh().await;
        assert!(status.error.unwrap().contains("larger"));
    }
}
