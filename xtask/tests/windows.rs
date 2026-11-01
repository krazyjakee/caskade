use std::{collections::BTreeSet, fs, io::Read};
use xtask::{checks::FIXTURE_ROOT, fixtures, workspace_root};

#[test]
fn windows_corpus_covers_real_versions_features_and_every_mini_installer() {
    let root = workspace_root().join(FIXTURE_ROOT);
    let corpus = fixtures::load_windows(&root).unwrap();
    assert_eq!(corpus.len(), 33);
    let ids: BTreeSet<_> = corpus.iter().map(|f| f.expected.id.as_str()).collect();
    for id in [
        "gog-inno-unity",
        "gog-inno-sliced",
        "gog-inno5-legacy",
        "legacy-registry-path",
        "nsis-indie",
        "msi-plus-cab",
        "humble-win-zip",
        "unreal-zip",
        "steamlike-commonredist",
        "sevenzip-sfx",
        "zip-sfx",
    ] {
        assert!(ids.contains(id), "missing {id}");
    }
    for version in ["5.6.1", "6.0.5", "6.4.3"] {
        let inno: Vec<_> = corpus
            .iter()
            .filter(|f| f.expected.windows.as_ref().unwrap().version == version)
            .collect();
        assert_eq!(inno.len(), 6, "ISCC {version}");
        for feature in ["SLICED", "PASSWORD", "CODE", "REGISTRY", "INI"] {
            assert!(
                inno.iter().any(|f| f
                    .expected
                    .windows
                    .as_ref()
                    .unwrap()
                    .features
                    .iter()
                    .any(|v| v == feature)),
                "{version} {feature}"
            );
        }
    }
    for fixture in corpus {
        assert!(fixture.expected.windows.is_some());
        for host in ["windows", "linux", "macos"] {
            assert!(fixture.expected.host.contains_key(host));
        }
        let header = fs::read(fixture.input()).unwrap();
        match fixture.expected.format.as_str() {
            "inno" | "nsis" | "7z-sfx" | "zip-sfx" | "rar-sfx" => {
                assert!(header.starts_with(b"MZ"))
            }
            "msi" => assert!(header.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1")),
            "zip" => assert!(header.starts_with(b"PK")),
            other => panic!("unexpected Windows format {other}"),
        }
        if fixture.expected.id.contains("sliced") {
            assert!(fixture.parts().len() >= 3);
        }
        if fixture.expected.id == "msi-plus-cab" {
            assert_eq!(fixture.parts().len(), 2);
        }
    }
}

#[test]
fn portable_and_zip_sfx_payloads_match_committed_golden_hashes_without_builders() {
    let root = workspace_root().join(FIXTURE_ROOT);
    for fixture in fixtures::load_windows(&root)
        .unwrap()
        .into_iter()
        .filter(|f| matches!(f.expected.format.as_str(), "zip" | "zip-sfx"))
    {
        let mut archive = zip::ZipArchive::new(fs::File::open(fixture.input()).unwrap()).unwrap();
        let mut actual = BTreeSet::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let path = file.name().unwrap().trim_end_matches('/').to_owned();
            fixtures::safe_relative(&path).unwrap();
            actual.insert(path.clone());
            let expected = fixture
                .tree
                .iter()
                .find(|entry| entry.path == path)
                .unwrap();
            if file.is_file() {
                let mut data = Vec::new();
                file.read_to_end(&mut data).unwrap();
                use sha2::{Digest, Sha256};
                assert_eq!(data.len() as u64, expected.size);
                assert_eq!(
                    format!("{:x}", Sha256::digest(&data)),
                    *expected.sha256.as_ref().unwrap()
                );
            }
        }
        // Prefix parent directories need not have explicit ZIP directory entries.
        let files: BTreeSet<_> = fixture
            .tree
            .iter()
            .filter(|e| e.kind == fixtures::EntryKind::File)
            .map(|e| e.path.clone())
            .collect();
        assert!(files.is_subset(&actual));
    }
}

#[test]
fn every_download_pin_is_also_recorded_in_the_integrity_lock() {
    let root = workspace_root().join(FIXTURE_ROOT);
    let pins: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("src/windows/builders.json")).unwrap()).unwrap();
    let lock: xtask::checks::BuildLock =
        toml::from_str(&fs::read_to_string(root.join("build.lock")).unwrap()).unwrap();
    for (name, pin) in pins.as_object().unwrap() {
        let recorded = lock.builders.get(&format!("windows_{name}")).unwrap();
        assert!(recorded.contains(pin["sha256"].as_str().unwrap()));
        assert!(recorded.contains(pin["url"].as_str().unwrap()));
    }
}
