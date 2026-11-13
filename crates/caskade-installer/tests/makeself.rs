#![cfg(feature = "linux-formats")]
use caskade_installer::{
    Format, Strategy,
    formats::{
        archive::{self, ArchiveOptions, NoopControl},
        linux::{makeself::*, mojosetup},
    },
    host::HostCaps,
    plan::{ExecutionPolicy, PlanEngine, Policy},
    probe::ProbeEngine,
    safety::{Limits, SafeDir, open_staging},
    testing::{TestRoot, probe_tree},
};
use std::{io::Cursor, path::Path};

use crate::fixtures;

fn corpus() -> Vec<fixtures::Fixture> {
    fixtures::load_linux_shell(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
        .unwrap()
}
fn engine() -> ProbeEngine {
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(MakeselfProbe));
    engine.register(Box::new(archive::ArchiveProbe));
    engine
}
fn host() -> HostCaps {
    caskade_installer::pipeline::input::host(Some("linux")).unwrap()
}
fn plans(
    node: &caskade_installer::ArtifactNode,
    host: &HostCaps,
    execution: ExecutionPolicy,
) -> caskade_installer::PlanSet {
    let mut engine = PlanEngine::new();
    engine.register(Box::new(MakeselfHandler)).unwrap();
    engine
        .register(Box::new(mojosetup::MojoSetupHandler))
        .unwrap();
    engine
        .plan(
            std::slice::from_ref(node),
            host,
            &Policy {
                execution,
                ..Policy::default()
            },
        )
        .unwrap()
}

#[test]
fn genuine_makeself_and_structural_mojo_fixtures_list_extract_and_discover() {
    assert_eq!(corpus().len(), 2);
    for fixture in corpus() {
        let path = fixture.input();
        let mut probe = engine().probe_path(&path).unwrap();
        let expected = if fixture.expected.id.starts_with("gog") {
            Format::MojoSetup
        } else {
            Format::Makeself
        };
        assert_eq!(probe.root.format, expected);
        let allowed = plans(&probe.root, &host(), ExecutionPolicy::Allow);
        assert_eq!(
            allowed.candidates[allowed.selected.unwrap() as usize].strategy,
            Strategy::Extract
        );
        let id = probe.root.id.clone();
        engine()
            .list_children(
                &mut std::fs::File::open(&path).unwrap(),
                &mut probe.root,
                &id,
            )
            .unwrap();
        assert!(
            probe
                .root
                .children
                .iter()
                .any(|n| n.path == Path::new("start.sh"))
        );
        assert!(
            probe
                .root
                .children
                .iter()
                .all(|n| !n.path.starts_with("data") && !n.path.starts_with("mojosetup"))
        );
        let root = TestRoot::new().unwrap();
        let packed = path.metadata().unwrap().len();
        let mut dir = SafeDir::new(
            open_staging(root.path()).unwrap(),
            Limits::default(),
            packed,
            true,
        );
        extract(
            &mut std::fs::File::open(&path).unwrap(),
            &mut dir,
            &ArchiveOptions::default(),
            &mut NoopControl,
        )
        .unwrap();
        root.assert_sentinel_untouched();
        fixture.assert_tree(root.path()).unwrap();
        assert!(
            probe_tree(root.path())
                .iter()
                .any(|n| n.format == Format::Elf)
        );
        let hints = mojosetup::discovery_hints(&dir);
        assert_eq!(
            hints.installer_title.as_deref(),
            Some("Caskade Fixture Game")
        );
        assert_eq!(hints.ids.get("gog").map(String::as_str), Some("1234567890"));
        let found = mojosetup::discover(root.path(), &host()).unwrap();
        assert_eq!(found.title, "Caskade Fixture Game");
        assert_eq!(found.launch_targets[0].path, Path::new("start.sh"));
        assert!(found.question.is_none());
    }
}

#[tokio::test]
async fn both_shell_installers_install_statically_then_launch_on_linux() {
    use caskade_installer::{
        exec::{CancellationToken, ExecRequest, Executor},
        testing::{FakeDiskSpace, FakeRuntime, RecordingSink},
    };
    use std::{collections::BTreeMap, sync::Arc};
    for fixture in corpus() {
        let root = TestRoot::new().unwrap();
        let node = engine().probe_path(&fixture.input()).unwrap().root;
        let plans = plans(&node, &host(), ExecutionPolicy::Never);
        let candidate = &plans.candidates[plans.selected.unwrap() as usize];
        assert_eq!(candidate.strategy, Strategy::Extract);
        assert!(!candidate.requires.executes_code);
        assert!(plans.candidates.iter().all(|c| !c.requires.executes_code));
        let sources = BTreeMap::from([(node.id.clone(), fixture.input())]);
        // The normal ArchiveExtractor routes shell handlers too, so both CLI
        // extraction paths work even when no 7zz sidecar is configured.
        let extractor = Arc::new(archive::ArchiveExtractor::new(sources.clone()));
        let runtime = Arc::new(FakeRuntime::default());
        let report = Executor::new(
            runtime.clone(),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        )
        .with_extractor(extractor.clone())
        .execute(
            ExecRequest {
                job_id: "shell".into(),
                root: root.path().into(),
                relative_dir: "Game".into(),
                plans,
                sources,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
        root.assert_sentinel_untouched();
        assert!(
            runtime.processes().is_empty(),
            "installer header was executed"
        );
        fixture.assert_tree(&report.dir).unwrap();
        assert!(
            probe_tree(&report.dir)
                .iter()
                .any(|n| n.format == Format::Elf)
        );
        let hints = extractor.take_discovery_hints(&node.id).unwrap();
        let found = caskade_installer::discover::discover(&report.dir, &host(), &hints).unwrap();
        assert_eq!(found.launch_targets[0].path, Path::new("start.sh"));
        #[cfg(target_os = "linux")]
        {
            let token = root.path().join("launch-token");
            let target = &found.launch_targets[0];
            let output = std::process::Command::new(report.dir.join(&target.path))
                .current_dir(report.dir.join(&target.working_dir))
                .args(&target.args)
                .arg("--token")
                .arg(&token)
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

fn skip_script(payload: &[u8], extra: &str) -> Vec<u8> {
    let prefix =
        format!("#!/bin/sh\n# Makeself generated test header\n{extra}skip=000000\nexit 91\n");
    let lines = prefix.lines().count();
    let header = prefix.replace("skip=000000", &format!("skip={lines:06}"));
    [header.as_bytes(), payload].concat()
}
fn tar(name: &str, body: &[u8]) -> Vec<u8> {
    let mut h = tar::Header::new_gnu();
    h.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
    h.set_size(body.len() as u64);
    h.set_mode(0o755);
    h.set_cksum();
    let mut bytes = h.as_bytes().to_vec();
    bytes.extend(body);
    bytes.resize(bytes.len().div_ceil(512) * 512, 0);
    bytes.extend([0; 1024]);
    bytes
}

#[test]
fn literal_offsets_line_counts_quotes_and_multiple_tar_segments() {
    let marker_payload = skip_script(&tar("MojoSetup", b"payload marker"), "");
    assert_eq!(
        parse_header(&marker_payload, marker_payload.len() as u64)
            .unwrap()
            .family,
        Family::Makeself
    );
    let payload = tar("file", b"payload");
    let bytes = skip_script(&payload, "label='Test Game'\n");
    let parsed = parse_header(&bytes, bytes.len() as u64).unwrap();
    assert_eq!(parsed.spans[0].offset, (bytes.len() - payload.len()) as u64);
    assert_eq!(parsed.label.as_deref(), Some("Test Game"));
    let header = "#!/bin/sh\n# Makeself\noffset=000000\nexit 92\n";
    let header = header.replace("offset=000000", &format!("offset={:06}", header.len()));
    let bytes = [header.as_bytes(), payload.as_slice()].concat();
    assert_eq!(
        parse_header(&bytes, bytes.len() as u64).unwrap().spans[0].offset,
        header.len() as u64
    );
    let second = tar("second", b"more");
    let bytes = skip_script(
        &[payload.as_slice(), second.as_slice()].concat(),
        &format!(
            "filesizes='{} {}'\ntotalsize={}\n",
            payload.len(),
            second.len(),
            payload.len() + second.len()
        ),
    );
    assert_eq!(
        parse_header(&bytes, bytes.len() as u64)
            .unwrap()
            .spans
            .len(),
        2
    );
    let root = TestRoot::new().unwrap();
    let mut dir = SafeDir::new(
        open_staging(root.path()).unwrap(),
        Limits::default(),
        bytes.len() as u64,
        true,
    );
    extract(
        &mut Cursor::new(bytes),
        &mut dir,
        &ArchiveOptions::default(),
        &mut NoopControl,
    )
    .unwrap();
    root.assert_sentinel_untouched();
    assert_eq!(std::fs::read(root.path().join("second")).unwrap(), b"more");
}

#[test]
fn dynamic_malformed_encrypted_and_hostile_headers_never_run_shell_code() {
    for header in [
        "#!/bin/sh\n# Makeself\noffset=$(touch ../sentinel)\n",
        "#!/bin/sh\n# Makeself\nskip=`touch ../sentinel`\n",
        "#!/bin/sh\n# Makeself\nskip=999999999999999999999999999\n",
        "#!/bin/sh\n# Makeself\noffset=999999999\n",
        "#!/bin/sh\n# Makeself\noffset=1\noffset=2\n",
        "#!/bin/sh\n# Makeself\nskip=100000\n",
        "#!/bin/sh\n# Makeself\nfilesizes='18446744073709551615 1'\nskip=5\n",
        "#!/bin/sh\n# Makeself\ndecrypt_cmd='openssl aes-256-cbc'\nskip=5\n",
    ] {
        let root = TestRoot::new().unwrap();
        let result = parse_header(header.as_bytes(), 4096);
        root.assert_clean();
        assert!(result.is_err(), "{header}");
    }
    let bytes = skip_script(&tar("../sentinel", b"changed"), "touch ../sentinel\n");
    let root = TestRoot::new().unwrap();
    let mut dir = SafeDir::new(
        open_staging(root.path()).unwrap(),
        Limits::default(),
        bytes.len() as u64,
        true,
    );
    let result = extract(
        &mut Cursor::new(bytes),
        &mut dir,
        &ArchiveOptions::default(),
        &mut NoopControl,
    );
    root.assert_clean();
    assert!(result.is_err());
}

#[test]
fn unknown_payload_fallback_is_explicit_execution_and_linux_only() {
    let bytes = skip_script(b"unsupported payload", "");
    let node = engine()
        .probe_reader(&mut Cursor::new(bytes), Path::new("game.run"))
        .unwrap()
        .root;
    assert_eq!(node.format, Format::Makeself);
    assert!(node.facts.contains_key("makeself.error"));
    let allowed = plans(&node, &host(), ExecutionPolicy::Allow);
    assert_eq!(allowed.candidates.len(), 1);
    assert!(allowed.candidates[0].requires.executes_code);
    assert!(
        plans(&node, &host(), ExecutionPolicy::Never)
            .candidates
            .is_empty()
    );
    for os in [
        caskade_installer::OperatingSystem::Windows,
        caskade_installer::OperatingSystem::Macos,
    ] {
        let mut host = host();
        host.os = os;
        assert!(
            plans(&node, &host, ExecutionPolicy::Allow)
                .candidates
                .is_empty()
        );
    }
}

#[test]
fn every_shell_fixture_truncation_is_a_clean_error_or_exact_tree() {
    for fixture in corpus() {
        let bytes = std::fs::read(fixture.input()).unwrap();
        for percent in [25, 50, 75, 99] {
            let bytes = &bytes[..bytes.len() * percent / 100];
            let root = TestRoot::new().unwrap();
            let mut dir = SafeDir::new(
                open_staging(root.path()).unwrap(),
                Limits::default(),
                bytes.len() as u64,
                true,
            );
            let result = extract(
                &mut Cursor::new(bytes),
                &mut dir,
                &ArchiveOptions::default(),
                &mut NoopControl,
            );
            root.assert_sentinel_untouched();
            if result.is_ok() {
                fixture.assert_tree(root.path()).unwrap();
            }
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(512))]
    #[test]
    fn arbitrary_header_bytes_never_produce_outside_or_overflowing_ranges(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4096), size in proptest::prelude::any::<u64>()) {
        if let Ok(header) = parse_header(&bytes, size) {
            proptest::prop_assert!(!header.spans.is_empty());
            proptest::prop_assert!(header.spans.len() <= 64);
            for span in header.spans {
                proptest::prop_assert!(span.offset > 0 && span.offset <= MAX_HEADER_BYTES as u64);
                proptest::prop_assert!(span.offset.checked_add(span.length).is_some_and(|end| end <= size));
            }
        }
    }
}
