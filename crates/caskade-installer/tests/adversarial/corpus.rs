//! Truncated and disguised fixture archives.
use super::*;

fn corpus() -> Vec<fixtures::Fixture> {
    fixtures::load_all(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")).unwrap()
}

#[test]
fn every_native_fixture_truncated_at_25_50_75_99_percent_is_confined() {
    let mut cases = 0;
    for fixture in corpus()
        .into_iter()
        .filter(|f| !f.expected.format.starts_with("rar"))
    {
        let bytes: Vec<_> = fixture
            .parts()
            .iter()
            .flat_map(|p| fs::read(p).unwrap())
            .collect();
        for percent in [25, 50, 75, 99] {
            let truncated = &bytes[..bytes.len() * percent / 100];
            let (root, result) = attempt(
                truncated,
                format(&fixture.expected.format),
                Limits::default(),
                &ArchiveOptions::default(),
                true,
            );
            if result.is_ok() {
                // TAR end padding can be truncated without losing a member.
                // Success must still equal the entire immutable expected tree.
                fixture
                    .assert_tree(root.path())
                    .unwrap_or_else(|e| panic!("{} {percent}%: {e}", fixture.expected.id));
            }
            cases += 1;
        }
    }
    assert_eq!(cases, 48);
}

#[test]
fn wrong_extensions_html_executables_and_prepended_zip_are_static_and_confined() {
    use caskade_installer::probe::ProbeEngine;
    let root = TestRoot::new().unwrap();
    let engine = ProbeEngine::with_builtin_formats(Default::default());
    let zip = raw_zip(&[ZipEntry::file(b"game.sh", b"#!/bin/sh\nexit 0\n")]);
    let detected = engine
        .probe_reader(&mut Cursor::new(&zip), Path::new("download.exe"))
        .unwrap();
    assert_eq!(detected.root.format, Format::Zip);
    let html = b"<!doctype html><script>location='sentinel'</script>";
    let detected = engine
        .probe_reader(&mut Cursor::new(html), Path::new("setup.exe"))
        .unwrap();
    assert_eq!(detected.root.format, Format::Unknown);
    rejected(html, Format::Zip);
    // ZIP offsets are relative to its original start, with a PE-shaped prefix.
    // Either a static PE classification or ZIP recognition is safe; never run it.
    let mut polyglot = vec![0; 512];
    polyglot[..2].copy_from_slice(b"MZ");
    u32_at(&mut polyglot, 0x3c, 0x80);
    polyglot[0x80..0x84].copy_from_slice(b"PE\0\0");
    polyglot.extend(&zip);
    let detected = engine
        .probe_reader(&mut Cursor::new(&polyglot), Path::new("setup.zip"))
        .unwrap();
    assert!(matches!(detected.root.format, Format::Pe | Format::Zip));
    let (out, result) = attempt(
        &polyglot,
        Format::Zip,
        Limits::default(),
        &ArchiveOptions::default(),
        true,
    );
    result.unwrap();
    assert_eq!(
        fs::read(out.path().join("game.sh")).unwrap(),
        b"#!/bin/sh\nexit 0\n"
    );
    root.assert_clean();
}

#[test]
#[ignore = "T5 native sidecar: requires CASKADE_7ZZ pointing to the pinned official binary"]
fn every_rar_fixture_truncated_at_25_50_75_99_percent_is_confined() {
    use archive::sevenzip::SevenZip;
    let tool = SevenZip::pinned(std::env::var_os("CASKADE_7ZZ").expect("set CASKADE_7ZZ")).unwrap();
    let mut cases = 0;
    for fixture in corpus()
        .into_iter()
        .filter(|f| f.expected.format.starts_with("rar"))
    {
        let parts = fixture.parts();
        let total: u64 = parts.iter().map(|p| p.metadata().unwrap().len()).sum();
        for percent in [25, 50, 75, 99] {
            let temp = tempfile::tempdir().unwrap();
            let mut remaining = total * percent / 100;
            // Retain every volume name, truncate the logical concatenated set
            // at the requested fraction, and leave later volumes empty.
            for part in &parts {
                let bytes = fs::read(part).unwrap();
                let length = bytes.len().min(remaining as usize);
                remaining -= length as u64;
                fs::write(
                    temp.path().join(part.file_name().unwrap()),
                    &bytes[..length],
                )
                .unwrap();
            }
            let input = temp.path().join(fixture.input().file_name().unwrap());
            let root = TestRoot::new().unwrap();
            let mut dir = SafeDir::new(
                open_staging(root.path()).unwrap(),
                Limits::default(),
                total * percent / 100,
                true,
            );
            let result = tool.extract(&input, root.path(), &mut dir, &mut NoopControl);
            confined(&root);
            if result.is_ok() {
                fixture.assert_tree(root.path()).unwrap();
            }
            cases += 1;
        }
    }
    assert_eq!(cases, 12);
}
