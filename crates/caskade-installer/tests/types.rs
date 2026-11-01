use std::{collections::BTreeMap, fmt::Debug, path::PathBuf};

use caskade_installer::types::*;
use schemars::{JsonSchema, schema_for};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use url::Url;

fn roundtrip<T>(value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let encoded = serde_json::to_string(value).expect("serialize");
    let decoded: T = serde_json::from_str(&encoded).expect("deserialize");
    assert_eq!(&decoded, value, "wire value: {encoded}");
}

/// Compare tested tags with the derived schema so new variants cannot silently
/// miss round-trip coverage. Payloads below exercise nested non-empty data too.
fn tagged_variants<T>(cases: Vec<T>)
where
    T: Serialize + DeserializeOwned + JsonSchema + PartialEq + Debug,
{
    let schema = serde_json::to_value(schema_for!(T)).unwrap();
    let mut expected: Vec<String> = schema["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variant| {
            variant["properties"]["type"]["const"]
                .as_str()
                .unwrap()
                .into()
        })
        .collect();
    let mut seen: Vec<String> = cases
        .iter()
        .map(|case| {
            roundtrip(case);
            serde_json::to_value(case).unwrap()["type"]
                .as_str()
                .unwrap()
                .into()
        })
        .collect();
    expected.sort();
    seen.sort();
    seen.dedup();
    assert_eq!(seen, expected, "every schema variant needs a sample");
}

fn unit_variants<T>()
where
    T: Serialize + DeserializeOwned + JsonSchema + PartialEq + Debug,
{
    let schema = serde_json::to_value(schema_for!(T)).unwrap();
    for variant in schema["enum"].as_array().unwrap() {
        let value: T = serde_json::from_value(variant.clone()).unwrap();
        roundtrip(&value);
        assert_eq!(serde_json::to_value(value).unwrap(), *variant);
    }
}

fn artifact_id() -> ArtifactId {
    ArtifactId("source:0".into())
}

fn source() -> SourcePath {
    SourcePath::Artifact {
        artifact: artifact_id(),
        path: ".".into(),
    }
}

fn warning() -> Warning {
    Warning {
        code: "registry_entries_skipped".into(),
        message: "Registry entries could not be replayed".into(),
        path: Some("game.reg".into()),
    }
}

fn launch() -> LaunchTarget {
    LaunchTarget {
        path: "Game/游戏.exe".into(),
        args: vec!["--language=en".into()],
        working_dir: "Game".into(),
        env: BTreeMap::from([("GAMEID".into(), "umu-default".into())]),
        runtime: RuntimeSpec::ProtonPrefix { id: "main".into() },
        score: 0.95,
        reason: "Explicit goggame play task".into(),
    }
}

fn steps() -> Vec<Step> {
    vec![
        Step::Extract {
            from: artifact_id(),
            select: vec!["app/**".into()],
            strip: Some("app".into()),
            to: ".".into(),
        },
        Step::Copy {
            from: source(),
            to: "Game".into(),
        },
        Step::Move {
            from: SourcePath::Game("old".into()),
            to: "new".into(),
        },
        Step::Run {
            program: Program::Source(source()),
            args: vec!["/VERYSILENT".into(), "/DIR=G:\\".into()],
            env: BTreeMap::from([("LANG".into(), "en_US.UTF-8".into())]),
            runtime: RuntimeSpec::Native,
            timeout_secs: 120,
            expect: vec!["Game.exe".into()],
        },
        Step::RunInteractive {
            program: Program::Tool("msiexec".into()),
            args: vec!["/i".into(), "game.msi".into()],
            env: BTreeMap::new(),
            runtime: RuntimeSpec::WinePrefix { id: "main".into() },
            target_dir: ".".into(),
        },
        Step::Mount {
            from: artifact_id(),
            mount_point: ".mount".into(),
        },
        Step::Unmount {
            mount_point: ".mount".into(),
        },
        Step::SetExecutable {
            path: "start.sh".into(),
        },
        Step::Symlink {
            path: "Framework/Current".into(),
            target: "A".into(),
        },
        Step::WriteFile {
            path: "config.ini".into(),
            contents: vec![0, 0xff, b'\n'],
        },
        Step::CopyRange {
            from: ArtifactId("pack".into()),
            offset: 4,
            length: u64::MAX,
            path: "bin/game".into(),
        },
        Step::RegistryImport {
            runtime: RuntimeSpec::ProtonPrefix { id: "main".into() },
            entries: vec![RegistryEntry {
                hive: RegistryHive::LocalMachine,
                key: "Software\\CaskadeFixture".into(),
                name: "InstallPath".into(),
                value: RegistryValue::String("G:\\".into()),
            }],
        },
        Step::Delegate {
            tool: "flatpak".into(),
            args: vec!["install".into(), "--user".into(), "game.flatpak".into()],
            uninstall: vec![DelegateCommand {
                tool: "flatpak".into(),
                args: vec!["uninstall".into(), "--user".into(), "org.game.Game".into()],
                only_if: Some(DelegateCheck {
                    tool: "flatpak".into(),
                    args: vec!["info".into(), "--user".into(), "org.game.Game".into()],
                }),
            }],
            only_if: None,
        },
    ]
}

fn candidate() -> Candidate {
    Candidate {
        handler: HandlerId("builtin.archive@1".into()),
        strategy: Strategy::Extract,
        confidence: 0.875,
        fidelity: Fidelity::Equivalent,
        requires: Requirements {
            executes_code: false,
            runtime: None,
            may_prompt_elevation: false,
            interactive: false,
            est_disk_bytes: Some(1_048_576),
        },
        steps: steps(),
        warnings: vec![warning()],
    }
}

fn questions() -> Vec<Question> {
    vec![
        Question::ReinstallAfterMove {
            title: "Fixture Game".into(),
            source: Input::Path("setup.exe".into()),
            reason: "binary references to the old path".into(),
        },
        Question::ChooseInstallDirectory {
            paths: vec!["/observed/Game".into()],
            reason: "Installer chose another folder".into(),
        },
        Question::ChooseLaunchTarget {
            targets: vec![launch()],
        },
        Question::ChooseInstaller {
            artifacts: vec![artifact_id()],
        },
        Question::ChooseAsset {
            assets: vec![DownloadAsset {
                name: "game-linux.zip".into(),
                url: Url::parse("https://example.com/game.zip").unwrap(),
                size: Some(1024),
                sha256: Some("a".repeat(64)),
            }],
        },
        Question::LowConfidence {
            candidates: vec![candidate()],
        },
        Question::ConfirmExecution {
            candidates: vec![candidate()],
        },
        Question::ChooseDestination {
            reason: "A ROM scanner keeps these ROMs together".into(),
            options: vec![
                Destination {
                    label: "Move into GBA library".into(),
                    folder: Some("/roms/gba".into()),
                    copy: false,
                },
                Destination {
                    label: "Keep them where they are".into(),
                    folder: None,
                    copy: false,
                },
            ],
        },
    ]
}

fn reasons() -> Vec<Reason> {
    vec![
        Reason::UnknownFormat,
        Reason::UnsupportedFormat {
            format: Format::Dmg,
        },
        Reason::PlatformMismatch {
            payload: PayloadPlatform::Macos,
            host: OperatingSystem::Linux,
        },
        Reason::MissingRuntime {
            runtime: RuntimeSpec::WinePrefix { id: "main".into() },
        },
        Reason::ExecutionDisabled,
        Reason::InvalidInput {
            message: "Input set is missing part 2".into(),
        },
    ]
}

fn installed() -> Installed {
    Installed {
        root_id: "root-1".into(),
        dir: "游戏 (2)".into(),
        title: "游戏".into(),
        ids: BTreeMap::from([("gog".into(), "1234567890".into())]),
        launch_targets: vec![launch()],
        runtime: RuntimeSpec::ProtonPrefix { id: "main".into() },
        dependencies: vec![Dependency {
            id: "vcrun2022".into(),
            name: "Visual C++ runtime".into(),
            installed: false,
            install_steps: vec![steps().pop().unwrap()],
            uninstall_steps: vec![],
            runtime_verbs: vec!["vcrun2022".into()],
        }],
        warnings: vec![warning()],
        manifest: ".caskade/install.json".into(),
    }
}

fn artifact() -> ArtifactNode {
    ArtifactNode {
        id: artifact_id(),
        path: "game.zip".into(),
        format: Format::Zip,
        platform: PayloadPlatform::Windows,
        role: Role::Installer,
        size: Some(u64::MAX),
        facts: BTreeMap::from([
            ("test.null".into(), Value::Null),
            (
                "test.metadata".into(),
                json!({"title": "游戏", "flags": [true, 2]}),
            ),
        ]),
        children: vec![ArtifactNode {
            id: ArtifactId("source:0/game.exe".into()),
            path: "Game/游戏.exe".into(),
            format: Format::Pe,
            platform: PayloadPlatform::Windows,
            role: Role::Application,
            size: None,
            facts: BTreeMap::new(),
            children: vec![],
            children_complete: true,
        }],
        children_complete: true,
    }
}

#[test]
fn plan_and_outcome_schema_snapshots() {
    insta::assert_json_snapshot!("install_plan_schema", schema_for!(InstallPlan));
    insta::assert_json_snapshot!("install_outcome_schema", schema_for!(InstallOutcome));
}

#[test]
fn every_unit_enum_variant_roundtrips() {
    unit_variants::<Format>();
    unit_variants::<Role>();
    unit_variants::<Strategy>();
    unit_variants::<Fidelity>();
    unit_variants::<RegistryHive>();
    unit_variants::<OperatingSystem>();
}

#[test]
fn every_input_platform_and_runtime_variant_roundtrips() {
    tagged_variants(vec![
        Input::Path(PathBuf::from("游戏 archive.zip")),
        Input::Urls(vec![
            Url::parse("https://example.com/game.7z.001#sha256=abc").unwrap(),
            Url::parse("https://example.com/game.7z.002").unwrap(),
        ]),
    ]);
    tagged_variants(vec![
        PayloadPlatform::Windows,
        PayloadPlatform::Linux,
        PayloadPlatform::Macos,
        PayloadPlatform::ConsoleRom {
            platform: Some("ps1".into()),
        },
        PayloadPlatform::ConsoleRom { platform: None },
        PayloadPlatform::Unknown,
    ]);
    tagged_variants(vec![
        RuntimeSpec::Native,
        RuntimeSpec::ProtonPrefix { id: "main".into() },
        RuntimeSpec::WinePrefix { id: "main".into() },
    ]);
}

#[test]
fn every_step_and_source_variant_roundtrips() {
    tagged_variants(steps());
    tagged_variants(vec![source(), SourcePath::Game("游戏/start.sh".into())]);
    tagged_variants(vec![Program::Source(source()), Program::Tool("7zz".into())]);
    tagged_variants(vec![
        RegistryValue::String("G:\\游戏".into()),
        RegistryValue::ExpandString("%USERPROFILE%\\游戏".into()),
        RegistryValue::MultiString(vec!["one".into(), "two".into()]),
        RegistryValue::Dword(u32::MAX),
        RegistryValue::Qword(u64::MAX),
        RegistryValue::Binary(vec![0, 0xff]),
    ]);
}

#[test]
fn every_outcome_and_question_variant_roundtrips() {
    tagged_variants(questions());
    tagged_variants(reasons());
    let handoffs = vec![
        HandOff::Rom {
            platform: "ps1".into(),
            files: vec!["Game.cue".into(), "Game.bin".into()],
        },
        HandOff::Delegated {
            launch_uri: "flatpak:org.game.Game".into(),
            uninstall_steps: vec![steps().pop().unwrap()],
        },
        HandOff::Package {
            package: "game".into(),
            title: "Game".into(),
            program: Some("/usr/games/game".into()),
            args: vec!["--fullscreen".into()],
            uninstall_steps: vec![steps().pop().unwrap()],
        },
    ];
    tagged_variants(handoffs.clone());
    let mut outcomes = vec![InstallOutcome::Installed(installed())];
    outcomes.extend(handoffs.into_iter().map(InstallOutcome::HandOff));
    outcomes.extend(questions().into_iter().map(InstallOutcome::NeedsUser));
    outcomes.extend(reasons().into_iter().map(InstallOutcome::Unsupported));
    tagged_variants(outcomes);
}

#[test]
fn full_plan_and_plan_set_roundtrip() {
    let plan = InstallPlan {
        schema_version: 1,
        input: Input::Path("game.zip".into()),
        artifacts: vec![artifact()],
        root_id: "root-1".into(),
        relative_dir: "游戏".into(),
        candidate: candidate(),
    };
    roundtrip(&plan);
    roundtrip(&PlanSet {
        candidates: vec![plan.candidate.clone()],
        selected: Some(0),
        question: None,
    });
    roundtrip(&PlanSet {
        candidates: vec![plan.candidate],
        selected: None,
        question: Some(questions().remove(0)),
    });
    roundtrip(&PlanSet {
        candidates: vec![],
        selected: None,
        question: None,
    });
}

#[test]
fn wire_format_uses_explicit_tags_and_relative_destinations() {
    assert_eq!(
        serde_json::to_value(Step::SetExecutable {
            path: "start.sh".into()
        })
        .unwrap(),
        json!({"type": "set_executable", "value": {"path": "start.sh"}})
    );
    let installed = installed();
    assert!(installed.dir.is_relative());
    assert!(installed.manifest.is_relative());
    for target in installed.launch_targets {
        assert!(target.path.is_relative());
        assert!(target.working_dir.is_relative());
    }
    let schema = serde_json::to_value(schema_for!(Candidate)).unwrap();
    assert_eq!(schema["properties"]["confidence"]["minimum"], 0);
    assert_eq!(schema["properties"]["confidence"]["maximum"], 1);
    assert!(serde_json::from_value::<Step>(json!({"type": "execute_shell"})).is_err());
    assert!(
        serde_json::from_value::<Input>(json!({"type": "urls", "value": ["not a url"]})).is_err()
    );
}

#[test]
fn specta_reflects_complete_public_graph() {
    let types = specta::Types::default()
        .register::<InstallPlan>()
        .register::<InstallOutcome>()
        .register::<PlanSet>();
    assert!(types.len() >= 20);
}
