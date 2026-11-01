//! Expansion ratio, byte, entry and nesting budgets.
use super::*;

fn deflated(name: &str, body: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(name, zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(body).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn ratio_total_entries_depth_and_lying_sizes_are_bounded() {
    let bomb = deflated("zeros", &vec![0; 1024 * 1024]);
    let limits = Limits {
        max_expansion_ratio: 2,
        ..Limits::default()
    };
    let (_, result) = attempt(&bomb, Format::Zip, limits, &ArchiveOptions::default(), true);
    assert!(matches!(
        result,
        Err(ArchiveError::Safety(SafetyError::ExpansionRatio))
    ));
    let bytes = raw_zip(&[
        ZipEntry::file(b"one", &[0; 40]),
        ZipEntry::file(b"two", &[0; 40]),
    ]);
    for limits in [
        Limits {
            max_total_bytes: 64,
            ..Limits::default()
        },
        Limits {
            max_entries: 1,
            ..Limits::default()
        },
    ] {
        let (_, result) = attempt(
            &bytes,
            Format::Zip,
            limits,
            &ArchiveOptions::default(),
            true,
        );
        assert!(result.is_err());
    }
    for (limits, options, name) in [
        (
            Limits {
                max_depth: 2,
                ..Limits::default()
            },
            ArchiveOptions {
                depth: 3,
                ..ArchiveOptions::default()
            },
            "file",
        ),
        (
            Limits {
                max_path_depth: 2,
                ..Limits::default()
            },
            ArchiveOptions::default(),
            "a/b/file",
        ),
    ] {
        let bytes = raw_zip(&[ZipEntry::file(name.as_bytes(), b"x")]);
        let (_, result) = attempt(&bytes, Format::Zip, limits, &options, true);
        assert!(matches!(
            result,
            Err(ArchiveError::Safety(SafetyError::DepthLimit))
        ));
    }
    for claimed in [1_u32, 10_000] {
        let mut bytes = raw_zip(&[ZipEntry::file(b"file", b"actual payload")]);
        let central = bytes.windows(4).position(|s| s == b"PK\x01\x02").unwrap();
        u32_at(&mut bytes, central + 24, claimed);
        rejected(&bytes, Format::Zip);
    }
    let mut truncated = tar_record(b'0', b"file", b"", b"actual payload");
    truncated.truncate(515);
    rejected(&truncated, Format::Tar);
    // Actual streams with no size claim still consume the shared byte budget.
    let root = TestRoot::new().unwrap();
    let mut dir = SafeDir::new(
        open_staging(root.path()).unwrap(),
        Limits {
            max_total_bytes: 32,
            ..Limits::default()
        },
        1024,
        true,
    );
    let result = dir.write_file("file", Cursor::new([0; 64]), None, 0, 0o644);
    confined(&root);
    assert!(matches!(result, Err(SafetyError::ByteLimit)));
    assert!(!root.path().join("file").exists());
}

#[test]
fn overlapping_zip_members_cannot_bypass_output_budget() {
    let payload = [b'x'; 128 * 1024];
    let mut bytes = raw_zip(&[
        ZipEntry::file(b"a", &payload),
        ZipEntry::file(b"b", &payload),
    ]);
    let central = bytes.windows(4).position(|s| s == b"PK\x01\x02").unwrap();
    // Two central entries reference exactly the same local header/payload.
    u32_at(&mut bytes, central + 47 + 42, 0);
    let (_, result) = attempt(
        &bytes,
        Format::Zip,
        Limits {
            max_total_bytes: payload.len() as u64,
            ..Limits::default()
        },
        &ArchiveOptions::default(),
        true,
    );
    assert!(result.is_err());
}

#[test]
fn million_entry_zip64_is_rejected_before_entry_table_allocation() {
    let root = TestRoot::new().unwrap();
    // Generate a structurally complete ZIP64 in a separate temp directory. The
    // million empty members are streamed; no million-element Vec is allocated.
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("million.zip");
    let mut file = BufWriter::new(fs::File::create(&path).unwrap());
    let count = 1_000_001_u64;
    for i in 0..count {
        let name = format!("{i:08}");
        file.write_all(&local(name.as_bytes(), b"", true)).unwrap();
        file.write_all(name.as_bytes()).unwrap();
    }
    let start = count * 38;
    for i in 0..count {
        let name = format!("{i:08}");
        file.write_all(&central(
            &ZipEntry::file(name.as_bytes(), b""),
            (i * 38) as u32,
        ))
        .unwrap();
        file.write_all(name.as_bytes()).unwrap();
    }
    let size = count * 54;
    let mut zip64 = [0; 56];
    zip64[..4].copy_from_slice(b"PK\x06\x06");
    zip64[4..12].copy_from_slice(&44_u64.to_le_bytes());
    u16_at(&mut zip64, 12, 45);
    u16_at(&mut zip64, 14, 45);
    zip64[24..32].copy_from_slice(&count.to_le_bytes());
    zip64[32..40].copy_from_slice(&count.to_le_bytes());
    zip64[40..48].copy_from_slice(&size.to_le_bytes());
    zip64[48..56].copy_from_slice(&start.to_le_bytes());
    file.write_all(&zip64).unwrap();
    let mut locator = [0; 20];
    locator[..4].copy_from_slice(b"PK\x06\x07");
    locator[8..16].copy_from_slice(&(start + size).to_le_bytes());
    u32_at(&mut locator, 16, 1);
    file.write_all(&locator).unwrap();
    let mut end = [0; 22];
    end[..4].copy_from_slice(b"PK\x05\x06");
    u16_at(&mut end, 8, u16::MAX);
    u16_at(&mut end, 10, u16::MAX);
    u32_at(&mut end, 12, size as u32);
    u32_at(&mut end, 16, start as u32);
    file.write_all(&end).unwrap();
    file.flush().unwrap();
    let mut dir = SafeDir::new(
        open_staging(root.path()).unwrap(),
        Limits::default(),
        path.metadata().unwrap().len(),
        true,
    );
    let result = archive::extract(
        fs::File::open(path).unwrap(),
        Format::Zip,
        &mut dir,
        &ArchiveOptions::default(),
        &mut NoopControl,
    );
    confined(&root);
    assert!(matches!(
        result,
        Err(ArchiveError::Safety(SafetyError::EntryLimit))
    ));
    root.assert_clean();
}

#[test]
fn nested_archives_share_total_budget_and_enforce_container_depth() {
    let mut bytes = raw_zip(&[ZipEntry::file(b"payload", &[0; 32])]);
    for _ in 0..4 {
        bytes = raw_zip(&[ZipEntry::file(b"inner.zip", &bytes)]);
    }
    let root = TestRoot::new().unwrap();
    let mut dir = SafeDir::new(
        open_staging(root.path()).unwrap(),
        Limits {
            max_depth: 2,
            ..Limits::default()
        },
        bytes.len() as u64,
        true,
    );
    for depth in 0..=3 {
        let options = ArchiveOptions {
            depth,
            strip: None,
            ..ArchiveOptions::default()
        };
        // Different selected output prefixes are achieved by separate member
        // names, keeping one SafeDir budget across all container depths.
        let archive = raw_zip(&[ZipEntry::file(
            format!("level{depth}/inner.zip").as_bytes(),
            &bytes,
        )]);
        let result = archive::extract(
            Cursor::new(archive),
            Format::Zip,
            &mut dir,
            &options,
            &mut NoopControl,
        );
        confined(&root);
        if depth == 3 {
            assert!(matches!(
                result,
                Err(ArchiveError::Safety(SafetyError::DepthLimit))
            ));
        } else {
            result.unwrap();
            // Recurse into the actual archived child rather than trusting a suffix.
            let mut inner = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
            let mut next = Vec::new();
            std::io::Read::read_to_end(&mut inner.by_index(0).unwrap(), &mut next).unwrap();
            bytes = next;
        }
    }
}
