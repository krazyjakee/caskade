//! Entry names, encodings, collisions and links.
use super::*;

#[test]
fn traversal_absolute_drives_unc_devices_and_hostile_names() {
    for name in [
        "../sentinel",
        "a/../../sentinel",
        "..\\sentinel",
        "a\\..\\..\\sentinel",
        "/sentinel",
        "\\sentinel",
        "C:/sentinel",
        "C:sentinel",
        "z:\\sentinel",
        "//server/share/sentinel",
        "\\\\server\\share\\sentinel",
        "\\\\?\\C:\\sentinel",
        "\\\\.\\NUL",
        "NUL",
        "con.txt",
        "COM1.dat",
        "LPT9",
        "COM¹",
        "CONIN$",
        "trailing.",
        "trailing ",
        "folder. /x",
        "a:b",
        "a\nb",
        "a\u{7f}b",
        "a*b",
        "a?b",
        "a|b",
        "a<b",
        "a\"b",
        "",
        ".",
        "./",
        "dir/../sentinel",
    ] {
        let zip = raw_zip(&[ZipEntry::file(name.as_bytes(), b"changed")]);
        let (_, result) = attempt(
            &zip,
            Format::Zip,
            Limits::default(),
            &ArchiveOptions::default(),
            true,
        );
        assert!(result.is_err(), "ZIP name {name:?}");
        let tar = raw_tar(&[tar_record(b'0', name.as_bytes(), b"", b"changed")]);
        let (root, result) = attempt(
            &tar,
            Format::Tar,
            Limits::default(),
            &ArchiveOptions::default(),
            true,
        );
        if matches!(name, "." | "./") {
            // A TAR root marker is safely ignored; it creates no leaf file.
            if result.is_ok() {
                root.assert_clean();
            }
        } else {
            assert!(result.is_err(), "TAR name {name:?}");
        }
    }
    for name in [b"a\0b".as_slice(), b"\xffbad"] {
        rejected(&raw_zip(&[ZipEntry::file(name, b"payload")]), Format::Zip);
    }
}

#[test]
fn encoded_names_are_decoded_before_portable_path_validation() {
    let sjis = encoding_rs::SHIFT_JIS.encode("ソフト.txt").0.into_owned();
    for (name, expected, encoding) in [
        (b"caf\x82.txt".to_vec(), "café.txt", NameEncoding::Cp437),
        (sjis, "ソフト.txt", NameEncoding::Label(b"shift_jis")),
    ] {
        let bytes = raw_zip(&[ZipEntry {
            name: &name,
            body: b"payload",
            mode: 0o100644,
            utf8: false,
        }]);
        let options = ArchiveOptions {
            legacy_zip_encoding: encoding,
            ..ArchiveOptions::default()
        };
        let (root, result) = attempt(&bytes, Format::Zip, Limits::default(), &options, true);
        result.unwrap();
        assert_eq!(fs::read(root.path().join(expected)).unwrap(), b"payload");
        // The Shift-JIS trail byte 0x5c is part of ソ, not a path separator.
        for hostile in [b"..\\sentinel".as_slice(), b"NUL.txt", b"bad\0name"] {
            let bytes = raw_zip(&[ZipEntry {
                name: hostile,
                body: b"changed",
                mode: 0o100644,
                utf8: false,
            }]);
            let (_, result) = attempt(&bytes, Format::Zip, Limits::default(), &options, true);
            assert!(result.is_err());
        }
    }
    let options = ArchiveOptions {
        legacy_zip_encoding: NameEncoding::Label(b"shift_jis"),
        ..ArchiveOptions::default()
    };
    let bytes = raw_zip(&[ZipEntry {
        name: b"\x81",
        body: b"x",
        mode: 0o100644,
        utf8: false,
    }]);
    let (_, result) = attempt(&bytes, Format::Zip, Limits::default(), &options, true);
    assert!(result.is_err());
}

#[test]
fn unicode_normalisation_case_ancestors_and_duplicate_collisions() {
    for (first, second, sensitive) in [
        ("café", "cafe\u{301}", true),
        ("GAME.exe", "game.exe", false),
        ("Foo/a", "foo/b", false),
        ("straße", "STRASSE", false),
        ("same", "same", true),
        ("file", "file/child", true),
    ] {
        let zip = raw_zip(&[
            ZipEntry::file(first.as_bytes(), b"one"),
            ZipEntry::file(second.as_bytes(), b"two"),
        ]);
        let (_, result) = attempt(
            &zip,
            Format::Zip,
            Limits::default(),
            &ArchiveOptions::default(),
            sensitive,
        );
        if first == second {
            // zip's index deduplicates identical byte names. It may expose a
            // single safe member; spelling aliases must still fail below.
            if let Ok(artifacts) = result {
                assert_eq!(artifacts.len(), 1);
            }
        } else {
            assert!(result.is_err(), "{first:?}, {second:?}");
        }
    }
    let name = format!(
        "{}/game",
        ["longcomponent01234567890123456789"; 12].join("/")
    );
    assert!(name.len() > 260);
    let bytes = raw_zip(&[ZipEntry::file(name.as_bytes(), b"long path")]);
    let (root, result) = attempt(
        &bytes,
        Format::Zip,
        Limits::default(),
        &ArchiveOptions::default(),
        true,
    );
    result.unwrap();
    assert_eq!(fs::read(root.path().join(name)).unwrap(), b"long path");
    let overlong = "x".repeat(300);
    rejected(
        &raw_zip(&[ZipEntry::file(overlong.as_bytes(), b"x")]),
        Format::Zip,
    );
}

#[test]
fn tar_links_chains_forward_hardlinks_and_write_through_attempts() {
    let cases = [
        vec![tar_record(b'2', b"escape", b"../sentinel", b"")],
        vec![tar_record(b'2', b"escape", b"/sentinel", b"")],
        vec![tar_record(b'2', b"escape", b"C:\\sentinel", b"")],
        vec![
            tar_record(b'2', b"a", b"b", b""),
            tar_record(b'2', b"b", b"a", b""),
        ],
        vec![
            tar_record(b'2', b"a", b"b", b""),
            tar_record(b'2', b"b", b"../sentinel", b""),
        ],
        vec![tar_record(b'1', b"hard", b"../sentinel", b"")],
        vec![
            tar_record(b'1', b"hard", b"later", b""),
            tar_record(b'0', b"later", b"", b"x"),
        ],
        vec![
            tar_record(b'2', b"link", b"dir", b""),
            tar_record(b'0', b"link/file", b"", b"x"),
            tar_record(b'5', b"dir", b"", b""),
        ],
    ];
    for records in cases {
        rejected(&raw_tar(&records), Format::Tar);
    }
    let bytes = raw_tar(&[
        tar_record(b'0', b"data", b"", b"payload"),
        tar_record(b'1', b"hard", b"data", b""),
        tar_record(b'1', b"hard2", b"hard", b""),
    ]);
    let (root, result) = attempt(
        &bytes,
        Format::Tar,
        Limits::default(),
        &ArchiveOptions::default(),
        true,
    );
    result.unwrap();
    assert_eq!(fs::read(root.path().join("hard2")).unwrap(), b"payload");
}

#[test]
fn existing_symlinks_cannot_redirect_capability_writes() {
    let root = TestRoot::new().unwrap();
    fs::create_dir(root.path().join("inside")).unwrap();
    let mut dir = SafeDir::new(
        open_staging(root.path()).unwrap(),
        Limits::default(),
        1024,
        true,
    );
    match dir.symlink("alias", "inside", 0) {
        Ok(_) => assert!(matches!(
            dir.write_file("alias/file", Cursor::new(b"x"), Some(1), 0, 0o644),
            Err(SafetyError::WriteThroughLink(_))
        )),
        // Windows without symlink privilege still returns a clean error.
        Err(SafetyError::Io(_)) if cfg!(windows) => (),
        Err(error) => panic!("{error}"),
    }
    confined(&root);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.sentinel_path(), root.path().join("outside")).unwrap();
        let result = dir.write_file("outside", Cursor::new(b"changed"), Some(7), 0, 0o644);
        root.assert_sentinel_untouched();
        assert!(matches!(result, Err(SafetyError::WriteThroughLink(_))));
        fs::remove_file(root.path().join("outside")).unwrap();
        confined(&root);
    }
}

#[test]
fn special_files_and_zip_links_are_rejected() {
    for kind in *b"346s" {
        rejected(
            &raw_tar(&[tar_record(kind, b"special", b"", b"")]),
            Format::Tar,
        );
    }
    for mode in [0o020644, 0o060644, 0o010644, 0o140644] {
        rejected(
            &raw_zip(&[ZipEntry {
                name: b"special",
                body: b"",
                mode,
                utf8: true,
            }]),
            Format::Zip,
        );
    }
    for target in [
        b"../sentinel".as_slice(),
        b"/sentinel",
        b"C:\\sentinel",
        b"missing",
        b"bad\0target",
    ] {
        rejected(
            &raw_zip(&[ZipEntry {
                name: b"link",
                body: target,
                mode: 0o120777,
                utf8: true,
            }]),
            Format::Zip,
        );
    }
}
