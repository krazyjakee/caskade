#![cfg(feature = "linux-formats")]
use caskade_installer::{
    Format, Strategy,
    formats::{
        archive::{ArchiveOptions, NoopControl},
        linux::{deb::*, rpm},
    },
    host::HostCaps,
    plan::{PlanEngine, Policy},
    probe::ProbeEngine,
    safety::{Limits, SafeDir, open_staging},
    testing::{FakeAction, FakeRuntime, FakeScript, TestRoot},
};
use std::{collections::BTreeMap, io::Cursor, path::Path};

use crate::fixtures;
fn fixture(id: &str) -> fixtures::Fixture {
    fixtures::load_packages(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
        .unwrap()
        .into_iter()
        .find(|f| f.expected.id == id)
        .unwrap()
}
fn engine() -> ProbeEngine {
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(PackageProbe::new()));
    engine
}
fn host() -> HostCaps {
    let mut host: HostCaps = toml::from_str(include_str!("hosts/linux.toml")).unwrap();
    host.packages.managers = BTreeMap::from([
        ("apt".into(), "/usr/bin/apt".into()),
        ("dpkg-query".into(), "/usr/bin/dpkg-query".into()),
        ("pkexec".into(), "/usr/bin/pkexec".into()),
        ("dnf".into(), "/usr/bin/dnf".into()),
        ("rpm".into(), "/usr/bin/rpm".into()),
    ]);
    host.packages.installed.insert("/bin/sh".into());
    host
}
fn plans(node: &caskade_installer::ArtifactNode, host: &HostCaps) -> caskade_installer::PlanSet {
    let mut planner = PlanEngine::new();
    planner.register(Box::new(DebHandler::new())).unwrap();
    planner.register(Box::new(rpm::RpmHandler::new())).unwrap();
    planner
        .plan(std::slice::from_ref(node), host, &Policy::default())
        .unwrap()
}

#[test]
fn genuine_deb_and_rpm_metadata_listing_and_confined_payload_match_golden() {
    for id in [
        "deb-self-contained",
        "deb-missing-dep",
        "rpm-self-contained",
    ] {
        let fixture = fixture(id);
        let path = fixture.input();
        let mut probe = engine().probe_path(&path).unwrap();
        assert_eq!(
            probe.root.format,
            if id.starts_with("rpm") {
                Format::Rpm
            } else {
                Format::Deb
            }
        );
        assert!(
            probe.root.facts.contains_key("linux.package"),
            "{:?}",
            probe.root.facts
        );
        let artifact = probe.root.id.clone();
        engine()
            .list_children(
                &mut std::fs::File::open(&path).unwrap(),
                &mut probe.root,
                &artifact,
            )
            .unwrap();
        assert!(
            probe
                .root
                .children
                .iter()
                .any(|n| n.path.ends_with("start.sh"))
        );
        let root = TestRoot::new().unwrap();
        let packed = path.metadata().unwrap().len();
        let mut writer = SafeDir::new(
            open_staging(root.path()).unwrap(),
            Limits::default(),
            packed,
            true,
        );
        let (info, nodes) = if id.starts_with("rpm") {
            rpm::extract(
                &mut std::fs::File::open(path).unwrap(),
                &mut writer,
                &ArchiveOptions::default(),
                &mut NoopControl,
            )
        } else {
            extract(
                &mut std::fs::File::open(path).unwrap(),
                &mut writer,
                &ArchiveOptions::default(),
                &mut NoopControl,
            )
        }
        .unwrap();
        root.assert_sentinel_untouched();
        fixture.assert_tree(root.path()).unwrap();
        assert!(!info.maintainer_scripts.is_empty());
        assert!(
            !root
                .path()
                .join("opt/caskade-fixture/maintainer-ran")
                .exists()
        );
        assert!(nodes.iter().any(|n| n.format == Format::Elf));
    }
}

#[test]
fn missing_dependency_and_absolute_packages_delegate_with_recorded_uninstall_no_question() {
    for id in ["deb-missing-dep", "deb-absolute-path"] {
        let node = engine().probe_path(&fixture(id).input()).unwrap().root;
        let plans = plans(&node, &host());
        assert!(plans.question.is_none());
        assert_eq!(plans.candidates.len(), 1);
        let candidate = &plans.candidates[plans.selected.unwrap() as usize];
        assert_eq!(candidate.strategy, Strategy::Delegate);
        assert!(candidate.requires.may_prompt_elevation);
        let caskade_installer::Step::Delegate {
            tool,
            args,
            uninstall,
            ..
        } = &candidate.steps[0]
        else {
            panic!("expected delegate")
        };
        assert_eq!(tool, "pkexec");
        assert_eq!(args[0], "/usr/bin/apt");
        assert_eq!(
            args.last().unwrap(),
            &caskade_installer::exec::source_argument(&node.id)
        );
        assert_eq!(uninstall.len(), 1);
        assert!(uninstall[0].args.iter().any(|s| s == "remove"));
        // A package removed by hand is left alone, not removed again.
        let check = uninstall[0].only_if.as_ref().expect("installed check");
        assert_eq!(check.tool, "dpkg-query");
        assert_eq!(check.args[..2], ["--status", "--"]);
        assert_eq!(uninstall[0].args.last(), check.args.last());
    }
    let node = engine()
        .probe_path(&fixture("deb-self-contained").input())
        .unwrap()
        .root;
    let plans = plans(&node, &host());
    assert_eq!(
        plans.candidates[plans.selected.unwrap() as usize].strategy,
        Strategy::Extract
    );
    assert!(
        plans.candidates[0]
            .warnings
            .iter()
            .any(|w| w.code == "maintainer_scripts_skipped")
    );
}

#[test]
fn dependency_queries_use_only_injected_runtime_and_host_paths() {
    let bytes = b"Package: test\nVersion: 1\nDepends: hello | alternative, present\n";
    let info = parse_control(bytes).unwrap();
    let runtime = FakeRuntime::new([1, 1, 0].into_iter().map(|code| FakeScript {
        actions: vec![FakeAction::Exit { code }],
    }));
    let root = TestRoot::new().unwrap();
    let report = check_dependencies(&runtime, &host(), &info, root.path()).unwrap();
    root.assert_clean();
    assert_eq!(report.missing, ["hello | alternative"]);
    assert_eq!(report.installed, ["present"]);
    assert!(runtime.processes().iter().all(|p| {
        p.command.program == Path::new("/usr/bin/dpkg-query")
            && p.command
                .args
                .starts_with(&["--status".into(), "--".into()])
    }));
    let runtime = FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::Hang],
    }]);
    assert!(check_dependencies(&runtime, &host(), &info, root.path()).is_err());
    assert!(runtime.processes()[0].killed);
    root.assert_clean();
}

#[tokio::test]
async fn self_contained_packages_rewrite_final_desktop_references_skip_scripts_and_launch() {
    use caskade_installer::{
        exec::{CancellationToken, ExecRequest, Executor},
        testing::{FakeDiskSpace, RecordingSink},
    };
    use std::sync::Arc;
    for id in ["deb-self-contained", "rpm-self-contained"] {
        let fixture = fixture(id);
        let node = engine().probe_path(&fixture.input()).unwrap().root;
        let root = TestRoot::new().unwrap();
        std::fs::create_dir(root.path().join("Game")).unwrap();
        let sources = BTreeMap::from([(node.id.clone(), fixture.input())]);
        let extractor = Arc::new(PackageExtractor::new(sources.clone()));
        let runtime = Arc::new(FakeRuntime::default());
        let report = Executor::new(
            runtime.clone(),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        )
        .with_extractor(extractor.clone())
        .execute(
            ExecRequest {
                job_id: id.into(),
                root: root.path().into(),
                relative_dir: "Game".into(),
                plans: plans(&node, &host()),
                sources,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
        root.assert_sentinel_untouched();
        assert!(runtime.processes().is_empty());
        assert_eq!(report.dir.file_name().unwrap(), "Game (2)");
        assert!(
            !report
                .dir
                .join("opt/caskade-fixture/maintainer-ran")
                .exists()
        );
        let refs = extractor.take_references(&node.id).unwrap();
        assert_eq!(refs.len(), 3);
        // Exec values are quoted, with backslashes doubled as the Desktop
        // Entry format requires (Windows paths have them).
        assert!(
            refs.iter().all(|r| r.origin == "caskade-generated"
                && r.replacement
                    .replace("\\\\", "\\")
                    .contains(report.dir.to_str().unwrap())
                && !r.replacement.contains("staging")),
            "{:?} in {refs:#?}",
            report.dir
        );
        let desktop = std::fs::read_to_string(
            report
                .dir
                .join("usr/share/applications/caskade-fixture.desktop"),
        )
        .unwrap();
        for reference in &refs {
            assert!(desktop.contains(&format!("{}={}", reference.key, reference.replacement)));
        }
        let recorded: Vec<GeneratedReference> = serde_json::from_slice(
            &std::fs::read(report.dir.join(".caskade/package-references.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(recorded, refs);
        let hints = discovery_hints(&refs);
        assert_eq!(hints.launch_targets.len(), 1);
        assert_eq!(
            hints.launch_targets[0].path,
            Path::new(&fixture.expected.launch_target)
        );
        let discovery =
            caskade_installer::discover::discover(&report.dir, &host(), &hints).unwrap();
        assert!(discovery.question.is_none());
        assert_eq!(
            discovery.launch_targets[0].path,
            Path::new(&fixture.expected.launch_target)
        );
        assert!(
            extractor
                .take_artifacts(&node.id)
                .unwrap()
                .iter()
                .any(|n| n.format == Format::Elf)
        );
        #[cfg(target_os = "linux")]
        {
            let token = root.path().join(format!("{id}-token"));
            let output =
                std::process::Command::new(report.dir.join(&fixture.expected.launch_target))
                    .args(["--token", token.to_str().unwrap()])
                    .output()
                    .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                std::fs::read_to_string(token)
                    .unwrap()
                    .trim()
                    .ends_with("/FixtureGame.x86_64")
            );
            root.assert_sentinel_untouched();
        }
    }
}

#[test]
fn generated_rewrite_cannot_follow_links_or_modify_hardlink_aliases() {
    let root = TestRoot::new().unwrap();
    let mut dir = SafeDir::new(
        open_staging(root.path()).unwrap(),
        Limits::default(),
        1024,
        true,
    );
    dir.write_file("original", &b"original"[..], Some(8), 0, 0o644)
        .unwrap();
    dir.hardlink("alias", "original", 0).unwrap();
    dir.rewrite_generated_file("original", b"generated")
        .unwrap();
    assert_eq!(
        std::fs::read(root.path().join("alias")).unwrap(),
        b"original"
    );
    assert_eq!(
        std::fs::read(root.path().join("original")).unwrap(),
        b"generated"
    );
    assert!(dir.rewrite_generated_file("../sentinel", b"bad").is_err());
    root.assert_sentinel_untouched();
}

#[test]
fn ar_control_and_rpm_header_bombs_fail_before_unbounded_allocations() {
    for text in [
        "Package: x\nVersion: 1\nPackage: duplicate\n",
        " continuation\n",
        "Package: ../sentinel\nVersion: 1\n",
        "Package: x\nVersion: 1\nDepends: --bad\n",
        "Package: x\nVersion: 1\0bad\n",
    ] {
        let root = TestRoot::new().unwrap();
        let result = parse_control(text.as_bytes());
        root.assert_clean();
        assert!(result.is_err());
    }
    let deb = std::fs::read(fixture("deb-self-contained").input()).unwrap();
    let mut corrupted = deb.clone();
    corrupted[8 + 48..8 + 58].copy_from_slice(b"9999999999");
    let root = TestRoot::new().unwrap();
    assert!(ar_members(&mut Cursor::new(corrupted)).is_err());
    root.assert_clean();
    let rpm = std::fs::read(fixture("rpm-self-contained").input()).unwrap();
    for (offset, value) in [(104, u32::MAX), (108, u32::MAX)] {
        let mut bytes = rpm.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        let root = TestRoot::new().unwrap();
        let result = rpm::inspect(&mut Cursor::new(bytes));
        root.assert_clean();
        assert!(result.is_err());
    }
    for (bytes, rpm_format) in [(&deb, false), (&rpm, true)] {
        for percent in [25, 50, 75, 99] {
            let bytes = &bytes[..bytes.len() * percent / 100];
            let root = TestRoot::new().unwrap();
            let mut dir = SafeDir::new(
                open_staging(root.path()).unwrap(),
                Limits::default(),
                bytes.len() as u64,
                true,
            );
            let result = if rpm_format {
                rpm::extract(
                    &mut Cursor::new(bytes),
                    &mut dir,
                    &ArchiveOptions::default(),
                    &mut NoopControl,
                )
            } else {
                extract(
                    &mut Cursor::new(bytes),
                    &mut dir,
                    &ArchiveOptions::default(),
                    &mut NoopControl,
                )
            };
            root.assert_sentinel_untouched();
            assert!(result.is_err());
        }
    }
}

#[test]
fn rpm_cannot_hide_required_dependencies_in_mismatched_or_duplicate_tags() {
    fn be(bytes: &[u8], at: usize) -> usize {
        u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
    }
    let source = std::fs::read(fixture("rpm-self-contained").input()).unwrap();
    let signature = 16 + be(&source, 104) * 16 + be(&source, 108);
    let header = 96 + signature + (8 - signature % 8) % 8;
    let count = be(&source, header + 8);
    let entries = header + 16;
    let flags = (0..count)
        .map(|index| entries + index * 16)
        .find(|at| be(&source, *at) == ::rpm::IndexTag::RPMTAG_REQUIREFLAGS as usize)
        .unwrap();
    let mut mismatch = source.clone();
    mismatch[flags + 12..flags + 16].fill(0);
    let root = TestRoot::new().unwrap();
    assert!(rpm::inspect(&mut Cursor::new(mismatch)).is_err());
    let mut duplicate = source;
    let tag = duplicate[entries..entries + 4].to_vec();
    duplicate[entries + 16..entries + 20].copy_from_slice(&tag);
    assert!(rpm::inspect(&mut Cursor::new(duplicate)).is_err());
    root.assert_clean();
}

#[test]
fn dnf5_delegate_argv_omits_unsupported_subcommand_separator_and_validates_identity() {
    let mut node = engine()
        .probe_path(&fixture("rpm-self-contained").input())
        .unwrap()
        .root;
    node.facts.get_mut("linux.package").unwrap()["dependencies"] =
        serde_json::json!([[{"name":"missing-runtime","constraint":null}]]);
    for root in [false, true] {
        let mut host = host();
        host.packages.root = root;
        host.packages.installed.clear();
        let set = plans(&node, &host);
        let candidate = set
            .candidates
            .iter()
            .find(|c| c.strategy == Strategy::Delegate)
            .unwrap();
        let caskade_installer::Step::Delegate {
            tool,
            args,
            uninstall,
            ..
        } = &candidate.steps[0]
        else {
            panic!()
        };
        let expected = vec![
            "install".to_owned(),
            "-y".into(),
            caskade_installer::exec::source_argument(&node.id),
        ];
        if root {
            assert_eq!(tool, "dnf");
            assert_eq!(*args, expected);
            assert_eq!(uninstall[0].args[..2], ["remove", "-y"]);
        } else {
            assert_eq!(tool, "pkexec");
            assert_eq!(&args[1..], expected);
            assert_eq!(args[0], "/usr/bin/dnf");
            assert_eq!(uninstall[0].args[..3], ["/usr/bin/dnf", "remove", "-y"]);
        }
        assert!(!args.iter().any(|arg| arg == "--"));
        assert!(!uninstall[0].args.iter().any(|arg| arg == "--"));
    }
    let mut spoofed = node;
    spoofed.facts.get_mut("linux.package").unwrap()["name"] = "--all".into();
    let mut plan = PlanEngine::new();
    plan.register(Box::new(rpm::RpmHandler::new())).unwrap();
    assert!(plan.plan(&[spoofed], &host(), &Policy::default()).is_err());
}

#[test]
fn skipped_rpm_script_interpreters_do_not_block_self_contained_extraction() {
    let node = engine()
        .probe_path(&fixture("rpm-self-contained").input())
        .unwrap()
        .root;
    let info: PackageInfo = serde_json::from_value(node.facts["linux.package"].clone()).unwrap();
    assert!(!info.maintainer_scripts.is_empty());
    assert!(info.dependencies.is_empty());
    let mut host = host();
    host.packages.installed.clear();
    host.packages.managers.clear();
    let planned = plans(&node, &host);
    assert!(
        planned
            .candidates
            .iter()
            .any(|candidate| candidate.strategy == Strategy::Extract)
    );
    assert!(
        planned
            .candidates
            .iter()
            .any(|candidate| !candidate.warnings.is_empty())
    );
}
