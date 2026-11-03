use crate::fixtures::write;
use caskade_discovery::{
    probes::{self, bundles, path},
    *,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn executable(root: &Path, name: &str) -> PathBuf {
    let path = write(
        root,
        name,
        b"original test executable placeholder: never run",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}
fn machine(os: &str, extra: &str) -> tempfile::TempDir {
    let temp = crate::fixtures::tempdir();
    fs::create_dir(temp.path().join("root")).unwrap();
    write(
        temp.path(),
        "machine.toml",
        format!("schema_version=1\nos='{os}'\n{extra}"),
    );
    temp
}
#[test]
fn path_probe_obeys_captured_linux_macos_and_windows_semantics() {
    for os in ["linux", "macos", "windows"] {
        let windows = os == "windows";
        let temp = machine(
            os,
            if windows {
                "[env]\nPATH='relative;;C:\\Tools;C:\\Other'\nPATHEXT='.EXE;.CMD'\n"
            } else {
                "[env]\nPATH='relative::/opt/emulators:/usr/bin'\n"
            },
        );
        let first = executable(
            temp.path(),
            if windows {
                "root/drives/c/Tools/retroarch.exe"
            } else {
                "root/opt/emulators/retroarch"
            },
        );
        executable(
            temp.path(),
            if windows {
                "root/drives/c/Other/retroarch.exe"
            } else {
                "root/usr/bin/retroarch"
            },
        );
        let env = HostEnv::fixture(temp.path()).unwrap();
        assert_eq!(
            path::executables(&env, &ScanCtx::default(), &["retroarch"]).unwrap(),
            [first]
        );
        assert!(
            path::executables(&env, &ScanCtx::default(), &["absent"])
                .unwrap()
                .is_empty()
        );
        assert!(path::executables(&env, &ScanCtx::default(), &["../retroarch"]).is_err());
    }
}
#[cfg(unix)]
#[test]
fn unix_path_probe_requires_execute_mode_without_using_host_path_or_cwd() {
    let temp = machine("linux", "[env]\nPATH='/bin:.:relative'\n");
    write(temp.path(), "root/bin/retroarch", b"non-executable");
    let env = HostEnv::fixture(temp.path()).unwrap();
    assert!(
        path::executables(&env, &ScanCtx::default(), &["retroarch", "sh"])
            .unwrap()
            .is_empty()
    );
}
#[cfg(feature = "linux-sources")]
#[test]
fn flatpak_reader_finds_user_system_and_extra_deployments_and_excludes_runtimes() {
    let temp = machine("linux", "");
    for (root, id) in [
        (
            "home/fixture/.local/share/flatpak",
            "org.libretro.RetroArch",
        ),
        ("var/lib/flatpak", "org.DolphinEmu.dolphin-emu"),
        ("srv/apps", "org.example.Extra"),
    ] {
        write(
            temp.path(),
            &format!("root/{root}/app/{id}/current/active/metadata"),
            format!("[Application]\nname={id}\n"),
        );
        write(
            temp.path(),
            &format!("root/{root}/app/{id}/current/active/files/.keep"),
            b"synthetic deployment",
        );
    }
    write(
        temp.path(),
        "root/etc/flatpak/installations.d/extra.conf",
        b"[Installation \"extra\"]\nPath=/srv/apps\n",
    );
    write(
        temp.path(),
        "root/var/lib/flatpak/runtime/org.freedesktop.Platform/current/active/metadata",
        b"[Runtime]\nname=org.freedesktop.Platform\n",
    );
    write(
        temp.path(),
        "root/var/lib/flatpak/app/org.example.Bad/current/active/metadata",
        b"[Application]\nname=org.example.Wrong\n",
    );
    write(
        temp.path(),
        "root/var/lib/flatpak/app/org.example.Bad/current/active/files/.keep",
        b"bad",
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    let ctx = ScanCtx::default();
    let apps = probes::flatpak::applications(&env, &ctx).unwrap();
    assert_eq!(apps.len(), 3);
    assert!(apps.iter().all(|a| a.location.is_dir()));
    let app = probes::flatpak::find(&env, &ctx, "org.libretro.RetroArch")
        .unwrap()
        .unwrap();
    assert_eq!(
        app.launch,
        LaunchTarget::Flatpak {
            app_id: "org.libretro.RetroArch".into(),
            args: vec![]
        }
    );
    assert!(app.location.starts_with(env.home()));
    assert!(probes::flatpak::find(&env, &ctx, "not-an-id").is_err());
    assert!(!ctx.take_warnings().is_empty());
}
fn bundle(root: &Path, at: &str, name: &str, id: &str) -> PathBuf {
    let path = root.join(at);
    let mut dict = plist::Dictionary::new();
    for (key, value) in [
        ("CFBundleName", name),
        ("CFBundleIdentifier", id),
        ("CFBundleExecutable", "Emulator"),
        ("CFBundleShortVersionString", "1.2.3"),
        ("CFBundleIconFile", "emulator"),
        (
            "LSApplicationCategoryType",
            "public.app-category.entertainment",
        ),
    ] {
        dict.insert(key.into(), plist::Value::String(value.into()));
    }
    let mut bytes = vec![];
    plist::Value::Dictionary(dict)
        .to_writer_binary(&mut bytes)
        .unwrap();
    write(&path, "Contents/Info.plist", bytes);
    executable(&path, "Contents/MacOS/Emulator");
    write(
        &path,
        "Contents/Resources/emulator.icns",
        b"image placeholder",
    );
    path
}
#[test]
fn bundle_metadata_and_exact_identity_are_neutral_to_game_classifier_on_mac_profile() {
    let temp = machine("macos", "");
    let app = bundle(
        temp.path(),
        "root/Applications/Dolphin.app",
        "Dolphin Café",
        "org.dolphin-emu.dolphin",
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    let found = bundles::applications(&env, &ScanCtx::default()).unwrap();
    assert_eq!(found.len(), 1);
    let found = &found[0];
    assert_eq!(found.title, "Dolphin Café");
    assert_eq!(found.identifier.as_deref(), Some("org.dolphin-emu.dolphin"));
    assert_eq!(found.version.as_deref(), Some("1.2.3"));
    assert_eq!(found.executable, Some(app.join("Contents/MacOS/Emulator")));
    assert_eq!(
        found.icon,
        Some(app.join("Contents/Resources/emulator.icns"))
    );
    assert!(
        matches!(&found.launch,LaunchTarget::Exec{program,args,..}if program==&app.join("Contents/MacOS/Emulator")&&args.is_empty())
    );
    assert!(!app.join(".caskade").exists());
}
#[cfg(feature = "deep")]
#[test]
fn appimage_probe_reads_committed_squashfs_metadata_without_launch_or_extraction() {
    let temp = machine("linux", "");
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../caskade-installer/tests/fixtures/bin/appimage/Indie_Game-x86_64.AppImage");
    let input = fs::read(source).unwrap();
    let path = write(
        temp.path(),
        "root/home/fixture/Applications/Indie.AppImage",
        &input,
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    let app = probes::appimage::inspect(&env, &ScanCtx::default(), &path).unwrap();
    assert!(!app.title.is_empty());
    assert!(app.categories.iter().any(|c| c == "Game"));
    assert_eq!(app.executable, Some(path.clone()));
    assert_eq!(fs::read(&path).unwrap(), input);
    assert_eq!(
        probes::appimage::applications(&env, &ScanCtx::default())
            .unwrap()
            .len(),
        1
    );
    assert!(!path.parent().unwrap().join(".caskade").exists());
    write(
        temp.path(),
        "root/home/fixture/Applications/Bad.AppImage",
        b"not an image",
    );
    let ctx = ScanCtx::default();
    assert_eq!(probes::appimage::applications(&env, &ctx).unwrap().len(), 1);
    assert!(!ctx.take_warnings().is_empty());
}
#[cfg(feature = "linux-sources")]
#[test]
fn emulator_only_desktop_entries_are_handed_off_without_entering_game_report() {
    let temp = machine("linux", "[env]\nPATH='/usr/bin'\n");
    executable(temp.path(), "root/usr/bin/retroarch");
    write(temp.path(),"root/home/fixture/.local/share/applications/retroarch.desktop",b"[Desktop Entry]\nType=Application\nName=RetroArch\nExec=retroarch --menu\nCategories=Game;Emulator;\n");
    let env = HostEnv::fixture(temp.path()).unwrap();
    let candidates = probes::emulator_candidates(&env, &ScanCtx::default(), &[]).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].origin, probes::EmulatorOrigin::DesktopEntry);
    assert!(
        matches!(&candidates[0].application.launch,LaunchTarget::Exec{args,..}if args==&["--menu"])
    );
    assert!(
        sources::desktop::Desktop
            .scan(&env, &ScanCtx::default())
            .unwrap()
            .is_empty()
    );
}
#[cfg(feature = "windows-sources")]
fn shortcut(target: &str) -> Vec<u8> {
    let mut data = vec![0; 76];
    data[..4].copy_from_slice(&76u32.to_le_bytes());
    data[4..20].copy_from_slice(&[1, 20, 2, 0, 0, 0, 0, 0, 192, 0, 0, 0, 0, 0, 0, 70]);
    data[20..24].copy_from_slice(&0x88u32.to_le_bytes());
    let words: Vec<_> = target.encode_utf16().collect();
    data.extend((words.len() as u16).to_le_bytes());
    for word in words {
        data.extend(word.to_le_bytes());
    }
    data.extend(0u32.to_le_bytes());
    data
}
#[cfg(feature = "windows-sources")]
#[test]
fn emulator_shortcuts_are_handed_to_consumer_definition_matcher_not_game_scan() {
    let temp = machine("windows", "");
    executable(
        temp.path(),
        "root/drives/c/Emulators/RetroArch/retroarch.exe",
    );
    write(
        temp.path(),
        "root/drives/c/Users/fixture/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/RetroArch.lnk",
        shortcut(r"C:\Emulators\RetroArch\retroarch.exe"),
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    assert!(
        probes::emulator_candidates(&env, &ScanCtx::default(), &[])
            .unwrap()
            .is_empty()
    );
    let candidates =
        probes::emulator_candidates(&env, &ScanCtx::default(), &["RETROARCH.EXE"]).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].origin, probes::EmulatorOrigin::Shortcut);
    assert_eq!(candidates[0].application.title, "RetroArch");
    assert!(
        sources::startmenu::StartMenuSource::new()
            .scan(&env, &ScanCtx::default())
            .unwrap()
            .is_empty()
    );
}
#[test]
fn cancelled_probes_do_not_read_or_return_partial_matches() {
    let temp = machine("macos", "");
    let env = HostEnv::fixture(temp.path()).unwrap();
    let cancel = Cancel::new();
    cancel.cancel();
    let ctx = ScanCtx::new(cancel);
    assert!(matches!(
        path::executables(&env, &ctx, &["retroarch"]),
        Err(SourceError::Cancelled)
    ));
    assert!(matches!(
        bundles::applications(&env, &ctx),
        Err(SourceError::Cancelled)
    ));
    assert!(matches!(
        probes::emulator_candidates(&env, &ctx, &[]),
        Err(SourceError::Cancelled)
    ));
}
#[cfg(unix)]
#[test]
fn escaped_bundle_executable_is_rejected_without_touching_external_sentinel() {
    let temp = machine("macos", "");
    let app = bundle(
        temp.path(),
        "root/Applications/Escape.app",
        "Escape",
        "org.example.Escape",
    );
    let outside = crate::fixtures::tempdir();
    let sentinel = write(outside.path(), "sentinel", b"untouched");
    fs::remove_file(app.join("Contents/MacOS/Emulator")).unwrap();
    std::os::unix::fs::symlink(&sentinel, app.join("Contents/MacOS/Emulator")).unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    assert!(bundles::inspect(&env, &ScanCtx::default(), &app).is_err());
    assert_eq!(fs::read(sentinel).unwrap(), b"untouched");
}
