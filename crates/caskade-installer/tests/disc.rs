#![cfg(feature = "disc")]
#[cfg(feature = "windows-formats")]
use crate::fixtures;
use caskade_installer::{
    Format, HandOff, InstallOutcome,
    formats::{
        archive::{NoopControl, sevenzip::SevenZip},
        disc::{DataTrack, DiscKind, DiscProbe, DiscSession, SectorReader, parse_cue},
    },
    probe::{ProbeEngine, ProbeLimits},
    safety::Limits,
};
use std::{
    io::{Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[test]
fn cue_parser_confines_binary_tracks_and_requires_a_valid_data_index() {
    let track = parse_cue("FILE \"Disc One.bin\" BINARY\n TRACK 01 MODE2/2352\n INDEX 00 00:00:00\n INDEX 01 00:02:00\n TRACK 02 AUDIO\n INDEX 01 12:00:00\n").unwrap();
    assert_eq!(track.file, Path::new("Disc One.bin"));
    assert_eq!(
        (track.sector_bytes, track.data_offset, track.start_sector),
        (2352, 24, 150)
    );
    for bad in [
        "FILE \"../outside.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00",
        "FILE \"C:\\outside.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00",
        "FILE \"disc.bin\" BINARY\nTRACK 01 MODE2/2352\nINDEX 01 00:60:00",
        "FILE \"disc.bin\" BINARY\nTRACK 01 MODE2/2352\nINDEX 01 00:00:75",
        "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048",
        "FILE \"disc.wav\" WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00",
        "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nTRACK 02 MODE1/2048\nINDEX 01 00:00:00",
    ] {
        assert!(parse_cue(bad).is_err(), "{bad}");
    }
}
fn raw_sector(value: u8) -> Vec<u8> {
    let mut bytes = vec![0; 2352];
    bytes[1..11].fill(255);
    bytes[15] = 2;
    bytes[24..2072].fill(value);
    bytes
}
#[test]
fn raw_sector_view_reads_only_form1_data_and_bounds_seeks() {
    let mut raw = raw_sector(0x11);
    raw.extend(raw_sector(0x22));
    let track = DataTrack {
        file: "disc.bin".into(),
        sector_bytes: 2352,
        data_offset: 24,
        start_sector: 0,
    };
    let mut view = SectorReader::new(Cursor::new(raw.clone()), track.clone()).unwrap();
    assert_eq!(view.seek(SeekFrom::End(0)).unwrap(), 4096);
    view.seek(SeekFrom::Start(2044)).unwrap();
    let mut crossing = [0; 8];
    view.read_exact(&mut crossing).unwrap();
    assert_eq!(crossing, [0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0x22, 0x22]);
    for seek in [
        SeekFrom::Start(u64::MAX),
        SeekFrom::End(1),
        SeekFrom::Current(i64::MIN),
    ] {
        assert!(view.seek(seek).is_err());
    }
    let mut form2 = raw_sector(0);
    form2[18] = 0x20;
    form2[22] = 0x20;
    assert!(
        SectorReader::new(Cursor::new(form2), track.clone())
            .unwrap()
            .read(&mut [0; 1])
            .is_err()
    );
    raw.pop();
    assert!(SectorReader::new(Cursor::new(raw), track.clone()).is_err());
    assert!(
        SectorReader::new(
            Cursor::new(vec![]),
            DataTrack {
                start_sector: u64::MAX,
                ..track
            }
        )
        .is_err()
    );
}
fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/disc")
        .join(name)
}
fn engine() -> ProbeEngine {
    let mut engine = ProbeEngine::new(ProbeLimits::default());
    engine.register(Box::new(DiscProbe));
    engine
}
#[test]
fn disc_probe_identifies_real_iso_cue_and_udf_descriptors_without_a_sidecar() {
    for (name, format) in [
        ("iso-pc-disc.iso", Format::Iso),
        ("ps1-cue-bin.cue", Format::Cue),
        ("ps1-cue-bin.bin", Format::Iso),
    ] {
        let node = engine().probe_path(&fixture_path(name)).unwrap();
        assert_eq!(node.root.format, format);
        assert!(node.io.bytes_read < 4096);
    }
    let mut udf = vec![0; 34 * 2048];
    udf[17 * 2048 + 1..17 * 2048 + 6].copy_from_slice(b"NSR03");
    let node = engine()
        .probe_reader(&mut Cursor::new(udf), Path::new("disc.udf"))
        .unwrap()
        .root;
    assert_eq!(node.format, Format::Iso);
    assert_eq!(node.facts["disc.filesystem"], "udf");
    assert_ne!(
        engine()
            .probe_reader(
                &mut Cursor::new(b"FILE ../evil BINARY"),
                Path::new("evil.cue")
            )
            .unwrap()
            .root
            .format,
        Format::Cue
    );
}
fn tool() -> SevenZip {
    SevenZip::pinned(std::env::var_os("CASKADE_7ZZ").expect("set pinned CASKADE_7ZZ")).unwrap()
}
#[test]
#[ignore = "requires CASKADE_7ZZ pointing to the pinned official sidecar"]
fn ps1_cue_bin_handoff_uses_platform_definition_and_retains_every_member() {
    let scratch = tempfile::tempdir().unwrap();
    let session = DiscSession::open(
        &fixture_path("ps1-cue-bin.bin"),
        &tool(),
        scratch.path(),
        Limits::default(),
        &mut NoopControl,
    )
    .unwrap();
    assert_eq!(
        session.kind,
        DiscKind::Console {
            platform: "sony_playstation".into()
        }
    );
    assert_eq!(
        session.handoff(),
        Some(InstallOutcome::HandOff(HandOff::Rom {
            platform: "sony_playstation".into(),
            files: vec!["ps1-cue-bin.cue".into(), "ps1-cue-bin.bin".into()]
        }))
    );
    assert!(session.installer().is_err());
    assert!(
        session
            .artifacts
            .iter()
            .any(|n| n.path == Path::new("SYSTEM.CNF"))
    );
    let contents = session.contents().to_owned();
    drop(session);
    assert!(!contents.exists());
    assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
}
#[test]
#[ignore = "requires CASKADE_7ZZ pointing to the pinned official sidecar"]
fn pc_multidisc_set_is_retained_and_next_disc_uses_interactive_fallback() {
    let inputs = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    for n in 1..=2 {
        std::fs::copy(
            fixture_path("iso-pc-disc.iso"),
            inputs.path().join(format!("Game (Disc {n}).iso")),
        )
        .unwrap();
    }
    let session = DiscSession::open(
        &inputs.path().join("Game (Disc 2).iso"),
        &tool(),
        scratch.path(),
        Limits::default(),
        &mut NoopControl,
    )
    .unwrap();
    assert_eq!(session.kind, DiscKind::Pc);
    assert_eq!(
        session.files,
        vec![
            Path::new("Game (Disc 1).iso"),
            Path::new("Game (Disc 2).iso")
        ]
    );
    assert_eq!(session.installer().unwrap().format, Format::Inno);
    assert_eq!(session.warnings()[0].code, "disc_interactive_fallback");
    assert_eq!(std::fs::read_dir(inputs.path()).unwrap().count(), 2);
}
#[cfg(feature = "windows-formats")]
#[tokio::test]
#[ignore = "requires CASKADE_7ZZ pointing to the pinned official sidecar"]
async fn genuine_pc_iso_installs_nested_inno_via_the_real_executor_to_its_golden_tree() {
    use caskade_installer::{
        exec::{CancellationToken, ExecRequest, Executor},
        formats::windows::inno::{InnoExtractor, InnoHandler},
        host::HostCaps,
        plan::{PlanEngine, Policy},
        testing::{FakeDiskSpace, FakeRuntime, RecordingSink, TestRoot},
    };
    use std::sync::Arc;
    let scratch = tempfile::tempdir().unwrap();
    for image in ["iso-pc-disc.iso", "udf-pc-disc.iso"] {
        let session = DiscSession::open(
            &fixture_path(image),
            &tool(),
            scratch.path(),
            Limits::default(),
            &mut NoopControl,
        )
        .unwrap();
        assert_eq!(session.kind, DiscKind::Pc);
        assert!(session.handoff().is_none());
        let installer = session.installer().unwrap();
        assert_eq!(installer.format, Format::Inno);
        let host: HostCaps = toml::from_str(include_str!("hosts/linux.toml")).unwrap();
        let mut planner = PlanEngine::new();
        planner.register(Box::new(InnoHandler)).unwrap();
        let plans = planner
            .plan(std::slice::from_ref(installer), &host, &Policy::default())
            .unwrap();
        let root = TestRoot::new().unwrap();
        let sources = session.sources();
        let extractor = Arc::new(InnoExtractor::new(sources.clone()));
        let report = Executor::new(
            Arc::new(FakeRuntime::default()),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        )
        .with_extractor(extractor)
        .execute(
            ExecRequest {
                root: root.path().into(),
                relative_dir: "Game".into(),
                job_id: "pc-disc".into(),
                plans,
                sources,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let fixture =
            fixtures::load_windows(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
                .unwrap()
                .into_iter()
                .find(|f| f.expected.id == "inno60-basic")
                .unwrap();
        fixture.assert_tree(&report.dir).unwrap();
        root.assert_sentinel_untouched();
        assert!(!report.dir.join("setup.exe").exists());
        std::fs::remove_dir_all(report.dir).unwrap();
        root.assert_clean();
    }
}
