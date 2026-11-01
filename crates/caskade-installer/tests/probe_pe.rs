use caskade_installer::{
    Format, Role,
    probe::{PeMetadata, PeProbe, ProbeEngine},
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Seek, SeekFrom, Write},
    path::Path,
};
#[path = "support/pe.rs"]
mod support;
fn engine() -> ProbeEngine {
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(PeProbe::new()));
    engine
}
fn probe(bytes: &[u8], name: &str) -> caskade_installer::probe::Probe {
    engine()
        .probe_reader(&mut Cursor::new(bytes), Path::new(name))
        .unwrap()
}
#[test]
fn every_existing_windows_fixture_and_layout_executable_is_snapshotted() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin");
    let mut outputs = BTreeMap::new();
    let bytes = fs::read(base.join("fixture-game-windows-x86_64.exe")).unwrap();
    let result = probe(&bytes, "fixture-game-windows-x86_64.exe");
    assert_eq!(result.root.role, Role::Application);
    assert_eq!(result.root.format, Format::Pe);
    outputs.insert("fixture-game-windows-x86_64.exe".to_owned(), result);
    for archive in ["unity.zip", "unreal.zip", "gog.zip"] {
        let mut zip = zip::ZipArchive::new(fs::File::open(base.join(archive)).unwrap()).unwrap();
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).unwrap();
            let name = file.name().unwrap().into_owned();
            if !name.ends_with(".exe") {
                continue;
            }
            assert!(file.size() < 4 * 1024 * 1024);
            let mut bytes = vec![];
            file.read_to_end(&mut bytes).unwrap();
            outputs.insert(format!("{archive}/{name}"), probe(&bytes, &name));
        }
    }
    insta::assert_json_snapshot!("windows_fixtures", outputs);
}
#[test]
fn synthetic_engine_signatures_sfx_and_roles_are_snapshotted() {
    let mut outputs = BTreeMap::new();
    for (label, signature, expected) in [
        (
            "inno",
            b"Inno Setup Setup Data (6.4.0)".as_slice(),
            Format::Inno,
        ),
        ("nsis", b"\xef\xbe\xad\xdeNullsoftInst", Format::Nsis),
        ("installshield", b"InstallShield", Format::InstallShield),
        ("wise", b"Wise Installation Wizard", Format::Wise),
        ("setupfactory", b"Setup Factory", Format::SetupFactory),
    ] {
        for resource in [false, true] {
            let resources = if resource {
                vec![(10, signature.to_vec())]
            } else {
                vec![]
            };
            let bytes = support::image(
                0x8664,
                if resource { ".rsrc" } else { ".text" },
                &resources,
                if resource { b"" } else { signature },
            );
            let result = probe(&bytes, "setup.exe");
            assert_eq!(result.root.format, expected);
            assert_eq!(result.root.role, Role::Installer);
            outputs.insert(
                format!("{label}/{}", if resource { "resource" } else { "overlay" }),
                result,
            );
        }
    }
    let burn = probe(&support::image(0x8664, ".wixburn", &[], b""), "setup.exe");
    assert_eq!(burn.root.format, Format::Burn);
    outputs.insert("burn".into(), burn);
    for (name, signature) in [
        ("zip", b"PK\x03\x04payload".as_slice()),
        ("seven_zip", b"7z\xbc\xaf\x27\x1cpayload"),
        ("rar", b"Rar!\x1a\x07\x01\x00payload"),
    ] {
        let result = probe(&support::image(0x8664, ".text", &[], signature), "sfx.exe");
        assert_eq!(result.root.role, Role::Installer);
        assert_eq!(result.root.facts["pe.sfx_format"], name);
        outputs.insert(format!("sfx/{name}"), result);
    }
    for (name, role) in [
        ("Game.exe", Role::Application),
        ("setup.exe", Role::Application),
        ("unins000.exe", Role::Uninstaller),
        ("vc_redist.x64.exe", Role::Redistributable),
        ("GameLauncher.exe", Role::Launcher),
    ] {
        let result = probe(&support::image(0x14c, ".text", &[], b""), name);
        assert_eq!(result.root.role, role);
        outputs.insert(format!("role/{name}"), result);
    }
    insta::assert_json_snapshot!("synthetic_pe", outputs);
}
#[test]
fn resources_version_strings_and_utf8_utf16_execution_level_are_parsed() {
    for utf16 in [false, true] {
        let xml = "<?xml version='1.0'?><assembly><trustInfo><security><requestedPrivileges><requestedExecutionLevel level='requireAdministrator' uiAccess='false'/></requestedPrivileges></security></trustInfo></assembly>";
        let manifest = if utf16 {
            [
                vec![0xff, 0xfe],
                xml.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            ]
            .concat()
        } else {
            xml.as_bytes().to_vec()
        };
        let version = support::version(&[
            ("ProductName", "Fixture Product"),
            ("FileDescription", "Game executable"),
            ("CompanyName", "Caskade"),
            ("FileVersion", "1.2.3.4"),
        ]);
        let bytes = support::image(0x8664, ".rsrc", &[(16, version), (24, manifest)], b"");
        let result = probe(&bytes, "Game.exe");
        let metadata: PeMetadata =
            serde_json::from_value(result.root.facts["pe.metadata"].clone()).unwrap();
        assert_eq!(
            metadata.requested_execution_level.as_deref(),
            Some("requireAdministrator")
        );
        assert_eq!(
            metadata.version_info["040904b0/ProductName"],
            "Fixture Product"
        );
        assert_eq!(metadata.subsystem, 2);
        assert_eq!(metadata.architecture, "x86_64");
        assert_eq!(result.root.role, Role::Application);
    }
}
#[test]
fn certificate_without_payload_and_large_sparse_overlay_stay_bounded() {
    let mut bytes = support::image(
        0x8664,
        ".text",
        &[],
        b"Inno Setup Setup Data (certificate text)",
    );
    let certificate = 1024;
    let cert_len = bytes.len() - certificate;
    support::put32(&mut bytes, 152 + 112 + 32, certificate as u32);
    support::put32(&mut bytes, 152 + 112 + 36, cert_len as u32);
    let result = probe(&bytes, "Game.exe");
    assert_eq!(result.root.role, Role::Application);
    assert_eq!(result.root.facts["pe.metadata"]["overlay_size"], 0);
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&support::image(
        0x8664,
        ".text",
        &[],
        b"Inno Setup Setup Data (",
    ))
    .unwrap();
    file.set_len(20 * 1024 * 1024 * 1024).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    let result = engine()
        .probe_reader(&mut file, Path::new("large.exe"))
        .unwrap();
    assert_eq!(result.root.format, Format::Inno);
    assert!(result.io.bytes_read < 2 * 1024 * 1024);
    assert_eq!(
        result.root.facts["pe.metadata"]["signature_scan_truncated"],
        true
    );
}
#[test]
fn malformed_resources_and_optional_headers_never_escape_bounds() {
    let bytes = support::image(0x8664, ".rsrc", &[(24, b"<broken>".to_vec())], b"");
    let result = probe(&bytes, "Game.exe");
    assert!(
        result.root.facts["pe.metadata"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w == "malformed application manifest")
    );
    for at in [
        60,
        148,
        152 + 60,
        152 + 112 + 16,
        152 + 112 + 20,
        392 + 16,
        392 + 20,
    ] {
        let mut broken = bytes.clone();
        support::put32(&mut broken, at, u32::MAX);
        let result = engine().probe_reader(&mut Cursor::new(broken), Path::new("fuzz.exe"));
        assert!(result.is_ok());
    }
}
proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(128))]
    #[test]
    fn arbitrary_headers_do_not_panic(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..8192)) {
        let mut source = Cursor::new(bytes); let _ = engine().probe_reader(&mut source, Path::new("fuzz.exe"));
    }
}

#[test]
fn zip_sfx_preserves_pe_metadata_and_lists_lazily() {
    use caskade_installer::probe::ProbeLimits;
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    zip.start_file("payload/start.sh", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"#!/bin/sh\n").unwrap();
    let bytes = support::image(0x8664, ".text", &[], &zip.finish().unwrap().into_inner());
    let engine = ProbeEngine::with_builtin_formats(ProbeLimits::default());
    let mut reader = Cursor::new(bytes);
    let mut probe = engine
        .probe_reader(&mut reader, Path::new("sfx.exe"))
        .unwrap();
    assert_eq!(probe.root.format, Format::Pe);
    assert_eq!(probe.root.facts["pe.sfx_format"], "zip");
    assert!(!probe.root.children_complete);
    let id = probe.root.id.clone();
    engine
        .list_children(&mut reader, &mut probe.root, &id)
        .unwrap();
    assert_eq!(probe.root.children[0].path, Path::new("payload/start.sh"));
    assert!(probe.root.children_complete);
}

/// #21 robustness report: a 4-aligned `e_lfanew` with `SizeOfOptionalHeader`
/// 0x66 made pelite build an unaligned section-header slice.
#[test]
fn regression_21_unaligned_optional_header_is_declined_without_panicking() {
    use caskade_installer::{
        discover::{DiscoveryHints, discover},
        host::HostCaps,
        probe::ProbeLimits,
    };
    let bytes = include_bytes!("regressions/21-pe-unaligned-optional-header.exe");
    let engine = ProbeEngine::with_builtin_formats(ProbeLimits::default());
    let result = engine
        .probe_reader(&mut Cursor::new(bytes), Path::new("setup.exe"))
        .unwrap();
    // Magic still names it PE; the header inspector declines to add facts.
    assert!(!result.root.facts.keys().any(|key| key.starts_with("pe.")));

    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Game.exe"), bytes).unwrap();
    let host: HostCaps = toml::from_str(include_str!("hosts/windows.toml")).unwrap();
    let _ = discover(root.path(), &host, &DiscoveryHints::default());
}

use crate::fixtures;

/// #23 AC: PE probe output for every genuine Windows fixture (#34). Builder
/// families must be recognised as installers; nothing here may be guessed
/// as an application, while the game executable itself stays one.
#[test]
fn every_genuine_windows_fixture_has_a_pe_probe_snapshot() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut outputs = BTreeMap::new();
    for fixture in fixtures::load_windows(&root).unwrap() {
        let input = fixture.input();
        let name = input.file_name().unwrap().to_str().unwrap().to_owned();
        let bytes = fs::read(&input).unwrap();
        let result = probe(&bytes, &name);
        let expected = match fixture.expected.format.as_str() {
            "inno" => Some(Format::Inno),
            "nsis" => Some(Format::Nsis),
            _ => None,
        };
        if let Some(format) = expected {
            assert_eq!(result.root.role, Role::Installer, "{}", fixture.expected.id);
            assert_eq!(result.root.format, format, "{}", fixture.expected.id);
        }
        if fixture.expected.format.ends_with("-sfx") {
            assert_eq!(result.root.format, Format::Pe, "{}", fixture.expected.id);
            assert!(
                result.root.facts.contains_key("pe.sfx_format"),
                "{}",
                fixture.expected.id
            );
        }
        outputs.insert(fixture.expected.id.clone(), result);
    }
    assert_eq!(outputs.len(), 33);
    let game = fs::read(root.join("bin/fixture-game-windows-x86_64.exe")).unwrap();
    assert_eq!(probe(&game, "Game.exe").root.role, Role::Application);
    insta::assert_json_snapshot!("genuine_windows_fixtures", outputs);
}
