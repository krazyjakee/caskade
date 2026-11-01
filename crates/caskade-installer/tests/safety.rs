use std::io::Cursor;
use std::path::{Component, Path};

use caskade_installer::safety::*;
use proptest::prelude::*;

#[test]
fn rejects_every_unsafe_name_category() {
    let cases = [
        ("", "empty"),
        ("./", "empty"),
        ("a\0b", "nul"),
        ("/etc/passwd", "absolute"),
        ("\\root", "absolute"),
        ("C:relative", "drive"),
        ("D:\\absolute", "drive"),
        ("\\\\server\\share", "unc"),
        ("//server/share", "unc"),
        ("\\\\?\\C:\\x", "unc"),
        ("../escape", "parent"),
        ("a\\..\\b", "parent"),
        ("a/../../b", "parent"),
        ("CON", "reserved"),
        ("nul.txt", "reserved"),
        ("aux.foo", "reserved"),
        ("LPT9.ext", "reserved"),
        ("COM¹", "reserved"),
        ("con .txt", "reserved"),
        ("a./b", "trailing"),
        ("a /b", "trailing"),
        ("file:stream", "invalid"),
        ("a?b", "invalid"),
        ("a\nb", "invalid"),
    ];
    for (name, category) in cases {
        let error = sanitize_path(name).unwrap_err();
        let actual = match error {
            SafetyError::EmptyName => "empty",
            SafetyError::Nul => "nul",
            SafetyError::AbsolutePath => "absolute",
            SafetyError::DrivePrefix => "drive",
            SafetyError::UncPath => "unc",
            SafetyError::ParentTraversal => "parent",
            SafetyError::ReservedName(_) => "reserved",
            SafetyError::TrailingDotOrSpace => "trailing",
            SafetyError::InvalidCharacter => "invalid",
            other => panic!("unexpected {other}"),
        };
        assert_eq!(actual, category, "{name:?}");
    }
    assert_eq!(sanitize_path("a\\b/./file").unwrap(), Path::new("a/b/file"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]
    #[test]
    fn every_name_is_rejected_or_confined(name in prop_oneof![any::<String>(), (prop::collection::vec(prop::sample::select(vec!["..", ".", "C:", "CON", "foo", "bar", "é", "e\u{301}", "\0", "trail ", "a."]), 1..8), any::<bool>()).prop_map(|(parts, backslash)| parts.join(if backslash { "\\" } else { "/" }))]) {
        if let Ok(path) = sanitize_path(&name) {
            prop_assert!(!path.is_absolute());
            prop_assert!(path.components().all(|c| matches!(c, Component::Normal(_))));
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("staging");
            std::fs::create_dir(&root).unwrap();
            let sentinel = temp.path().join("sentinel");
            std::fs::write(&sentinel, b"unchanged").unwrap();
            let mut safe = SafeDir::new(open_staging(&root).unwrap(), Limits::default(), 1024, true);
            // Host path length constraints may reject otherwise valid names.
            if safe.write_file(&name, Cursor::new(b"ok"), Some(2), 0, 0o644).is_ok() {
                let resolved = std::fs::canonicalize(root.join(path)).unwrap();
                prop_assert!(resolved.starts_with(std::fs::canonicalize(&root).unwrap()));
            }
            prop_assert_eq!(std::fs::read(sentinel).unwrap(), b"unchanged");
        }
    }
}

fn safe(temp: &tempfile::TempDir, limits: Limits, sensitive: bool) -> SafeDir {
    SafeDir::new(open_staging(temp.path()).unwrap(), limits, 1024, sensitive)
}
fn write(dir: &mut SafeDir, name: &str) -> Result<std::path::PathBuf> {
    dir.write_file(name, Cursor::new(b"payload"), Some(7), 0, 0o6755)
}

#[test]
fn collisions_include_ancestors_and_unicode_normalization() {
    for (first, second) in [
        ("Foo/a", "foo/b"),
        ("café", "cafe\u{301}"),
        ("σ", "ς"),
        ("ß", "ss"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut dir = safe(&temp, Limits::default(), false);
        write(&mut dir, first).unwrap();
        assert!(matches!(
            write(&mut dir, second),
            Err(SafetyError::Collision(_))
        ));
    }
    let temp = tempfile::tempdir().unwrap();
    let mut dir = safe(&temp, Limits::default(), true);
    write(&mut dir, "same").unwrap();
    assert!(matches!(
        write(&mut dir, "same"),
        Err(SafetyError::Collision(_))
    ));
    dir.create_dir("implicit/child", 0).unwrap();
    dir.create_dir("implicit", 0).unwrap();
}

#[test]
fn rejects_devices_fifos_and_other_special_files() {
    assert!(matches!(
        validate_entry_kind(EntryKind::Device),
        Err(SafetyError::DeviceNode)
    ));
    assert!(matches!(
        validate_entry_kind(EntryKind::Fifo),
        Err(SafetyError::Fifo)
    ));
    assert!(matches!(
        validate_entry_kind(EntryKind::Other),
        Err(SafetyError::SpecialFile)
    ));
    assert_eq!(safe_mode(0o106755), 0o755);
}

#[test]
fn validates_declared_and_streamed_limits() {
    let limits = Limits {
        max_total_bytes: 10,
        max_entries: 1,
        max_depth: 2,
        ..Limits::default()
    };
    let mut tracker = LimitTracker::new(limits, 100);
    assert!(matches!(
        tracker.begin_entry(11, 0),
        Err(SafetyError::ByteLimit)
    ));
    assert!(matches!(
        tracker.begin_entry(0, 3),
        Err(SafetyError::DepthLimit)
    ));
    tracker.begin_entry(10, 2).unwrap();
    assert!(matches!(
        tracker.begin_entry(0, 0),
        Err(SafetyError::EntryLimit)
    ));
    tracker.record_bytes(10).unwrap();
    assert!(matches!(
        tracker.record_bytes(1),
        Err(SafetyError::ByteLimit)
    ));
    assert!(matches!(
        tracker.record_bytes(u64::MAX),
        Err(SafetyError::ByteLimit)
    ));
    let mut tracker = LimitTracker::new(
        Limits {
            max_expansion_ratio: 2,
            ..Limits::default()
        },
        2,
    );
    assert!(matches!(
        tracker.begin_entry(5, 0),
        Err(SafetyError::ExpansionRatio)
    ));
    assert!(matches!(
        tracker.record_bytes(5),
        Err(SafetyError::ExpansionRatio)
    ));
    let mut tracker = LimitTracker::new(Limits::default(), 0);
    assert!(matches!(
        tracker.record_bytes(1),
        Err(SafetyError::ExpansionRatio)
    ));
}

#[test]
fn mismatched_and_unknown_sizes_leave_no_partial_file() {
    for declared in [Some(1), Some(99)] {
        let temp = tempfile::tempdir().unwrap();
        let mut dir = safe(&temp, Limits::default(), true);
        assert!(matches!(
            dir.write_file("file", Cursor::new(b"payload"), declared, 0, 0o644),
            Err(SafetyError::SizeMismatch { .. })
        ));
        assert!(!temp.path().join("file").exists());
    }
    let temp = tempfile::tempdir().unwrap();
    let mut dir = safe(
        &temp,
        Limits {
            max_total_bytes: 2,
            ..Limits::default()
        },
        true,
    );
    assert!(matches!(
        dir.write_file("file", Cursor::new(b"payload"), None, 0, 0o644),
        Err(SafetyError::ByteLimit)
    ));
    assert!(!temp.path().join("file").exists());
    let mut dir = safe(
        &temp,
        Limits {
            max_path_depth: 1,
            ..Limits::default()
        },
        true,
    );
    assert!(matches!(
        write(&mut dir, "a/b"),
        Err(SafetyError::DepthLimit)
    ));
}

#[test]
fn hardlinks_require_a_completed_regular_file() {
    let temp = tempfile::tempdir().unwrap();
    let mut dir = safe(&temp, Limits::default(), true);
    for target in ["missing", "../escape", "/etc/passwd"] {
        assert!(matches!(
            dir.hardlink("link", target, 0),
            Err(SafetyError::InvalidHardlinkTarget)
        ));
    }
    dir.create_dir("folder", 0).unwrap();
    assert!(matches!(
        dir.hardlink("link", "folder", 0),
        Err(SafetyError::InvalidHardlinkTarget)
    ));
    write(&mut dir, "file").unwrap();
    dir.hardlink("link", "file", 0).unwrap();
    assert_eq!(std::fs::read(temp.path().join("link")).unwrap(), b"payload");
}

#[test]
fn decodes_archive_name_encodings_without_loss() {
    assert_eq!(
        decode_name(b"caf\x82.txt", NameEncoding::Cp437).unwrap(),
        "café.txt"
    );
    let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode("日本語.txt");
    assert_eq!(
        decode_name(&bytes, NameEncoding::Label(b"shift_jis")).unwrap(),
        "日本語.txt"
    );
    assert_eq!(
        decode_name("café.txt".as_bytes(), NameEncoding::Detect).unwrap(),
        "café.txt"
    );
    assert!(matches!(
        decode_name(&[0xff], NameEncoding::Utf8),
        Err(SafetyError::InvalidEncoding)
    ));
    assert!(matches!(
        decode_name(&[0x81], NameEncoding::Label(b"shift_jis")),
        Err(SafetyError::InvalidEncoding)
    ));
    assert!(matches!(
        decode_name(b"x", NameEncoding::Label(b"unknown")),
        Err(SafetyError::InvalidEncoding)
    ));
    assert!(matches!(
        decode_name(b"a\0b", NameEncoding::Cp437),
        Err(SafetyError::Nul)
    ));
}

#[cfg(unix)]
#[test]
fn links_cannot_escape_or_be_written_through_and_privileged_bits_are_stripped() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let outer = tempfile::tempdir().unwrap();
    let root = outer.path().join("staging");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(outer.path().join("sentinel"), b"unchanged").unwrap();
    let mut dir = SafeDir::new(open_staging(&root).unwrap(), Limits::default(), 1024, true);
    write(&mut dir, "data/file").unwrap();
    assert_eq!(
        std::fs::metadata(root.join("data/file"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o755
    );
    for (i, target) in [
        "../sentinel",
        "/etc/passwd",
        "C:\\escape",
        "missing",
        "\\\\server\\share",
    ]
    .iter()
    .enumerate()
    {
        assert!(matches!(
            dir.symlink(&format!("bad{i}"), target, 0),
            Err(SafetyError::InvalidSymlinkTarget)
        ));
    }
    dir.symlink("link", "data", 0).unwrap();
    dir.symlink("chain", "link", 0).unwrap();
    dir.symlink("data/relative", "../data/file", 0).unwrap();
    assert_eq!(
        std::fs::read(root.join("data/relative")).unwrap(),
        b"payload"
    );
    assert!(matches!(
        write(&mut dir, "link/evil"),
        Err(SafetyError::WriteThroughLink(_))
    ));
    symlink("..", root.join("hostile")).unwrap();
    assert!(matches!(
        write(&mut dir, "hostile/sentinel"),
        Err(SafetyError::WriteThroughLink(_))
    ));
    assert_eq!(
        std::fs::read(outer.path().join("sentinel")).unwrap(),
        b"unchanged"
    );
}

#[test]
fn long_paths_work_through_staging_capability() {
    let temp = tempfile::tempdir().unwrap();
    let mut dir = safe(&temp, Limits::default(), true);
    let name = format!("{}/file", ["segment0123456789"; 20].join("/"));
    write(&mut dir, &name).unwrap();
    assert_eq!(dir.actual_bytes(), 7);
    assert_eq!(
        open_staging(temp.path())
            .unwrap()
            .read(sanitize_path(&name).unwrap())
            .unwrap(),
        b"payload"
    );
}

#[cfg(unix)]
#[test]
fn modes_drop_group_and_world_write_and_directories_stay_open_to_the_owner() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o7777;
    assert_eq!(safe_mode(0o777), 0o755);
    assert_eq!(safe_mode(0o666), 0o644);
    let temp = tempfile::tempdir().unwrap();
    let mut dir = safe(&temp, Limits::default(), true);
    dir.write_file("shared", Cursor::new(b"x"), Some(1), 0, 0o777)
        .unwrap();
    assert_eq!(mode(&temp.path().join("shared")), 0o755);
    dir.create_dir("locked", 0).unwrap();
    dir.set_entry_mode("locked", 0o500).unwrap();
    assert_eq!(mode(&temp.path().join("locked")), 0o700);
    dir.set_entry_mode("shared", 0o666).unwrap();
    assert_eq!(mode(&temp.path().join("shared")), 0o644);
}

#[test]
fn entries_never_land_in_the_caskade_folder() {
    let temp = tempfile::tempdir().unwrap();
    let mut dir = safe(&temp, Limits::default(), false);
    for name in [
        ".caskade/prefix/user.reg",
        ".CASKADE/install.json",
        "./.caskade/x",
        ".caskade\\prefix\\system.reg",
    ] {
        assert!(
            matches!(write(&mut dir, name), Err(SafetyError::Reserved)),
            "{name}"
        );
    }
    assert!(matches!(
        dir.create_dir(".caskade", 0),
        Err(SafetyError::Reserved)
    ));
    write(&mut dir, "data/.caskade/kept").unwrap();
    write(&mut dir, ".caskade-notes").unwrap();
    assert!(!temp.path().join(".caskade").exists());
}
