//! T2 matrix: actual committed fixture inputs × every committed host profile.
//!
//! Production archive probes list the actual fixture bytes; no expected trees
//! or test handlers are used to manufacture proposals. RAR cases remain explicit
//! unsupported inventory until their dedicated format handler is implemented.

use std::{collections::BTreeMap, fs, io::Cursor, path::Path};

use caskade_installer::{
    formats::archive::{self, ArchiveHandler, ArchiveProbe},
    host::HostCaps,
    plan::{PlanEngine, Policy},
    probe::ProbeEngine,
};
use serde_json::json;

fn engines() -> (ProbeEngine, PlanEngine) {
    let mut probes = ProbeEngine::default();
    probes.register(Box::new(ArchiveProbe));
    let mut planner = PlanEngine::default();
    planner.register(Box::new(ArchiveHandler)).unwrap();
    (probes, planner)
}

fn toml_files(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "missing matrix inputs: {}",
        directory.display()
    );
    paths
}

#[test]
fn every_fixture_by_every_host_profile() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let fixtures = tests.join("fixtures");
    let expectations = toml_files(&fixtures.join("expected"));
    let profiles = toml_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("data/hosts"));
    let (probes, planner) = engines();
    let policy = Policy::default();
    let mut count = 0;
    for profile in &profiles {
        let host: HostCaps = toml::from_str(&fs::read_to_string(profile).unwrap()).unwrap();
        let mut snapshots = BTreeMap::new();
        for expectation in &expectations {
            let expected: toml::Value =
                toml::from_str(&fs::read_to_string(expectation).unwrap()).unwrap();
            let id = expected["id"].as_str().unwrap();
            let input = expected["input"].as_str().unwrap();
            // Multipart fixtures are the concatenation of their committed bytes,
            // in the source volume order; this is input data, not a tree oracle.
            let mut bytes = vec![];
            for part in expected["parts"].as_array().unwrap() {
                bytes.extend(fs::read(fixtures.join(part.as_str().unwrap())).unwrap());
            }
            let mut reader = Cursor::new(bytes);
            let mut probe = probes.probe_reader(&mut reader, Path::new(input)).unwrap();
            let supported = archive::supported(probe.root.format);
            if supported {
                let root_id = probe.root.id.clone();
                probes
                    .list_children(&mut reader, &mut probe.root, &root_id)
                    .unwrap();
            }
            let plans = planner
                .plan(std::slice::from_ref(&probe.root), &host, &policy)
                .unwrap();
            if supported {
                assert_eq!(plans.selected, Some(0), "{id} / {}", profile.display());
                assert!(
                    plans
                        .candidates
                        .iter()
                        .any(|candidate| candidate.handler.0 == "archive")
                );
            } else {
                assert!(plans.candidates.is_empty());
            }
            snapshots.insert(
                id.to_owned(),
                json!({
                    "probe": probe.root,
                    "plans": plans,
                    "coverage": if supported { "production archive listing and plan" } else { "unsupported format (no registered handler)" },
                }),
            );
            count += 1;
        }
        let profile_name = profile.file_stem().unwrap().to_str().unwrap();
        insta::assert_json_snapshot!(format!("fixture_matrix_{profile_name}"), snapshots);
    }
    assert_eq!(count, expectations.len() * profiles.len());
    assert!(count >= 90, "the initial 15 × 6 matrix must not shrink");
}
