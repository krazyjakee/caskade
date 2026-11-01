use super::*;
use tempfile::TempDir;

fn sample() -> TempDir {
    let temp = TempDir::new().unwrap();
    for directory in ["src", "support", "machines/linux/root", "formats"] {
        fs::create_dir_all(temp.path().join(FIXTURES).join(directory)).unwrap();
    }
    fs::create_dir_all(temp.path().join("xtask/discovery")).unwrap();
    fs::write(
        temp.path().join("xtask/discovery/generate.py"),
        "# original synthetic builder\n",
    )
    .unwrap();
    fs::write(
        temp.path().join(FIXTURES).join("src/source.json"),
        r#"{"home":"/home/fixture"}"#,
    )
    .unwrap();
    fs::write(
        temp.path()
            .join(FIXTURES)
            .join("machines/linux/root/game.json"),
        r#"{"path":"/home/fixture/Games/Synthetic","steamid":"76561198000000000"}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(FIXTURES).join("formats/sample.bin"),
        [0, 1, 2, 3],
    )
    .unwrap();
    write_lock(temp.path()).unwrap();
    temp
}

#[test]
fn checks_need_no_builders_and_do_not_write() {
    let temp = sample();
    let before = fs::read(temp.path().join(FIXTURES).join("build.lock")).unwrap();
    check(temp.path()).unwrap();
    assert_eq!(
        fs::read(temp.path().join(FIXTURES).join("build.lock")).unwrap(),
        before
    );
}

#[test]
fn source_and_output_and_inventory_mutations_fail_staleness() {
    for path in [
        "src/source.json",
        "formats/sample.bin",
        "machines/linux/root/game.json",
    ] {
        let temp = sample();
        fs::write(temp.path().join(FIXTURES).join(path), b"changed bytes").unwrap();
        assert!(
            check_stale(temp.path())
                .unwrap_err()
                .to_string()
                .contains("stale")
        );
    }
    let temp = sample();
    fs::write(temp.path().join(FIXTURES).join("formats/extra.bin"), [0]).unwrap();
    assert!(check_stale(temp.path()).is_err());
    fs::remove_file(temp.path().join(FIXTURES).join("formats/sample.bin")).unwrap();
    assert!(check_stale(temp.path()).is_err());
}

#[test]
fn source_hash_distinguishes_non_utf8_bytes() {
    let temp = sample();
    let path = temp.path().join(FIXTURES).join("src/source.json");
    fs::write(&path, [0xff]).unwrap();
    let a = hash_sources(temp.path()).unwrap();
    fs::write(path, [0xfe]).unwrap();
    assert_ne!(a, hash_sources(temp.path()).unwrap());
}

#[test]
fn planted_home_is_rejected_in_source_binary_utf16_and_filename() {
    for data in [
        b"/home/real-user/private".to_vec(),
        b"C:\\Users\\real-user\\private".to_vec(),
        "C:\\Users\\real-user\\private"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect(),
        [
            vec![7],
            "/Users/real-user/private"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect(),
        ]
        .concat(),
    ] {
        let temp = sample();
        fs::write(temp.path().join(FIXTURES).join("formats/sample.bin"), data).unwrap();
        assert!(
            check_privacy(temp.path())
                .unwrap_err()
                .to_string()
                .contains("home")
        );
    }
    let temp = sample();
    let path = temp
        .path()
        .join(FIXTURES)
        .join("machines/linux/root/home/real-user/private");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, []).unwrap();
    assert!(check_privacy(temp.path()).is_err());
}

#[test]
fn real_account_ids_rejected_without_redacting_game_app_ids() {
    for data in [
        "steamid=76561198123456789",
        "userdata/123456/config/",
        r#"{"nested":{"account_id":"private-account"}}"#,
    ] {
        assert!(privacy::check_bytes(Path::new("synthetic"), data.as_bytes()).is_err());
    }
    privacy::check_bytes(
        Path::new("synthetic"),
        b"steam:620 gog:1207658924 /home/fixture SteamID=76561198000000000 userdata/424242/",
    )
    .unwrap();
}

#[test]
fn size_violation_fails_without_allocating_the_oversized_file() {
    let temp = sample();
    let file = fs::File::create(temp.path().join(FIXTURES).join("formats/too-large.bin")).unwrap();
    file.set_len(MAX_FILE + 1).unwrap();
    assert!(check_size(temp.path()).is_err());
    assert!(outputs(temp.path()).is_err());
}

#[test]
fn total_budget_rejects_many_individually_valid_files() {
    let temp = sample();
    for i in 0..9 {
        fs::File::create(temp.path().join(FIXTURES).join(format!("formats/{i}.bin")))
            .unwrap()
            .set_len(MAX_FILE)
            .unwrap();
    }
    assert!(check_size(temp.path()).is_err());
}

#[cfg(unix)]
#[test]
fn fixture_links_are_rejected_without_following_them() {
    let temp = sample();
    std::os::unix::fs::symlink(
        "/etc/passwd",
        temp.path().join(FIXTURES).join("formats/link"),
    )
    .unwrap();
    assert!(
        check(temp.path())
            .unwrap_err()
            .to_string()
            .contains("symlink")
    );
}
