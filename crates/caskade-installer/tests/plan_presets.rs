use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

use caskade_installer::{
    exec::{CancellationToken, ExecConfig, ExecRequest, Executor, INSTALL_DIR},
    plan::{
        PresetArgs,
        presets::{
            InstallerFamily as Family, InstallerMode as Mode, PresetError, SwitchSettings,
            build_args, build_uninstall_args, switch_catalog,
        },
    },
    testing::{FakeAction, FakeDiskSpace, FakeRuntime, FakeScript, RecordingSink, TestRoot},
    *,
};

const FAMILIES: [Family; 8] = [
    Family::Inno,
    Family::Nsis,
    Family::Msi,
    Family::Burn,
    Family::InstallShield,
    Family::Wise,
    Family::SetupFactory,
    Family::Custom,
];

// Expected values are independent of both the table and the builder.
fn expected(family: Family, mode: Mode, dir: &str) -> Vec<String> {
    let silent = mode == Mode::Silent;
    match family {
        Family::Inno => {
            let mut args = if silent {
                vec!["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"]
            } else {
                vec!["/NORESTART", "/SP-"]
            }
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
            args.extend([
                format!("/DIR=\"{dir}\""),
                "/LANG=english".into(),
                "/NOICONS".into(),
            ]);
            args
        }
        Family::Nsis => {
            let mut args = if silent { vec!["/S".into()] } else { vec![] };
            args.push(format!("/D={dir}"));
            args
        }
        Family::Msi => {
            let mut args = if silent {
                vec!["/qn".into(), "/norestart".into()]
            } else {
                vec!["/norestart".into()]
            };
            args.extend([format!("INSTALLDIR={dir}"), format!("TARGETDIR={dir}")]);
            args
        }
        Family::Burn => {
            let mut args = if silent {
                vec!["/quiet".into(), "/norestart".into()]
            } else {
                vec!["/norestart".into()]
            };
            args.push(format!("InstallFolder={dir}"));
            args
        }
        _ => vec![],
    }
}

#[test]
fn every_family_has_exact_presets_for_spaces_and_unicode() {
    assert_eq!(switch_catalog().len(), FAMILIES.len());
    for dir in [
        r"C:\Games\Fixture Game",
        r"D:\遊戲\Été très long",
        INSTALL_DIR,
    ] {
        for family in FAMILIES {
            for mode in [Mode::Silent, Mode::Interactive] {
                let settings = SwitchSettings {
                    install_dir: dir.into(),
                    language: Some("english".into()),
                    ..SwitchSettings::default()
                };
                let result = build_args(family, mode, &settings);
                if mode == Mode::Silent && !switch_catalog()[&family].silent_supported {
                    assert_eq!(result, Err(PresetError::SilentUnsupported));
                } else {
                    let result = result.unwrap();
                    assert_eq!(
                        result.args,
                        expected(family, mode, dir),
                        "{family:?}/{mode:?}"
                    );
                    assert!(!result.warnings.is_empty());
                }
                assert!(!switch_catalog()[&family].notes.is_empty());
            }
        }
    }
}

#[test]
fn settings_apply_supported_options_and_report_package_specific_preferences() {
    let defaults = SwitchSettings::default();
    assert!(!defaults.let_installers_create_shortcuts);
    assert!(defaults.no_launch_at_finish);
    assert_eq!(defaults.install_dir, INSTALL_DIR);
    let inno = build_args(Family::Inno, Mode::Interactive, &defaults).unwrap();
    assert!(inno.args.contains(&"/NOICONS".into()));
    assert!(!inno.args.iter().any(|arg| arg.starts_with("/LANG=")));
    assert!(
        inno.warnings
            .iter()
            .any(|warning| warning.contains("Start Menu"))
    );
    assert!(
        inno.warnings
            .iter()
            .any(|warning| warning.contains("Launch-at-finish"))
    );
    let enabled = SwitchSettings {
        let_installers_create_shortcuts: true,
        no_launch_at_finish: false,
        ..defaults
    };
    let inno = build_args(Family::Inno, Mode::Interactive, &enabled).unwrap();
    assert!(!inno.args.contains(&"/NOICONS".into()));
    let policy = PresetArgs {
        language: Some("japanese".into()),
        no_shortcuts: false,
    };
    let mapped = SwitchSettings::from(&policy);
    assert_eq!(mapped.language.as_deref(), Some("japanese"));
    assert!(mapped.let_installers_create_shortcuts);
    for family in [Family::Nsis, Family::Msi, Family::Burn] {
        let result = build_args(
            family,
            Mode::Interactive,
            &SwitchSettings::from(&PresetArgs {
                language: Some("english".into()),
                no_shortcuts: true,
            }),
        )
        .unwrap();
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("language cannot"))
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("Shortcut suppression"))
        );
        assert!(
            !result
                .args
                .iter()
                .any(|arg| arg.contains("LANG") || arg.contains("SHORTCUT"))
        );
    }
}

#[test]
fn values_cannot_break_embedded_quotes_or_add_control_characters() {
    for value in ["", "bad\"value", "bad\nvalue", "bad\0value"] {
        let mut settings = SwitchSettings {
            install_dir: value.into(),
            ..SwitchSettings::default()
        };
        assert!(matches!(
            build_args(Family::Inno, Mode::Silent, &settings),
            Err(PresetError::InvalidValue("install directory"))
        ));
        settings.install_dir = INSTALL_DIR.into();
        settings.language = Some(value.into());
        assert!(matches!(
            build_args(Family::Inno, Mode::Silent, &settings),
            Err(PresetError::InvalidValue("installer language"))
        ));
    }
}

#[tokio::test]
async fn fake_runtime_receives_exact_silent_and_interactive_presets() {
    for family in FAMILIES {
        for mode in [Mode::Silent, Mode::Interactive] {
            if mode == Mode::Silent && !switch_catalog()[&family].silent_supported {
                continue;
            }
            let root = TestRoot::new().unwrap();
            // An existing name forces the executor to expand the final path
            // after selecting a collision suffix, never the requested path.
            fs::create_dir(root.path().join("遊戲 Été Game")).unwrap();
            let runtime = Arc::new(FakeRuntime::new([FakeScript {
                actions: vec![FakeAction::WriteFile {
                    path: "game.exe".into(),
                    contents: b"game".to_vec(),
                }],
            }]));
            let settings = SwitchSettings {
                language: Some("english".into()),
                ..SwitchSettings::default()
            };
            let mut args = build_args(family, mode, &settings).unwrap().args;
            let mut expected_args = expected(family, mode, INSTALL_DIR);
            let program = if family == Family::Msi {
                args.splice(0..0, ["/i".into(), "setup with spaces.msi".into()]);
                expected_args.splice(0..0, ["/i".into(), "setup with spaces.msi".into()]);
                Program::Tool("msiexec".into())
            } else {
                Program::Tool("setup.exe".into())
            };
            let step = match mode {
                Mode::Silent => Step::Run {
                    program,
                    args,
                    env: BTreeMap::new(),
                    runtime: RuntimeSpec::Native,
                    timeout_secs: 30,
                    expect: vec!["game.exe".into()],
                },
                Mode::Interactive => Step::RunInteractive {
                    program,
                    args,
                    env: BTreeMap::new(),
                    runtime: RuntimeSpec::Native,
                    target_dir: ".".into(),
                },
            };
            let proposal = Candidate {
                handler: HandlerId("preset-test".into()),
                strategy: if mode == Mode::Silent {
                    Strategy::RunSilent
                } else {
                    Strategy::RunInteractive
                },
                confidence: 1.0,
                fidelity: Fidelity::Exact,
                requires: Requirements {
                    executes_code: true,
                    runtime: None,
                    may_prompt_elevation: false,
                    interactive: mode == Mode::Interactive,
                    est_disk_bytes: None,
                },
                steps: vec![step],
                warnings: vec![],
            };
            let report = Executor::new(
                runtime.clone(),
                Arc::new(FakeDiskSpace::fixed(u64::MAX)),
                Arc::new(RecordingSink::default()),
            )
            .with_config(ExecConfig {
                disk_margin_bytes: 0,
                ..ExecConfig::default()
            })
            .execute(
                ExecRequest {
                    job_id: "argv-job".into(),
                    root: root.path().into(),
                    relative_dir: "遊戲 Été Game".into(),
                    plans: PlanSet {
                        candidates: vec![proposal],
                        selected: Some(0),
                        question: None,
                    },
                    sources: BTreeMap::new(),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_ne!(report.relative_dir, Path::new("遊戲 Été Game"));
            let final_dir = report.dir.to_str().unwrap();
            let expected_args = expected_args
                .into_iter()
                .map(|arg| arg.replace(INSTALL_DIR, final_dir))
                .collect::<Vec<_>>();
            assert_eq!(
                runtime.processes()[0].command.args,
                expected_args,
                "{family:?}/{mode:?}"
            );
            assert_eq!(runtime.processes()[0].command.cwd, report.dir);
            assert_eq!(fs::read(report.dir.join("game.exe")).unwrap(), b"game");
            fs::remove_dir_all(report.dir).unwrap();
            fs::remove_dir(root.path().join("遊戲 Été Game")).unwrap();
            root.assert_clean();
        }
    }
}

#[test]
fn uninstall_presets_validate_product_codes_and_preserve_the_nsis_directory_tail() {
    assert_eq!(
        build_uninstall_args(Family::Inno, None).unwrap(),
        ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]
    );
    assert_eq!(
        build_uninstall_args(Family::Nsis, None).unwrap(),
        ["/S".to_owned(), format!("_?={INSTALL_DIR}")]
    );
    for code in [
        "{12345678-90AB-CDEF-1234-567890ABCDEF}",
        "{12345678-90ab-cdef-1234-567890abcdef}",
    ] {
        assert_eq!(
            build_uninstall_args(Family::Msi, Some(code)).unwrap(),
            ["/x", code, "/qn", "/norestart"]
        );
    }
    for code in [
        None,
        Some(""),
        Some("12345678-90AB-CDEF-1234-567890ABCDEF"),
        Some("{12345678_90AB-CDEF-1234-567890ABCDEF}"),
        Some("{12345678-90AB-CDEF-1234-567890ABCDEG}"),
        Some("{12345678-90AB-CDEF-1234-567890ABCDEF} /i evil.msi"),
        Some("{12345678-90AB-CDEF-1234-567890ABCDEF}\n"),
    ] {
        assert_eq!(
            build_uninstall_args(Family::Msi, code),
            Err(PresetError::InvalidProductCode)
        );
    }
    for family in [
        Family::Burn,
        Family::InstallShield,
        Family::Wise,
        Family::SetupFactory,
        Family::Custom,
    ] {
        assert_eq!(
            build_uninstall_args(family, None),
            Err(PresetError::UninstallUnsupported)
        );
    }
}
