#![cfg(feature = "windows-formats")]
use caskade_installer::formats::windows::inno::{InnoError, InnoLimits, inspect_path};
use caskade_installer::{RegistryHive, RegistryValue, RuntimeSpec, Step};
fn metadata() -> caskade_installer::formats::windows::inno::InnoMetadata {
    inspect_path(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/bin/windows/legacy-registry-path/setup.exe"),
        InnoLimits::default(),
    )
    .unwrap()
}
#[test]
fn legacy_registry_and_ini_replay_use_the_stable_prefix_drive() {
    let metadata = metadata();
    assert_eq!(metadata.registry[0].value, "{app}");
    for runtime in [
        RuntimeSpec::ProtonPrefix {
            id: "fixture".into(),
        },
        RuntimeSpec::WinePrefix {
            id: "fixture".into(),
        },
    ] {
        let steps = metadata.prefix_replay_steps(runtime.clone()).unwrap();
        assert_eq!(steps.len(), 2);
        let Step::WriteFile { path, contents } = &steps[0] else {
            panic!("INI step")
        };
        assert_eq!(path, std::path::Path::new("config.ini"));
        assert_eq!(contents, b"[Game]\r\nInstallPath=G:\\Game\r\n");
        let Step::RegistryImport {
            runtime: actual,
            entries,
        } = &steps[1]
        else {
            panic!("registry step")
        };
        assert_eq!(*actual, runtime);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hive, RegistryHive::LocalMachine);
        assert_eq!(entries[0].key, "Software\\CaskadeFixture\\FixtureGame");
        assert_eq!(entries[0].name, "InstallPath");
        assert_eq!(
            entries[0].value,
            RegistryValue::String(caskade_installer::runtime::proton::GAME_DIR.into())
        );
    }
}
#[test]
fn replay_is_opt_in_confined_and_rejects_opaque_effects_before_producing_steps() {
    let runtime = RuntimeSpec::ProtonPrefix {
        id: "fixture".into(),
    };
    let metadata = metadata();
    assert!(matches!(
        metadata.prefix_replay_steps(RuntimeSpec::Native),
        Err(InnoError::Unsupported(_))
    ));
    for bad in ["{tmp}\\config.ini", "{app}\\..\\outside.ini"] {
        let mut changed = metadata.clone();
        changed.ini[0].filename = bad.into();
        assert!(changed.prefix_replay_steps(runtime.clone()).is_err());
    }
    let mut changed = metadata.clone();
    changed.ini[0].value = "{code:Value}".into();
    assert!(changed.prefix_replay_steps(runtime.clone()).is_err());
    let mut changed = metadata.clone();
    changed.ini[0].key = "value\nInjected".into();
    assert!(changed.prefix_replay_steps(runtime.clone()).is_err());
    let mut changed = metadata.clone();
    changed.ini[0].filename = changed.files[0].destination.clone();
    assert!(changed.prefix_replay_steps(runtime.clone()).is_err());
    let mut changed = metadata.clone();
    changed.registry[0].value_type = "Binary".into();
    assert!(changed.prefix_replay_steps(runtime.clone()).is_err());
    let mut changed = metadata;
    changed.registry[0].hive = "HKEY_USERS".into();
    assert!(changed.prefix_replay_steps(runtime).is_err());
}

#[test]
fn replay_preserves_a_separator_after_the_game_subdirectory() {
    let mut metadata = metadata();
    metadata.ini[0].value = r"{app}\Data".into();
    let steps = metadata
        .prefix_replay_steps(RuntimeSpec::WinePrefix {
            id: "fixture".into(),
        })
        .unwrap();
    let Step::WriteFile { contents, .. } = &steps[0] else {
        panic!("INI step")
    };
    assert_eq!(contents, b"[Game]\r\nInstallPath=G:\\Game\\Data\r\n");
}
