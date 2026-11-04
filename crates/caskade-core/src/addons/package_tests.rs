//! Catalogue packages (#182): integrity, safe unpacking, identity, the
//! journaled swap and crash recovery, downloads and the records.
use super::package::*;
use super::{AddonKind, CatalogueArtifact, CatalogueEntry, CatalogueRelease};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        for (name, bytes) in entries {
            if name.ends_with('/') {
                writer
                    .add_directory(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
            } else {
                writer
                    .start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(bytes).unwrap();
            }
        }
        writer.finish().unwrap();
    }
    cursor.into_inner()
}

fn release_for(bytes: &[u8], version: &str, url: &str) -> CatalogueRelease {
    CatalogueRelease {
        version: version.into(),
        api: Some("^0.1".into()),
        os: vec![],
        modes: vec![],
        permissions: vec![],
        notes: String::new(),
        published: None,
        artifact: CatalogueArtifact {
            url: url.into(),
            sha256: Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            size: bytes.len() as u64,
        },
    }
}

fn entry(id: &str) -> CatalogueEntry {
    CatalogueEntry {
        id: id.into(),
        kind: AddonKind::Plugin,
        name: "N".into(),
        author: "A".into(),
        summary: "S".into(),
        description: String::new(),
        license: "MIT".into(),
        homepage: None,
        categories: vec![],
        screenshots: vec![],
        releases: vec![],
        deprecated: None,
    }
}

const MANIFEST: &[u8] = br#"{"id": "dev.example.stats", "version": "1.2.0", "api": "^0.1"}"#;

#[test]
fn an_archive_must_match_the_catalogue() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = zip(&[("plugin.json", MANIFEST)]);
    let archive = dir.path().join("a.zip");
    std::fs::write(&archive, &bytes).unwrap();
    let release = release_for(&bytes, "1.2.0", "https://x/a.zip");
    verify(&archive, &release).unwrap();
    let mut wrong_size = release.clone();
    wrong_size.artifact.size += 1;
    assert_eq!(
        verify(&archive, &wrong_size).unwrap_err().kind,
        AddonFailure::Integrity
    );
    let mut wrong_hash = release.clone();
    wrong_hash.artifact.sha256 = "0".repeat(64);
    let error = verify(&archive, &wrong_hash).unwrap_err();
    assert_eq!(error.kind, AddonFailure::Integrity);
    assert!(!error.kind.transient());
}

#[test]
fn unpacking_refuses_unsafe_archives_and_finds_the_package_folder() {
    let dir = tempfile::tempdir().unwrap();
    let unpack = |name: &str, bytes: &[u8]| {
        let archive = dir.path().join(format!("{name}.zip"));
        std::fs::write(&archive, bytes).unwrap();
        extract(&archive, &dir.path().join(name))
    };
    // One top folder: that is the package.
    let root = unpack(
        "nested",
        &zip(&[
            ("stats/", b""),
            ("stats/plugin.json", MANIFEST),
            ("stats/dist/ui.js", b"x"),
        ]),
    )
    .unwrap();
    assert_eq!(root, dir.path().join("nested/stats"));
    assert_eq!(
        identity(AddonKind::Plugin, &root).unwrap(),
        ("dev.example.stats".to_owned(), "1.2.0".to_owned())
    );
    // Files at the top: the folder itself.
    let flat = unpack(
        "flat",
        &zip(&[("plugin.json", MANIFEST), ("dist/ui.js", b"x")]),
    )
    .unwrap();
    assert_eq!(flat, dir.path().join("flat"));

    for (name, bytes) in [
        ("traversal", zip(&[("../evil.txt", b"x")])),
        ("absolute", zip(&[("/tmp/evil.txt", b"x")])),
    ] {
        assert_eq!(
            unpack(name, &bytes).unwrap_err().kind,
            AddonFailure::Unsafe,
            "{name}"
        );
        assert!(!dir.path().join("evil.txt").exists());
    }
    assert_eq!(
        unpack("junk", b"not a zip").unwrap_err().kind,
        AddonFailure::Unsafe
    );

    // A symbolic link entry.
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        writer
            .add_symlink(
                "link",
                "/etc/passwd",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.finish().unwrap();
    }
    assert_eq!(
        unpack("symlink", &cursor.into_inner()).unwrap_err().kind,
        AddonFailure::Unsafe
    );
}

#[test]
fn the_package_must_be_the_release_it_claims() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("plugin.json"), MANIFEST).unwrap();
    let release = release_for(b"", "1.2.0", "https://x/a.zip");
    check_identity(&entry("dev.example.stats"), &release, dir.path()).unwrap();
    let other = check_identity(&entry("dev.example.other"), &release, dir.path()).unwrap_err();
    assert_eq!(other.kind, AddonFailure::Identity);
    let older = release_for(b"", "1.1.0", "https://x/a.zip");
    assert_eq!(
        check_identity(&entry("dev.example.stats"), &older, dir.path())
            .unwrap_err()
            .kind,
        AddonFailure::Identity
    );
    std::fs::remove_file(dir.path().join("plugin.json")).unwrap();
    assert_eq!(
        check_identity(&entry("dev.example.stats"), &release, dir.path())
            .unwrap_err()
            .kind,
        AddonFailure::Identity
    );
}

#[test]
fn a_package_providing_metadata_is_refused_whatever_its_record_says() {
    let dir = tempfile::tempdir().unwrap();
    let release = release_for(b"", "1.2.0", "https://x/a.zip");
    let manifest = |providers: &str| {
        format!(
            r#"{{"id": "dev.example.stats", "version": "1.2.0", "api": "^0.1",
                "contributes": {{"providers": {providers}}}}}"#
        )
    };
    std::fs::write(
        dir.path().join("plugin.json"),
        manifest(r#"["metadata-provider"]"#),
    )
    .unwrap();
    assert_eq!(
        check_identity(&entry("dev.example.stats"), &release, dir.path())
            .unwrap_err()
            .kind,
        AddonFailure::Incompatible
    );
    std::fs::write(
        dir.path().join("plugin.json"),
        manifest(r#"["library-source"]"#),
    )
    .unwrap();
    check_identity(&entry("dev.example.stats"), &release, dir.path()).unwrap();
}

fn folder(path: &Path, marker: &str) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(path.join("marker"), marker).unwrap();
}

fn marker(path: &Path) -> String {
    std::fs::read_to_string(path.join("marker")).unwrap()
}

#[test]
fn a_swap_commits_rolls_back_and_survives_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("addons");
    let target = dir.path().join("plugins/dev.example.stats");
    folder(&target, "old");

    folder(&dir.path().join("new"), "new");
    let swap = swap_in(&dir.path().join("new"), &target, &work).unwrap();
    assert_eq!(marker(&target), "new");
    swap.rollback().unwrap();
    assert_eq!(marker(&target), "old");
    assert!(!work.join("journal.json").exists());

    folder(&dir.path().join("new"), "new");
    swap_in(&dir.path().join("new"), &target, &work)
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(marker(&target), "new");
    assert!(
        !dir.path()
            .join("plugins/.dev.example.stats.previous")
            .exists()
    );

    // The app stops between the swap and its confirmation: the next start
    // puts the previous version back and clears staging.
    folder(&dir.path().join("newer"), "newer");
    folder(&work.join("staging/job"), "half");
    let swap = swap_in(&dir.path().join("newer"), &target, &work).unwrap();
    std::mem::forget(swap);
    assert!(recover(&work).unwrap());
    assert_eq!(marker(&target), "new");
    assert!(!work.join("staging").exists());
    assert!(!recover(&work).unwrap());

    // A first install rolled back leaves nothing.
    let fresh = dir.path().join("plugins/dev.example.fresh");
    folder(&dir.path().join("first"), "first");
    swap_in(&dir.path().join("first"), &fresh, &work)
        .unwrap()
        .rollback()
        .unwrap();
    assert!(!fresh.exists());
}

#[tokio::test]
async fn records_persist_in_the_library() {
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    assert_eq!(
        AddonRecords::load(&store).await.unwrap(),
        AddonRecords::default()
    );
    let mut records = AddonRecords {
        updates: UpdatePreference::Manual,
        ..Default::default()
    };
    records.addons.insert(
        "dev.example.stats".into(),
        AddonRecord {
            version: Some("1.2.0".into()),
            skipped: Some("1.3.0".into()),
            rejected: Some(RejectedRelease {
                version: "1.4.0".into(),
                kind: AddonFailure::Integrity,
                message: "checksum".into(),
            }),
        },
    );
    records.save(&store).await.unwrap();
    assert_eq!(AddonRecords::load(&store).await.unwrap(), records);
}

#[cfg(feature = "net")]
mod download {
    use super::*;
    use tokio_util::sync::CancellationToken;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    async fn serve(bytes: &[u8]) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/stats.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.to_vec()))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn downloads_are_checked_bounded_cached_and_cancellable() {
        let bytes = zip(&[("plugin.json", MANIFEST)]);
        let server = serve(&bytes).await;
        let url = format!("{}/stats.zip", server.uri());
        let dir = tempfile::tempdir().unwrap();
        let release = release_for(&bytes, "1.2.0", &url);
        let mut seen = 0;
        let archive = download(&release, dir.path(), &CancellationToken::new(), |n| {
            seen = n
        })
        .await
        .unwrap();
        assert_eq!(seen, bytes.len() as u64);
        verify(&archive, &release).unwrap();
        // Downloaded once: the archive is reused.
        download(&release, dir.path(), &CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert_eq!(server.received_requests().await.unwrap().len(), 1);

        // A different checksum: refused, nothing kept.
        let other = tempfile::tempdir().unwrap();
        let mut wrong = release.clone();
        wrong.artifact.sha256 = "1".repeat(64);
        let error = download(&wrong, other.path(), &CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert_eq!(error.kind, AddonFailure::Integrity);
        assert_eq!(std::fs::read_dir(other.path()).unwrap().count(), 0);
        // Larger than the catalogue says.
        let mut small = release.clone();
        small.artifact.size = 4;
        let error = download(&small, other.path(), &CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert_eq!(error.kind, AddonFailure::Integrity);
        // Cancelled before it starts.
        let cancel = CancellationToken::new();
        cancel.cancel();
        let fresh = tempfile::tempdir().unwrap();
        let error = download(&release, fresh.path(), &cancel, |_| {})
            .await
            .unwrap_err();
        assert_eq!(error.kind, AddonFailure::Cancelled);
        // The server is gone: a network failure, worth retrying.
        let gone = release_for(&bytes, "1.2.0", "http://127.0.0.1:9/stats.zip");
        let error = download(&gone, fresh.path(), &CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert!(error.kind.transient());
    }
}
