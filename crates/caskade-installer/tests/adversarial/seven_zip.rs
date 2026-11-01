//! 7z headers, encryption and the 7-Zip sidecar listing.
use super::*;

fn seven_header(header: &[u8]) -> Vec<u8> {
    let mut prefix = [0; 32];
    prefix[..6].copy_from_slice(b"7z\xbc\xaf\x27\x1c");
    prefix[7] = 4;
    prefix[20..28].copy_from_slice(&(header.len() as u64).to_le_bytes());
    u32_at(&mut prefix, 28, crc32(header));
    let start_crc = crc32(&prefix[12..]);
    u32_at(&mut prefix, 8, start_crc);
    [prefix.as_slice(), header].concat()
}

fn empty_seven(name: &str) -> Vec<u8> {
    // EmptyStream + EmptyFile bitsets, then the UTF-16LE Name property. No
    // compressor or external tool is required for this genuine 7z container.
    let mut names = vec![0];
    for unit in name.encode_utf16().chain([0]) {
        names.extend(unit.to_le_bytes());
    }
    let mut header = vec![1, 5, 1, 14, 1, 0x80, 15, 1, 0x80, 17];
    if names.len() < 128 {
        header.push(names.len() as u8);
    } else {
        header.push(0xff);
        header.extend((names.len() as u64).to_le_bytes());
    }
    header.extend(names);
    header.extend([0, 0]);
    seven_header(&header)
}

#[test]
fn seven_zip_utf16_names_use_the_same_portable_path_policy() {
    let (root, result) = attempt(
        &empty_seven("safe.txt"),
        Format::SevenZip,
        Limits::default(),
        &ArchiveOptions::default(),
        true,
    );
    result.unwrap();
    assert_eq!(fs::read(root.path().join("safe.txt")).unwrap(), b"");
    for name in [
        "../sentinel",
        "..\\sentinel",
        "/sentinel",
        "C:\\sentinel",
        "//server/share",
        "NUL.txt",
        "bad.",
        "bad ",
        "bad\0name",
    ] {
        rejected(&empty_seven(name), Format::SevenZip);
    }
}

#[test]
fn encrypted_seven_headers_and_metadata_allocation_bombs_fail_before_decoding() {
    // Correct outer CRCs, encoded header with a declared AES-256 coder. The
    // preflight must recognise encryption before allocating/initialising it.
    let aes = [
        23, 6, 0, 1, 9, 0, 0, 7, 11, 1, 0, 1, 0x24, 6, 0xf1, 7, 1, 2, 0x3f, 0, 12, 0, 0, 0,
    ];
    let (_, result) = attempt(
        &seven_header(&aes),
        Format::SevenZip,
        Limits::default(),
        &ArchiveOptions::default(),
        true,
    );
    assert!(
        matches!(result, Err(ArchiveError::Unsupported(message)) if message.contains("encrypted"))
    );
    // FilesInfo with an enormous 7z-encoded file count, not a huge allocation.
    let mut files = vec![1, 5, 0xff];
    files.extend(u64::MAX.to_le_bytes());
    files.extend([0, 0]);
    let (_, result) = attempt(
        &seven_header(&files),
        Format::SevenZip,
        Limits::default(),
        &ArchiveOptions::default(),
        true,
    );
    assert!(matches!(result, Err(ArchiveError::MemoryLimit)));
    let mut oversized = seven_header(&[1, 0]);
    oversized[20..28].copy_from_slice(&u64::MAX.to_le_bytes());
    let crc = crc32(&oversized[12..32]);
    u32_at(&mut oversized, 8, crc);
    let (_, result) = attempt(
        &oversized,
        Format::SevenZip,
        Limits::default(),
        &ArchiveOptions::default(),
        true,
    );
    assert!(matches!(result, Err(ArchiveError::MemoryLimit)));
}

#[test]
fn sidecar_listing_rejects_hostile_paths_link_graphs_and_false_sizes() {
    use archive::sevenzip::SevenZip;
    let tool = SevenZip::new("never-executed").with_case_sensitivity(false);
    for name in [
        "../sentinel",
        "a\\..\\sentinel",
        "/sentinel",
        "C:sentinel",
        "//server/share",
        "NUL",
        "bad. ",
        "a:b",
    ] {
        let root = TestRoot::new().unwrap();
        let listing = format!("Path = {name}\nSize = 1\nAttributes = A\n\n");
        let result = tool.parse_listing(listing.as_bytes(), 1024);
        root.assert_clean();
        assert!(result.is_err(), "{name}");
    }
    for listing in [
        "Path = GAME\nSize = 1\n\nPath = game\nSize = 1\n\n",
        "Path = café\nSize = 1\n\nPath = cafe\u{301}\nSize = 1\n\n",
        "Path = link\nSize = 0\nSymbolic Link = ../sentinel\n\n",
        "Path = a\nSize = 0\nSymbolic Link = b\n\nPath = b\nSize = 0\nSymbolic Link = a\n\n",
        "Path = hard\nSize = 0\nHard Link = missing\n\n",
        "Path = file\nSize = 18446744073709551615\n\n",
        "Path = file\nSize = 1\nSize = 2\n\n",
        "Path = file\nSize = 1\nEncrypted = +\n\n",
    ] {
        let root = TestRoot::new().unwrap();
        let result = tool.parse_listing(listing.as_bytes(), 1024);
        root.assert_clean();
        assert!(result.is_err(), "{listing}");
    }
}

#[test]
#[ignore = "T5 native sidecar: generates a real encrypted 7z using the pinned official binary"]
fn generated_password_encrypted_seven_zip_is_a_clean_error() {
    use archive::sevenzip::{SevenZip, SevenZipError};
    let program =
        fs::canonicalize(std::env::var_os("CASKADE_7ZZ").expect("set CASKADE_7ZZ")).unwrap();
    let tool = SevenZip::pinned(&program).unwrap();
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("payload"), b"encrypted payload").unwrap();
    let output = std::process::Command::new(program)
        .current_dir(temp.path())
        .args([
            "a",
            "-t7z",
            "-psecret",
            "-mhe=on",
            "encrypted.7z",
            "payload",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let input = temp.path().join("encrypted.7z");
    let bytes = fs::read(&input).unwrap();
    let (root, result) = attempt(
        &bytes,
        Format::SevenZip,
        Limits::default(),
        &ArchiveOptions::default(),
        true,
    );
    assert!(matches!(result, Err(ArchiveError::Unsupported(_))));
    let mut dir = SafeDir::new(
        open_staging(root.path()).unwrap(),
        Limits::default(),
        bytes.len() as u64,
        true,
    );
    let result = tool.extract(&input, root.path(), &mut dir, &mut NoopControl);
    confined(&root);
    assert!(matches!(result, Err(SevenZipError::Encrypted)));
    root.assert_clean();
}
