use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Seek, SeekFrom},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[cfg(unix)]
use std::io::Write;

use caskade_installer::{
    ArtifactId, ArtifactNode, Format, PayloadPlatform, Role,
    probe::{
        CountingReader, Detection, FormatProbe, ProbeEngine, ProbeError, ProbeLimits, ProbeSource,
    },
};

fn samples() -> Vec<(&'static str, Vec<u8>, Format, PayloadPlatform)> {
    let unknown = PayloadPlatform::Unknown;
    let mut tar = vec![0; 512];
    tar[257..263].copy_from_slice(b"ustar\0");
    let mut elf = vec![0; 64];
    elf[..4].copy_from_slice(b"\x7fELF");
    let mut appimage = elf.clone();
    appimage[8..11].copy_from_slice(b"AI\x02");
    let mut pe = vec![0; 256];
    pe[..2].copy_from_slice(b"MZ");
    pe[60..64].copy_from_slice(&128_u32.to_le_bytes());
    pe[128..132].copy_from_slice(b"PE\0\0");
    let mut dmg = vec![0; 1024];
    dmg[512..516].copy_from_slice(b"koly");
    let mut iso = vec![0; 0x8800];
    iso[0x8001..0x8006].copy_from_slice(b"CD001");
    vec![
        ("zip", b"PK\x03\x04".to_vec(), Format::Zip, unknown.clone()),
        (
            "empty-zip",
            b"PK\x05\x06".to_vec(),
            Format::Zip,
            unknown.clone(),
        ),
        (
            "7z",
            b"7z\xbc\xaf\x27\x1c".to_vec(),
            Format::SevenZip,
            unknown.clone(),
        ),
        (
            "rar4",
            b"Rar!\x1a\x07\x00".to_vec(),
            Format::Rar,
            unknown.clone(),
        ),
        (
            "rar5",
            b"Rar!\x1a\x07\x01\x00".to_vec(),
            Format::Rar,
            unknown.clone(),
        ),
        ("xz", b"\xfd7zXZ\x00".to_vec(), Format::Xz, unknown.clone()),
        (
            "gzip",
            b"\x1f\x8b\x08".to_vec(),
            Format::Gzip,
            unknown.clone(),
        ),
        (
            "zstd",
            b"\x28\xb5\x2f\xfd".to_vec(),
            Format::Zstd,
            unknown.clone(),
        ),
        ("bzip2", b"BZh9".to_vec(), Format::Bzip2, unknown.clone()),
        ("tar", tar, Format::Tar, unknown.clone()),
        (
            "deb",
            b"!<arch>\ndebian-binary".to_vec(),
            Format::Deb,
            PayloadPlatform::Linux,
        ),
        (
            "rpm",
            b"\xed\xab\xee\xdb".to_vec(),
            Format::Rpm,
            PayloadPlatform::Linux,
        ),
        ("pkg", b"xar!".to_vec(), Format::Pkg, PayloadPlatform::Macos),
        ("cab", b"MSCF".to_vec(), Format::Cab, unknown.clone()),
        (
            "wim",
            b"MSWIM\0\0\0".to_vec(),
            Format::Wim,
            PayloadPlatform::Windows,
        ),
        ("elf", elf, Format::Elf, PayloadPlatform::Linux),
        (
            "appimage",
            appimage,
            Format::AppImage,
            PayloadPlatform::Linux,
        ),
        ("pe", pe, Format::Pe, PayloadPlatform::Windows),
        (
            "macho-be",
            b"\xfe\xed\xfa\xce".to_vec(),
            Format::MachO,
            PayloadPlatform::Macos,
        ),
        (
            "macho-le",
            b"\xce\xfa\xed\xfe".to_vec(),
            Format::MachO,
            PayloadPlatform::Macos,
        ),
        (
            "macho64-be",
            b"\xfe\xed\xfa\xcf".to_vec(),
            Format::MachO,
            PayloadPlatform::Macos,
        ),
        (
            "macho64-le",
            b"\xcf\xfa\xed\xfe".to_vec(),
            Format::MachO,
            PayloadPlatform::Macos,
        ),
        (
            "macho-fat",
            b"\xca\xfe\xba\xbe".to_vec(),
            Format::MachO,
            PayloadPlatform::Macos,
        ),
        (
            "macho-fat-swapped",
            b"\xbe\xba\xfe\xca".to_vec(),
            Format::MachO,
            PayloadPlatform::Macos,
        ),
        (
            "macho-fat64",
            b"\xca\xfe\xba\xbf".to_vec(),
            Format::MachO,
            PayloadPlatform::Macos,
        ),
        (
            "macho-fat64-swapped",
            b"\xbf\xba\xfe\xca".to_vec(),
            Format::MachO,
            PayloadPlatform::Macos,
        ),
        (
            "script",
            b"#!/bin/sh\nexit 0\n".to_vec(),
            Format::Script,
            PayloadPlatform::Linux,
        ),
        ("dmg", dmg, Format::Dmg, PayloadPlatform::Macos),
        ("iso", iso, Format::Iso, unknown.clone()),
        ("chd", b"MComprHD".to_vec(), Format::Chd, unknown.clone()),
        (
            "nes",
            b"NES\x1a".to_vec(),
            Format::Rom,
            PayloadPlatform::ConsoleRom {
                platform: Some("nes".into()),
            },
        ),
        (
            "unknown",
            b"unrecognised input".to_vec(),
            Format::Unknown,
            unknown,
        ),
    ]
}

#[test]
fn all_magic_families_are_static_and_snapshot_tested() {
    let engine = ProbeEngine::default();
    let mut snapshots = BTreeMap::new();
    for (name, bytes, format, platform) in samples() {
        let probe = engine
            .probe_reader(&mut Cursor::new(bytes), Path::new(name))
            .unwrap();
        assert_eq!(probe.root.format, format, "{name}");
        assert_eq!(probe.root.platform, platform, "{name}");
        snapshots.insert(name, probe);
    }
    insta::assert_json_snapshot!("magic_table", snapshots);
}

#[test]
fn every_committed_fixture_binary_has_a_probe_snapshot() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut snapshots = BTreeMap::new();
    for directory in ["bin", "handmade"] {
        for entry in fs::read_dir(root.join(directory)).expect("fixture directory missing") {
            let path = entry.unwrap().path();
            if !path.is_file() || path.extension().is_some_and(|ext| ext == "md") {
                continue;
            }
            let name = format!(
                "{directory}/{}",
                path.file_name().unwrap().to_string_lossy()
            );
            let mut file = fs::File::open(&path).unwrap();
            let probe = ProbeEngine::default()
                .probe_reader(&mut file, Path::new(&name))
                .unwrap();
            assert!(probe.io.bytes_read < 16 * 1024 * 1024, "{name}");
            snapshots.insert(name, probe);
        }
    }
    assert!(
        snapshots.len() >= 15,
        "committed fixtures must not be skipped"
    );
    // Every expectation file must reference an input covered by this inventory.
    for entry in fs::read_dir(root.join("expected")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            let data: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
            assert!(snapshots.contains_key(data["input"].as_str().unwrap()));
        }
    }
    insta::assert_json_snapshot!("committed_fixtures", snapshots);
}

#[cfg(unix)]
#[test]
fn twenty_gib_sparse_file_reads_less_than_sixteen_mib() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.zip");
    let mut file = fs::File::create(&path).unwrap();
    file.write_all(b"PK\x03\x04").unwrap();
    file.set_len(20 * 1024 * 1024 * 1024).unwrap();
    drop(file);
    let result = ProbeEngine::default().probe_path(&path).unwrap();
    assert_eq!(result.root.format, Format::Zip);
    assert_eq!(result.root.size, Some(20 * 1024 * 1024 * 1024));
    assert_eq!(result.io.bytes_read, 4096);
    assert!(result.io.bytes_read < 16 * 1024 * 1024);
}

#[test]
fn roles_and_platforms_are_independent_of_host_and_extension() {
    let elf = samples()
        .into_iter()
        .find(|(name, _, _, _)| *name == "elf")
        .unwrap()
        .1;
    for (name, role) in [
        ("setup.exe", Role::Installer),
        ("unins000.exe", Role::Uninstaller),
        ("vcredist_x64.exe", Role::Redistributable),
        ("UnityLauncher.exe", Role::Launcher),
        ("README.txt", Role::Document),
        ("Game.exe", Role::Application),
    ] {
        let probe = ProbeEngine::default()
            .probe_reader(&mut Cursor::new(&elf), Path::new(name))
            .unwrap();
        assert_eq!(probe.root.role, role, "{name}");
        assert_eq!(probe.root.format, Format::Elf); // Extension never determines format.
        assert_eq!(probe.root.platform, PayloadPlatform::Linux);
    }
}

fn node(id: &str, format: Format, complete: bool) -> ArtifactNode {
    ArtifactNode {
        id: ArtifactId(id.into()),
        path: "child".into(),
        format,
        platform: PayloadPlatform::Unknown,
        role: Role::Unknown,
        size: None,
        facts: BTreeMap::new(),
        children: vec![],
        children_complete: complete,
    }
}

struct ListedZip {
    listed: Arc<AtomicUsize>,
    children: Vec<ArtifactNode>,
}
impl FormatProbe for ListedZip {
    fn id(&self) -> &str {
        "test.zip"
    }
    fn list(
        &self,
        source: &mut ProbeSource<'_>,
        node: &ArtifactNode,
    ) -> Result<Option<Vec<ArtifactNode>>, ProbeError> {
        if node.format != Format::Zip {
            return Ok(None);
        }
        assert_eq!(source.read_at(0, 2)?, b"PK");
        self.listed.fetch_add(1, Ordering::SeqCst);
        Ok(Some(self.children.clone()))
    }
}

#[test]
fn containers_are_listed_only_on_request_and_then_cached() {
    let listed = Arc::new(AtomicUsize::new(0));
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(ListedZip {
        listed: listed.clone(),
        children: vec![node("source:0/game", Format::Pe, true)],
    }));
    let mut bytes = Cursor::new(b"PK\x03\x04");
    let mut probe = engine
        .probe_reader(&mut bytes, Path::new("game.zip"))
        .unwrap();
    assert_eq!(listed.load(Ordering::SeqCst), 0);
    assert!(probe.root.children.is_empty());
    assert!(!probe.root.children_complete);
    let id = probe.root.id.clone();
    let stats = engine
        .list_children(&mut bytes, &mut probe.root, &id)
        .unwrap();
    assert_eq!(stats.bytes_read, 2);
    assert_eq!(probe.root.children.len(), 1);
    assert!(probe.root.children_complete);
    assert_eq!(
        engine
            .list_children(&mut bytes, &mut probe.root, &id)
            .unwrap()
            .bytes_read,
        0
    );
    assert_eq!(listed.load(Ordering::SeqCst), 1);
}

#[test]
fn missing_lister_returns_clear_error_without_changing_tree() {
    let engine = ProbeEngine::default();
    let mut bytes = Cursor::new(b"PK\x03\x04");
    let mut probe = engine
        .probe_reader(&mut bytes, Path::new("game.zip"))
        .unwrap();
    let original = probe.root.clone();
    let id = original.id.clone();
    assert!(matches!(
        engine.list_children(&mut bytes, &mut probe.root, &id),
        Err(ProbeError::ListingUnavailable {
            format: Format::Zip
        })
    ));
    assert_eq!(probe.root, original);
}

#[test]
fn listing_rejects_depth_count_and_duplicate_ids_transactionally() {
    for (limits, children) in [
        (
            ProbeLimits {
                max_depth: 0,
                ..ProbeLimits::default()
            },
            vec![node("child", Format::Pe, true)],
        ),
        (
            ProbeLimits {
                max_children: 0,
                ..ProbeLimits::default()
            },
            vec![node("child", Format::Pe, true)],
        ),
        (
            ProbeLimits::default(),
            vec![node("source:0", Format::Pe, true)],
        ),
    ] {
        let mut engine = ProbeEngine::new(limits);
        engine.register(Box::new(ListedZip {
            listed: Arc::default(),
            children,
        }));
        let mut bytes = Cursor::new(b"PK\x03\x04");
        let mut root = engine
            .probe_reader(&mut bytes, Path::new("game.zip"))
            .unwrap()
            .root;
        let original = root.clone();
        let id = root.id.clone();
        assert!(engine.list_children(&mut bytes, &mut root, &id).is_err());
        assert_eq!(root, original);
    }
    // The node's real depth is derived from the tree, so the caller cannot reset it.
    let engine = ProbeEngine::new(ProbeLimits {
        max_depth: 1,
        ..ProbeLimits::default()
    });
    let mut root = node("root", Format::Zip, true);
    root.children.push(node("nested", Format::Zip, false));
    assert!(matches!(
        engine.list_children(
            &mut Cursor::new(b"PK"),
            &mut root,
            &ArtifactId("nested".into())
        ),
        Err(ProbeError::DepthLimit { limit: 1 })
    ));
}

struct CustomProbe;
impl FormatProbe for CustomProbe {
    fn id(&self) -> &str {
        "custom.detector"
    }
    fn detect(&self, source: &mut ProbeSource<'_>) -> Result<Option<Detection>, ProbeError> {
        if source.read_at(0, 3)? != b"SFX" {
            return Ok(None);
        }
        let mut detected = Detection::new(Format::Inno, PayloadPlatform::Windows, true);
        detected.role = Some(Role::Installer);
        detected
            .facts
            .insert("inno.app_name".into(), "Fixture Game".into());
        Ok(Some(detected))
    }
}

#[test]
fn registered_probes_override_generic_magic_with_counted_reads() {
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(CustomProbe));
    let probe = engine
        .probe_reader(&mut Cursor::new(b"SFXstuff"), Path::new("unknown.bin"))
        .unwrap();
    assert_eq!(probe.root.format, Format::Inno);
    assert_eq!(probe.root.facts["probe.handler"], "custom.detector");
    assert_eq!(probe.root.facts["inno.app_name"], "Fixture Game");
    assert_eq!(probe.io.bytes_read, 3);
    assert!(!probe.root.children_complete);
}

#[test]
fn read_view_rejects_budget_and_offsets_before_allocation() {
    let mut reader = CountingReader::new(Cursor::new(vec![0; 128]));
    {
        let mut source = ProbeSource::new(&mut reader, 16).unwrap();
        assert!(matches!(
            source.read_at(0, usize::MAX),
            Err(ProbeError::ReadBudgetExceeded { limit: 16 })
        ));
        assert!(matches!(
            source.read_at(129, 1),
            Err(ProbeError::InvalidOffset { .. })
        ));
        assert_eq!(source.read_at(0, 10).unwrap().len(), 10);
        assert!(matches!(
            source.read_at(0, 7),
            Err(ProbeError::ReadBudgetExceeded { .. })
        ));
        assert_eq!(source.read_at(128, usize::MAX).unwrap().len(), 0);
    }
    assert_eq!(reader.stats().bytes_read, 10);
}

#[test]
fn standard_read_seek_interface_shares_the_same_budget() {
    let mut reader = Cursor::new(b"abcdefgh");
    let mut source = ProbeSource::new(&mut reader, 4).unwrap();
    source.seek(SeekFrom::Start(0)).unwrap();
    let mut buffer = [0; 4];
    source.read_exact(&mut buffer).unwrap();
    assert_eq!(&buffer, b"abcd");
    assert_eq!(source.stats().bytes_read, 4);
    assert!(source.read(&mut buffer).is_err());
    source.seek(SeekFrom::End(0)).unwrap();
    assert_eq!(source.read(&mut buffer).unwrap(), 0);
    assert!(matches!(
        source.read_at(0, 1),
        Err(ProbeError::ReadBudgetExceeded { limit: 4 })
    ));
}

#[test]
fn truncated_magic_and_malformed_pe_offsets_never_panic() {
    let engine = ProbeEngine::default();
    for (_, bytes, _, _) in samples() {
        for length in [0, 1, 2, bytes.len() / 2, bytes.len().saturating_sub(1)] {
            let _ = engine.probe_reader(
                &mut Cursor::new(&bytes[..length.min(bytes.len())]),
                Path::new("truncated"),
            );
        }
    }
    let mut mz = vec![0; 64];
    mz[..2].copy_from_slice(b"MZ");
    mz[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        engine
            .probe_reader(&mut Cursor::new(mz), Path::new("fake.exe"))
            .unwrap()
            .root
            .format,
        Format::Unknown
    );
}

#[test]
fn directory_probe_does_not_walk_or_read_its_children() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("setup.exe"), b"not read").unwrap();
    let result = ProbeEngine::default().probe_path(dir.path()).unwrap();
    assert_eq!(result.root.format, Format::Directory);
    assert!(result.root.children.is_empty());
    assert!(!result.root.children_complete);
    assert_eq!(result.io.bytes_read, 0);
}

#[cfg(unix)]
#[test]
fn path_adapter_rejects_special_files_before_opening() {
    assert!(matches!(
        ProbeEngine::default().probe_path(Path::new("/dev/null")),
        Err(ProbeError::UnsupportedFileType { .. })
    ));
}

#[test]
fn reader_errors_are_reported_without_panics() {
    struct Broken(Cursor<Vec<u8>>);
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken"))
        }
    }
    impl Seek for Broken {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(pos)
        }
    }
    assert!(matches!(
        ProbeEngine::default()
            .probe_reader(&mut Broken(Cursor::new(vec![0; 8])), Path::new("broken")),
        Err(ProbeError::Io(_))
    ));
}
