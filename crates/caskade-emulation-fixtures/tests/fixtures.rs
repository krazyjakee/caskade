use caskade_discovery::{HostEnv, OperatingSystem, ScanCtx, probes};
use caskade_emulation_fixtures::{FixtureOs, MachineFixture, RomFixture, RomTree};
use std::{collections::BTreeSet, fs, io};

#[test]
fn generated_machine_layouts_are_isolated_and_consumed_by_shared_os_probes() {
    for (os, expected) in [
        (FixtureOs::Linux, OperatingSystem::Linux),
        (FixtureOs::Windows, OperatingSystem::Windows),
        (FixtureOs::Macos, OperatingSystem::Macos),
    ] {
        let machine = MachineFixture::new(os).unwrap();
        let env = HostEnv::fixture(machine.path()).unwrap();
        assert_eq!(env.os(), expected);
        assert_eq!(env.home(), machine.home);
        assert!(env.home().starts_with(machine.root()));
        assert!(env.var("CASKADE_LIVE_SECRET").is_none());
        let paths = probes::path::executables(&env, &ScanCtx::default(), &["retroarch"]).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].starts_with(machine.root()));
        if os == FixtureOs::Macos {
            let bundles = probes::bundles::applications(&env, &ScanCtx::default()).unwrap();
            assert_eq!(bundles.len(), 2);
            assert!(
                bundles
                    .iter()
                    .any(|app| app.identifier.as_deref() == Some("com.libretro.RetroArch"))
            );
        }
        if os == FixtureOs::Linux {
            let apps = probes::flatpak::applications(&env, &ScanCtx::default()).unwrap();
            assert_eq!(apps.len(), 1);
            assert_eq!(
                apps[0].identifier.as_deref(),
                Some("org.libretro.RetroArch")
            );
            assert!(apps[0].location.join("bin/retroarch").is_file());
            assert_eq!(
                machine.flatpak_list().unwrap(),
                "RetroArch\torg.libretro.RetroArch\t1.20.0\tstable\tuser\n"
            );
            let images = probes::appimage::applications(&env, &ScanCtx::default()).unwrap();
            assert_eq!(images.len(), 3);
            assert!(images.iter().any(|app| app.title == "PCSX2"));
            let candidates = probes::emulator_candidates(&env, &ScanCtx::default(), &[]).unwrap();
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].application.title, "RetroArch");
        }
    }
}

#[test]
fn thousand_file_rom_tree_has_independent_partition_and_genuine_archives() {
    let fixture = RomFixture::thousand().unwrap();
    let tree = &fixture.tree;
    assert_eq!(tree.files.len(), 1000);
    assert_eq!(tree.expected_games.len(), 963);
    assert_eq!(tree.expected_discs(), 983);
    assert_eq!(tree.claimed.len(), 9);
    assert_eq!(tree.excluded.len(), 8);
    let mut expected: Vec<_> = tree
        .expected_games
        .iter()
        .flat_map(|game| game.roms.iter())
        .collect();
    expected.extend(tree.claimed.iter());
    expected.extend(tree.excluded.iter());
    let expected: BTreeSet<_> = expected.into_iter().cloned().collect();
    assert_eq!(expected.len(), 1000);
    assert_eq!(expected, tree.files.iter().cloned().collect());
    assert!(tree.files.iter().all(|p| tree.root.join(p).is_file()));
    let mut zip = zip::ZipArchive::new(
        fs::File::open(tree.root.join("Archives/Fixture Zip (USA).zip")).unwrap(),
    )
    .unwrap();
    assert_eq!(zip.len(), 1);
    assert_eq!(
        zip.by_index(0).unwrap().name().unwrap(),
        "Fixture Zip (USA).sfc"
    );
    let mut seven = sevenz_rust2::ArchiveReader::open(
        tree.root.join("Archives/Fixture Seven (Japan).7z"),
        sevenz_rust2::Password::empty(),
    )
    .unwrap();
    let mut names = vec![];
    seven
        .for_each_entries(|entry, reader| {
            names.push(entry.name.clone());
            let mut bytes = vec![];
            reader.read_to_end(&mut bytes)?;
            assert_eq!(bytes, b"Caskade original synthetic ROM payload\n");
            Ok(true)
        })
        .unwrap();
    assert_eq!(names, ["Fixture Seven (Japan).sfc"]);
    let destination = tree.root.clone();
    assert_eq!(
        RomTree::generate(&destination, 1000).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
}

#[test]
fn fixture_generation_is_deterministic_and_temp_only() {
    let first = RomFixture::thousand().unwrap();
    let second = RomFixture::thousand().unwrap();
    assert_eq!(first.tree.files, second.tree.files);
    assert_eq!(first.tree.expected_games, second.tree.expected_games);
    for file in &first.tree.files {
        assert_eq!(
            fs::read(first.tree.root.join(file)).unwrap(),
            fs::read(second.tree.root.join(file)).unwrap(),
            "{}",
            file.display()
        );
    }
    let path = first.tree.root.clone();
    drop(first);
    assert!(!path.exists());
}

#[test]
fn read_only_fixture_check_rejects_an_unlisted_generated_file() {
    let fixture = RomFixture::thousand().unwrap();
    fixture.tree.validate().unwrap();
    fs::write(fixture.tree.root.join("unexpected.chd"), []).unwrap();
    assert!(fixture.tree.validate().is_err());
}
