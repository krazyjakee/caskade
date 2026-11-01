use caskade_installer::{
    Input, Program, SourcePath, Step,
    manifest::{
        InstallManifest, ManifestError, ManifestPath, ManifestSource, RelocationVerdict,
        RootManifest, Sha256Digest,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

const POSIX: &[u8] = include_bytes!("manifests/install.v1.posix.json");
const WINDOWS: &[u8] = include_bytes!("manifests/install.v1.windows.json");
const MACOS: &[u8] = include_bytes!("manifests/install.v1.macos.json");

fn document() -> Value {
    serde_json::from_slice(POSIX).unwrap()
}

#[test]
fn current_manifests_round_trip_all_fields() {
    for bytes in [POSIX, WINDOWS, MACOS] {
        let manifest: InstallManifest = serde_json::from_slice(bytes).unwrap();
        assert_eq!(manifest.schema_version(), 1);
        assert_eq!(
            manifest.record().handlers[0].id,
            manifest.record().plan.candidate.handler
        );
        assert_eq!(
            manifest.record().relocation.verdict,
            RelocationVerdict::Unscanned
        );
        let serialized = serde_json::to_vec(&manifest).unwrap();
        assert_eq!(manifest, serde_json::from_slice(&serialized).unwrap());
        assert_eq!(
            serialized,
            serde_json::to_vec(&InstallManifest::from_json(&serialized).unwrap()).unwrap()
        );
        assert_eq!(manifest.clone().into_record(), manifest.record().clone());
    }
}

#[test]
fn windows_and_posix_path_fixtures_have_the_same_meaning_on_every_host() {
    let windows = InstallManifest::from_json(WINDOWS).unwrap();
    let record = windows.record();
    assert_eq!(record.files[0].path.as_str(), "bin/FixtureGame.exe");
    assert_eq!(record.files[2].path.as_str(), "data/café.txt");
    assert_eq!(
        record.launch_targets[0].path,
        PathBuf::from("bin/FixtureGame.exe")
    );
    assert_eq!(
        record.plan.relative_dir,
        PathBuf::from("Library/Fixture Game")
    );
    assert_eq!(
        record.plan.artifacts[0].children[0].path,
        PathBuf::from("bin/FixtureGame.exe")
    );
    assert_eq!(
        record.plan.candidate.warnings[0].path,
        Some("data/config.ini".into())
    );
    let value = serde_json::to_value(&windows).unwrap();
    for pointer in [
        "/files/0/path",
        "/files/1/path",
        "/files/2/path",
        "/launch_targets/0/path",
        "/plan/relative_dir",
        "/plan/artifacts/0/path",
        "/plan/artifacts/0/children/0/path",
        "/plan/candidate/steps/1/value/path",
        "/uninstall_steps/0/value/program/value/value",
    ] {
        assert!(
            !value
                .pointer(pointer)
                .unwrap()
                .as_str()
                .unwrap()
                .contains('\\'),
            "{pointer}"
        );
    }
    assert_eq!(
        record.source.final_url.as_ref().unwrap().as_str(),
        "https://cdn.example.invalid/setup.zip"
    );
    let posix = InstallManifest::from_json(POSIX).unwrap();
    assert_eq!(
        posix.record().source.input,
        Input::Path("downloads/fixture.tar.gz".into())
    );
    assert_eq!(posix.record().files[0].path.as_str(), "bin/FixtureGame");
    let macos = InstallManifest::from_json(MACOS).unwrap();
    assert_eq!(
        macos.record().launch_targets[0].path,
        PathBuf::from("FixtureGame.app/Contents/MacOS/FixtureGame")
    );
}

#[test]
fn every_supported_install_schema_version_migrates_to_the_current_record() {
    let current = InstallManifest::from_json(POSIX).unwrap();
    for bytes in [
        include_bytes!("manifests/install.v0.json").as_slice(),
        POSIX,
    ] {
        let loaded = InstallManifest::from_json(bytes).unwrap();
        assert_eq!(loaded, current);
        let serialized = serde_json::to_value(&loaded).unwrap();
        assert_eq!(serialized["schema_version"], 1);
        assert!(serialized["handlers"].is_array());
        assert!(serialized["files"][0].get("hash").is_none());
        assert_eq!(serialized["relocation"]["verdict"], "unscanned");
        assert_eq!(loaded, serde_json::from_value(serialized).unwrap());
    }
}

#[test]
fn every_supported_root_schema_version_round_trips_with_stable_identity() {
    let root = RootManifest::new("root-fixture-0001").unwrap();
    for bytes in [
        include_bytes!("manifests/root.v0.json").as_slice(),
        include_bytes!("manifests/root.v1.json").as_slice(),
    ] {
        let loaded: RootManifest = serde_json::from_slice(bytes).unwrap();
        assert_eq!(loaded, root);
        assert_eq!(loaded.schema_version(), 1);
        assert_eq!(loaded.root_id(), "root-fixture-0001");
        let output = serde_json::to_value(&loaded).unwrap();
        assert_eq!(
            output,
            json!({"schema_version":1,"root_id":"root-fixture-0001"})
        );
        assert_eq!(loaded, serde_json::from_value(output).unwrap());
    }
}

#[test]
fn migrations_refuse_ambiguous_legacy_data_instead_of_discarding_it() {
    let legacy: Value =
        serde_json::from_slice(include_bytes!("manifests/install.v0.json")).unwrap();
    for (pointer, replacement) in [
        ("/handlers", json!([])),
        ("/files", json!({})),
        ("/files/0", json!(null)),
    ] {
        let mut value = legacy.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(serde_json::from_value::<InstallManifest>(value).is_err());
    }
    let mut value = legacy.clone();
    value["files"][0]["sha256"] = json!("00".repeat(32));
    assert!(serde_json::from_value::<InstallManifest>(value).is_err());
    let mut value = legacy;
    value["relocation"] = json!({"verdict":"fragile"});
    assert!(serde_json::from_value::<InstallManifest>(value).is_err());
    assert!(
        serde_json::from_value::<RootManifest>(
            json!({"schema_version":0,"id":"old","root_id":"other"})
        )
        .is_err()
    );
}

#[test]
fn future_versions_and_invalid_version_headers_fail_explicitly() {
    for version in [json!(2), json!(u64::MAX)] {
        let mut value = document();
        value["schema_version"] = version.clone();
        assert!(matches!(
            InstallManifest::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ManifestError::UnsupportedVersion { .. })
        ));
        assert!(matches!(
            RootManifest::from_json(
                &serde_json::to_vec(&json!({"schema_version":version,"root_id":"root"})).unwrap()
            ),
            Err(ManifestError::UnsupportedVersion { .. })
        ));
    }
    for version in [json!(-1), json!(1.5), json!("1"), Value::Null] {
        let mut value = document();
        value["schema_version"] = version;
        assert!(InstallManifest::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let mut value = document();
    value.as_object_mut().unwrap().remove("schema_version");
    assert!(serde_json::from_value::<InstallManifest>(value).is_err());
    for invalid in [b"null".as_slice(), b"[]", b"{", b"{}"] {
        assert!(InstallManifest::from_json(invalid).is_err());
        assert!(RootManifest::from_json(invalid).is_err());
    }
}

#[test]
fn typed_game_paths_reject_absolute_and_traversing_values_on_every_host() {
    for pointer in [
        "/files/0/path",
        "/source/input/value",
        "/plan/relative_dir",
        "/plan/artifacts/0/path",
        "/plan/artifacts/0/children/0/path",
        "/plan/candidate/steps/0/value/to",
        "/plan/candidate/steps/0/value/strip",
        "/plan/candidate/steps/1/value/path",
        "/plan/candidate/warnings/0/path",
        "/launch_targets/0/path",
        "/launch_targets/0/working_dir",
        "/uninstall_steps/0/value/program/value/value",
    ] {
        for bad in [
            "/absolute/game",
            "C:\\Games\\FixtureGame",
            "C:/Games/FixtureGame",
            "C:relative",
            "\\\\server\\share\\game",
            "\\rooted",
            "../escape",
            "a\\..\\escape",
            "a/../escape",
        ] {
            let mut value = document();
            *value.pointer_mut(pointer).unwrap() = json!(bad);
            assert!(
                serde_json::from_value::<InstallManifest>(value).is_err(),
                "accepted {bad:?} at {pointer}"
            );
        }
    }
}

#[test]
fn relative_path_and_hash_types_validate_and_normalize_during_serde() {
    for bad in [
        "",
        ".",
        "..",
        "nul.txt",
        "CON",
        "a/b. ",
        "a\0b",
        "a:b",
        "/foo",
        "foo/../bar",
    ] {
        assert!(ManifestPath::new(bad).is_err(), "accepted {bad:?}");
        assert!(serde_json::from_value::<ManifestPath>(json!(bad)).is_err());
    }
    let path = ManifestPath::new("data\\.\\sub//café.txt").unwrap();
    assert_eq!(path.as_str(), "data/sub/café.txt");
    assert_eq!(path.as_path(), std::path::Path::new("data/sub/café.txt"));
    assert_eq!(serde_json::to_value(path).unwrap(), "data/sub/café.txt");
    let hash = Sha256Digest::new("AB".repeat(32)).unwrap();
    assert_eq!(hash.as_str(), "ab".repeat(32));
    assert_eq!(serde_json::to_value(&hash).unwrap(), "ab".repeat(32));
    assert_eq!(
        hash,
        serde_json::from_value(json!("AB".repeat(32))).unwrap()
    );
    for bad in ["", "abcdef", &"x".repeat(64), &"0".repeat(65)] {
        assert!(Sha256Digest::new(bad).is_err());
    }
}

#[test]
fn snapshotting_a_source_removes_host_specific_absolute_local_locations() {
    for (path, name) in [
        ("/home/user/Downloads/setup.zip", "setup.zip"),
        ("D:\\Downloads\\setup.zip", "setup.zip"),
        ("\\\\host\\share\\setup.zip", "setup.zip"),
        ("relative\\setup.zip", "relative/setup.zip"),
    ] {
        let source = ManifestSource::from_input(&Input::Path(path.into()), None, None).unwrap();
        assert_eq!(source.input, Input::Path(name.into()));
        let mut record = InstallManifest::from_json(POSIX).unwrap().into_record();
        record.plan.input = Input::Path(path.into());
        record.source = source;
        let manifest = InstallManifest::new(record).unwrap();
        assert_eq!(manifest.record().plan.input, Input::Path(name.into()));
        assert!(
            !serde_json::to_string(&manifest)
                .unwrap()
                .contains("Downloads")
        );
    }
    assert!(ManifestSource::from_input(&Input::Path("/".into()), None, None).is_err());
    assert!(ManifestSource::from_input(&Input::Path("../setup.zip".into()), None, None).is_err());
}

#[test]
fn every_step_variant_is_normalized_and_validated() {
    let runtime = json!({"type":"proton_prefix","value":{"id":"prefix-1"}});
    let source = json!({"type":"game","value":"bin\\FixtureGame"});
    let program = json!({"type":"source","value":source});
    let valid = vec![
        json!({"type":"extract","value":{"from":"source","select":[],"strip":"top\\folder","to":"data\\game"}}),
        json!({"type":"copy","value":{"from":source,"to":"data\\game"}}),
        json!({"type":"move","value":{"from":{"type":"artifact","value":{"artifact":"source","path":"."}},"to":"."}}),
        json!({"type":"run","value":{"program":program,"args":[],"env":{},"runtime":runtime,"timeout_secs":30,"expect":["bin\\FixtureGame"]}}),
        json!({"type":"run_interactive","value":{"program":program,"args":[],"env":{},"runtime":runtime,"target_dir":"."}}),
        json!({"type":"mount","value":{"from":"source","mount_point":"disc\\one"}}),
        json!({"type":"unmount","value":{"mount_point":"disc\\one"}}),
        json!({"type":"set_executable","value":{"path":"bin\\FixtureGame"}}),
        json!({"type":"symlink","value":{"path":"lib\\current","target":"..\\data\\library"}}),
        json!({"type":"write_file","value":{"path":"data\\generated.ini","contents":[1,2,3]}}),
        json!({"type":"registry_import","value":{"runtime":runtime,"entries":[]}}),
        json!({"type":"delegate","value":{"tool":"flatpak","args":["install"],"uninstall":[{"tool":"flatpak","args":["uninstall"]}]}}),
    ];
    let mut record = InstallManifest::from_json(POSIX).unwrap().into_record();
    record.plan.candidate.steps = serde_json::from_value(json!(valid)).unwrap();
    record.uninstall_steps = record.plan.candidate.steps.clone();
    let manifest = InstallManifest::new(record).unwrap();
    assert_eq!(manifest.record().plan.candidate.steps.len(), 12);
    assert_eq!(
        manifest,
        serde_json::from_value(serde_json::to_value(&manifest).unwrap()).unwrap()
    );
    for mut step in valid {
        let fields = step["value"].as_object_mut().unwrap();
        if let Some(path) = fields.get_mut("to") {
            *path = json!("../escape");
        } else if let Some(path) = fields.get_mut("target_dir") {
            *path = json!("../escape");
        } else if let Some(path) = fields.get_mut("mount_point") {
            *path = json!("../escape");
        } else if let Some(path) = fields.get_mut("path") {
            *path = json!("../escape");
        } else if let Some(program) = fields.get_mut("program") {
            program["value"]["value"] = json!("../escape");
        } else if let Some(runtime) = fields.get_mut("runtime") {
            runtime["value"]["id"] = json!("../escape");
        } else {
            fields.insert("tool".into(), json!("/usr/bin/tool"));
        }
        let mut value = document();
        value["uninstall_steps"] = json!([step]);
        assert!(serde_json::from_value::<InstallManifest>(value).is_err());
    }
}

#[test]
fn symlink_targets_may_go_up_only_within_the_game_tree() {
    for (path, target, valid) in [
        ("lib/current", "../data/library", true),
        ("lib/current", "../../outside", false),
        ("current", "../outside", false),
        ("lib/current", "/etc", false),
        ("lib/current", "C:\\Windows", false),
        ("Framework/Versions/Current", "A", true),
    ] {
        let mut record = InstallManifest::from_json(POSIX).unwrap().into_record();
        record.uninstall_steps = vec![Step::Symlink {
            path: path.into(),
            target: target.into(),
        }];
        assert_eq!(
            InstallManifest::new(record).is_ok(),
            valid,
            "{path} -> {target}"
        );
    }
}

#[test]
fn metadata_invariants_do_not_silently_hide_invalid_records() {
    for (pointer, value) in [
        ("/title", json!("")),
        ("/plan/root_id", json!(" ")),
        ("/handlers/0/version", json!("")),
        ("/handlers/0/id", json!("other")),
        ("/files/0/sha256", json!("invalid")),
        ("/plan/candidate/confidence", json!(2)),
        ("/plan/schema_version", json!(2)),
        ("/source/input", json!({"type":"urls","value":[]})),
        ("/source/final_url", json!("file:///etc/passwd")),
        ("/uninstall_steps/0/value/timeout_secs", json!(0)),
    ] {
        let mut document = document();
        *document.pointer_mut(pointer).unwrap() = value;
        assert!(
            serde_json::from_value::<InstallManifest>(document).is_err(),
            "{pointer}"
        );
    }
    for key in ["files", "handlers"] {
        let mut value = document();
        let items = value[key].as_array_mut().unwrap();
        items.push(items[0].clone());
        assert!(serde_json::from_value::<InstallManifest>(value).is_err());
    }
    let mut value = document();
    value["files"][0]["path"] = json!(".caskade/install.json");
    assert!(serde_json::from_value::<InstallManifest>(value).is_err());
    let mut value = document();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<InstallManifest>(value).is_err());
    let mut record = InstallManifest::from_json(POSIX).unwrap().into_record();
    record.launch_targets[0].score = f32::NAN;
    assert!(InstallManifest::new(record).is_err());
    for root in ["", " ", "bad\0id", "bad/root", "root\\id"] {
        assert!(RootManifest::new(root).is_err());
    }
}

#[test]
fn full_url_and_checksum_provenance_is_preserved() {
    let input = Input::Urls(vec![
        "https://example.invalid/setup.zip?release=1"
            .parse()
            .unwrap(),
    ]);
    let source = ManifestSource::from_input(
        &input,
        Some("https://cdn.example.invalid/final.zip".parse().unwrap()),
        Some(Sha256Digest::new("AA".repeat(32)).unwrap()),
    )
    .unwrap();
    let mut record = InstallManifest::from_json(POSIX).unwrap().into_record();
    record.source = source.clone();
    let manifest = InstallManifest::new(record).unwrap();
    assert_eq!(manifest.record().source, source);
    assert_eq!(manifest.record().plan.input, input);
    assert_eq!(
        manifest.record().source.checksum.as_ref().unwrap().as_str(),
        "aa".repeat(32)
    );
}

#[test]
fn shared_program_and_step_types_are_used_without_a_second_dsl() {
    let mut record = InstallManifest::from_json(POSIX).unwrap().into_record();
    record.uninstall_steps = vec![Step::Run {
        program: Program::Source(SourcePath::Game("bin/uninstall".into())),
        args: vec!["--remove".into()],
        env: BTreeMap::new(),
        runtime: record.runtime.clone(),
        timeout_secs: 60,
        expect: Vec::new(),
    }];
    let manifest = InstallManifest::new(record).unwrap();
    assert!(matches!(
        manifest.record().uninstall_steps[0],
        Step::Run { .. }
    ));
}

#[test]
fn artifact_root_program_is_the_selected_file_and_game_root_is_never_executable() {
    use caskade_installer::{ArtifactId, Program, RuntimeSpec, SourcePath, Step};
    let mut record = caskade_installer::manifest::InstallManifest::from_json(include_bytes!(
        "manifests/install.v1.posix.json"
    ))
    .unwrap()
    .into_record();
    record.plan.candidate.steps = vec![Step::Run {
        program: Program::Source(SourcePath::Artifact {
            artifact: ArtifactId("setup".into()),
            path: ".".into(),
        }),
        args: vec![],
        env: Default::default(),
        runtime: RuntimeSpec::Native,
        timeout_secs: 30,
        expect: vec![],
    }];
    assert!(caskade_installer::manifest::InstallManifest::new(record.clone()).is_ok());
    if let Step::Run { program, .. } = &mut record.plan.candidate.steps[0] {
        *program = Program::Source(SourcePath::Game(".".into()));
    }
    assert!(caskade_installer::manifest::InstallManifest::new(record).is_err());
}
