//! #30 AC: redistributables reported for the genuine fixtures.
use caskade_installer::{
    host::HostCaps,
    redist::{RedistTable, dependencies},
    *,
};
use std::path::{Path, PathBuf};

fn host(name: &str) -> HostCaps {
    toml::from_str(match name {
        "windows" => include_str!("hosts/windows.toml"),
        _ => include_str!("hosts/linux.toml"),
    })
    .unwrap()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/windows")
}

fn ids(found: &[redist::Found]) -> Vec<&str> {
    found
        .iter()
        .map(|item| item.definition.id.as_str())
        .collect()
}

#[test]
fn steamlike_commonredist_reports_vcredist_with_host_specific_actions() {
    let zip = std::fs::File::open(fixtures().join("steamlike-commonredist/payload.zip")).unwrap();
    let archive = zip::ZipArchive::new(zip).unwrap();
    let paths: Vec<PathBuf> = archive
        .file_names()
        .map(|name| PathBuf::from(name.unwrap().into_owned()))
        .collect();
    let found = RedistTable::default().detect(paths.iter().map(PathBuf::as_path), [], &|_| None);
    assert_eq!(ids(&found), ["vcredist-2015-2022-x64"]);
    assert_eq!(found[0].path, Path::new("_CommonRedist/vcredist_x64.exe"));

    // Windows, not yet installed: one silent run of the shipped installer.
    let windows = dependencies(&found, &host("windows"), &RuntimeSpec::Native, &|_| false);
    assert!(!windows[0].installed);
    assert_eq!(
        windows[0].install_steps,
        [Step::Run {
            program: Program::Source(SourcePath::Game("_CommonRedist/vcredist_x64.exe".into())),
            args: vec!["/install".into(), "/quiet".into(), "/norestart".into()],
            env: Default::default(),
            runtime: RuntimeSpec::Native,
            timeout_secs: 1800,
            expect: vec![],
        }]
    );
    // Windows, already installed: skipped.
    let present = dependencies(&found, &host("windows"), &RuntimeSpec::Native, &|_| true);
    assert!(present[0].installed && present[0].install_steps.is_empty());
    // Proton: recorded as a dependency with its winetricks verb only.
    let proton = dependencies(
        &found,
        &host("linux"),
        &RuntimeSpec::ProtonPrefix { id: "main".into() },
        &|_| panic!("no host check inside a prefix"),
    );
    assert!(proton[0].install_steps.is_empty() && !proton[0].installed);
    assert_eq!(proton[0].runtime_verbs, ["vcrun2022"]);
}

#[cfg(feature = "windows-formats")]
#[test]
fn gog_inno_unity_reports_directx_and_vcredist() {
    use caskade_installer::formats::windows::inno::{InnoLimits, app_path, inspect_path};
    let metadata = inspect_path(
        &fixtures().join("gog-inno-unity/setup.exe"),
        InnoLimits::default(),
    )
    .unwrap();
    let paths: Vec<PathBuf> = metadata
        .files
        .iter()
        .filter_map(|file| app_path(&file.destination).ok().flatten())
        .collect();
    let runs: Vec<PathBuf> = metadata
        .run
        .iter()
        .filter_map(|run| app_path(&run.filename).ok().flatten())
        .collect();
    let found = RedistTable::default().detect(
        paths.iter().map(PathBuf::as_path),
        runs.iter().map(PathBuf::as_path),
        &|_| None,
    );
    assert_eq!(ids(&found), ["directx-jun2010", "vcredist-2015-2022-x64"]);
    let proton = dependencies(
        &found,
        &host("linux"),
        &RuntimeSpec::ProtonPrefix { id: "main".into() },
        &|_| false,
    );
    assert_eq!(proton[0].runtime_verbs[0], "d3dx9");
    assert_eq!(proton[1].runtime_verbs, ["vcrun2022"]);
}

#[test]
fn product_names_identify_renamed_installers_in_redist_folders_only() {
    let table = RedistTable::default();
    let renamed = Path::new("_CommonRedist/MSVC/setup.exe");
    let outside = Path::new("bin/setup.exe");
    let product = |_: &Path| Some("Microsoft Visual C++ 2019 X64 Minimum Runtime".to_owned());
    assert_eq!(
        ids(&table.detect([renamed], [], &product)),
        ["vcredist-2015-2022-x64"]
    );
    assert!(table.detect([outside], [], &product).is_empty());
}
