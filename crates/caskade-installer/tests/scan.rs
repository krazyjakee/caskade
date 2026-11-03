//! #32: reference scan and relocation verdict.
#[cfg(feature = "windows-formats")]
use caskade_installer::scan::{Location, Via};
use caskade_installer::{
    manifest::RelocationVerdict,
    scan::{RegistryValue, ScanInput, ScanLimits, scan_references},
};
use std::{fs, path::PathBuf};

fn input<'a>(
    game: &'a std::path::Path,
    real: &'a str,
    alias: Option<&'a str>,
    registry: &'a [RegistryValue],
    external: &'a [PathBuf],
) -> ScanInput<'a> {
    ScanInput {
        game_dir: game,
        real_path: real,
        alias_path: alias,
        real_variants: &[],
        registry,
        external_files: external,
        delegated: false,
        limits: ScanLimits::default(),
    }
}

/// Unicode Inno setups currently surface registry data as raw UTF-16LE bytes
/// in the string; decode those so `{app}` can be substituted.
#[cfg(feature = "windows-formats")]
fn recorded_text(value: &str) -> String {
    if !value.contains('\0') {
        return value.into();
    }
    let units: Vec<u16> = value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .into()
}

/// AC: replay what `legacy-registry-path` writes (its recorded [INI] and
/// [Registry] entries, `{app}` = install directory) and scan the result.
#[cfg(feature = "windows-formats")]
#[test]
fn legacy_registry_path_is_aliased_with_the_alias_and_rewritable_without() {
    use caskade_installer::formats::windows::inno::{InnoLimits, inspect_path};
    let setup = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/windows/legacy-registry-path/setup.exe");
    let metadata = inspect_path(&setup, InnoLimits::default()).unwrap();
    assert!(!metadata.ini.is_empty() && !metadata.registry.is_empty());
    let real = r"C:\Games\Fixture Game";
    let alias = r"C:\Users\player\AppData\Local\Caskade\g\0123456789ab";

    let install = |install_dir: &str| {
        let game = tempfile::tempdir().unwrap();
        fs::write(game.path().join("FixtureGame.exe"), b"MZ game binary").unwrap();
        for entry in &metadata.ini {
            assert_eq!(entry.filename.to_ascii_lowercase(), r"{app}\config.ini");
            let value = entry.value.replace("{app}", install_dir);
            let path = game.path().join("config.ini");
            let mut text = fs::read_to_string(&path).unwrap_or_default();
            text.push_str(&format!("[{}]\r\n{}={value}\r\n", entry.section, entry.key));
            fs::write(path, text).unwrap();
        }
        let registry: Vec<_> = metadata
            .registry
            .iter()
            .map(|entry| RegistryValue {
                hive: entry.hive.clone(),
                key: entry.key.clone(),
                name: entry.name.clone(),
                value_type: "REG_SZ".into(),
                data: recorded_text(&entry.value)
                    .replace("{app}", install_dir)
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            })
            .collect();
        (game, registry)
    };

    let (game, registry) = install(alias);
    let report = scan_references(&input(game.path(), real, Some(alias), &registry, &[])).unwrap();
    assert_eq!(report.verdict, RelocationVerdict::Aliased, "{report:#?}");
    assert!(report.references.iter().all(|r| r.via == Via::Alias));
    assert_eq!(report.references.len(), 2);

    let (game, registry) = install(real);
    let report = scan_references(&input(game.path(), real, Some(alias), &registry, &[])).unwrap();
    assert_eq!(report.verdict, RelocationVerdict::Rewritable, "{report:#?}");
    assert!(report.references.iter().any(|r| matches!(
        &r.location,
        Location::GameFile { path, rewritable: true } if path == "config.ini"
    )));
    assert!(report.references.iter().any(|r| matches!(
        &r.location,
        Location::Registry { value_type, rewritable: true, .. } if value_type == "REG_SZ"
    )));
    // Without an alias at all, the same install is still Rewritable.
    let report = scan_references(&input(game.path(), real, None, &registry, &[])).unwrap();
    assert_eq!(report.verdict, RelocationVerdict::Rewritable);
}

#[test]
fn verdicts_cover_free_fragile_pinned_and_skip_caskade_metadata() {
    let game = tempfile::tempdir().unwrap();
    let real = game.path().to_str().unwrap().to_owned();
    fs::write(game.path().join("game.bin"), b"\0\x01 no paths here").unwrap();
    fs::create_dir(game.path().join(".caskade")).unwrap();
    fs::write(game.path().join(".caskade/install.json"), real.as_bytes()).unwrap();
    let report = scan_references(&input(game.path(), &real, None, &[], &[])).unwrap();
    assert_eq!(report.verdict, RelocationVerdict::Free);

    // A real path inside a binary file is Fragile.
    let mut binary = b"\0\x7fELF".to_vec();
    binary.extend(real.encode_utf16().flat_map(u16::to_le_bytes));
    binary.extend([0, 0, 0xff]);
    fs::write(game.path().join("cache.dat"), &binary).unwrap();
    let report = scan_references(&input(game.path(), &real, None, &[], &[])).unwrap();
    assert_eq!(report.verdict, RelocationVerdict::Fragile);
    fs::remove_file(game.path().join("cache.dat")).unwrap();

    // REG_BINARY and unknown out-of-root files are Fragile too.
    let binary_value = [RegistryValue {
        hive: "HKCU".into(),
        key: r"Software\Game".into(),
        name: "Blob".into(),
        value_type: "REG_BINARY".into(),
        data: real.as_bytes().to_vec(),
    }];
    let report = scan_references(&input(game.path(), &real, None, &binary_value, &[])).unwrap();
    assert_eq!(report.verdict, RelocationVerdict::Fragile);
    let outside = tempfile::tempdir().unwrap();
    let shortcut = outside.path().join("Game.desktop");
    fs::write(&shortcut, format!("Exec={real}/game\n")).unwrap();
    let external = [shortcut];
    let report = scan_references(&input(game.path(), &real, None, &[], &external)).unwrap();
    assert_eq!(report.verdict, RelocationVerdict::Rewritable);

    let mut delegated = input(game.path(), &real, None, &[], &[]);
    delegated.delegated = true;
    assert_eq!(
        scan_references(&delegated).unwrap().verdict,
        RelocationVerdict::Pinned
    );

    // Oversized files are reported as skipped, not silently trusted.
    fs::write(game.path().join("huge.cfg"), vec![b'x'; 2048]).unwrap();
    let mut limited = input(game.path(), &real, None, &[], &[]);
    limited.limits.max_file_bytes = 1024;
    assert_eq!(scan_references(&limited).unwrap().skipped, ["huge.cfg"]);
}
