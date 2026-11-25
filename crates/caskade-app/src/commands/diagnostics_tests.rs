//! Diagnostics tests (#176): privacy-safe system info formatting, OS release
//! parsing, DRM adapter detection and logs folder preparation.

use super::*;

#[test]
fn formats_full_gpu_system_info() {
    let info = SystemInfo {
        version: "0.2.2".into(),
        debug_build: false,
        git_sha: Some("67c01f76".into()),
        os: "linux".into(),
        os_version: Some("Ubuntu 24.04.1 LTS".into()),
        arch: "x86_64".into(),
        renderer: RendererMode::Gpu("vello_hybrid"),
        gpu_adapter: Some("AMD Radeon RX 7800 XT".into()),
    };
    assert_eq!(
        format_system_info(&info),
        "Caskade: 0.2.2\nBuild: Release (67c01f76)\nOS: linux (x86_64) — Ubuntu 24.04.1 LTS\nRenderer: GPU (vello_hybrid) — AMD Radeon RX 7800 XT"
    );
}

#[test]
fn formats_minimal_cpu_system_info_without_optional_fields() {
    let info = SystemInfo {
        version: "0.2.2".into(),
        debug_build: true,
        git_sha: None,
        os: "windows".into(),
        os_version: None,
        arch: "x86_64".into(),
        renderer: RendererMode::Cpu("vello_cpu"),
        gpu_adapter: Some("Should not appear on CPU".into()),
    };
    assert_eq!(
        format_system_info(&info),
        "Caskade: 0.2.2\nBuild: Debug\nOS: windows (x86_64)\nRenderer: CPU (vello_cpu)"
    );
}

#[test]
fn system_info_sanitizes_control_chars_and_invalid_sha_and_leaks_no_paths_or_usernames() {
    let info = SystemInfo {
        version: "0.2.2\nInjected: secret".into(),
        debug_build: true,
        git_sha: Some("not-a-sha/../../../etc/passwd".into()),
        os: std::env::consts::OS.into(),
        os_version: Some("Fedora Linux 41\r\n".into()),
        arch: std::env::consts::ARCH.into(),
        renderer: RendererMode::Gpu("vello_hybrid"),
        gpu_adapter: None,
    };
    let formatted = format_system_info(&info);
    // Exactly four lines: no newline injection.
    assert_eq!(formatted.lines().count(), 4, "{formatted}");
    // Non-hex SHA is dropped rather than included verbatim.
    assert!(!formatted.contains("etc/passwd"), "{formatted}");
    assert!(formatted.contains("Build: Debug\n"), "{formatted}");
    // No home directory path or OS username leaks from environment.
    for var in ["HOME", "USERPROFILE", "LOGNAME"] {
        if let Ok(val) = std::env::var(var)
            && val.len() >= 4
        {
            assert!(
                !formatted.contains(&val),
                "diagnostics leaked {var}: {formatted}"
            );
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    assert!(
        !formatted.contains(&paths.logs.display().to_string()),
        "{formatted}"
    );
}

#[test]
fn parses_os_release_pretty_name_or_falls_back_to_name_and_version() {
    assert_eq!(
        parse_os_release(
            "# comment\nNAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n"
        )
        .as_deref(),
        Some("Ubuntu 24.04.1 LTS")
    );
    assert_eq!(
        parse_os_release("NAME='Arch Linux'\nVERSION_ID=rolling\n").as_deref(),
        Some("Arch Linux rolling")
    );
    assert_eq!(parse_os_release("NAME=NixOS\n").as_deref(), Some("NixOS"));
    assert_eq!(parse_os_release("# empty\n"), None);
}

#[test]
fn reads_drm_adapter_name_and_skips_connectors() {
    let temp = tempfile::tempdir().unwrap();
    let connector = temp.path().join("card0-DP-1").join("device");
    std::fs::create_dir_all(&connector).unwrap();
    std::fs::write(connector.join("product_name"), "Wrong Connector").unwrap();

    let card = temp.path().join("card1").join("device");
    std::fs::create_dir_all(&card).unwrap();
    std::fs::write(card.join("product_name"), "  AMD Radeon RX 7800 XT \n").unwrap();

    assert_eq!(
        read_drm_adapter_name(temp.path()).as_deref(),
        Some("AMD Radeon RX 7800 XT")
    );
}

#[test]
fn prepare_logs_folder_creates_missing_dir_and_rejects_non_directory() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    assert!(!paths.logs.exists());

    // Missing directory is created safely.
    let created = prepare_logs_folder(&paths).unwrap();
    assert_eq!(created, paths.logs);
    assert!(paths.logs.is_dir());

    // Existing directory succeeds.
    assert_eq!(prepare_logs_folder(&paths).unwrap(), paths.logs);

    // Regular file at logs path fails with a clear CommandError.
    let bad_root = tempfile::tempdir().unwrap();
    let bad_paths = AppPaths::under(bad_root.path());
    std::fs::write(&bad_paths.logs, "not a directory").unwrap();
    let err = prepare_logs_folder(&bad_paths).unwrap_err();
    assert_eq!(err.code, ErrorCode::Invalid);
    assert!(!err.summary.is_empty());
}
