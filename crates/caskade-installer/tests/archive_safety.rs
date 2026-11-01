use caskade_installer::{
    Format,
    formats::archive::*,
    safety::{Limits, SafeDir, open_staging},
};
use std::io::{Cursor, Write};

fn zip_data(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    zip.start_file(name, zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(contents).unwrap();
    zip.finish().unwrap().into_inner()
}
fn root() -> (tempfile::TempDir, std::path::PathBuf) {
    let outer = tempfile::tempdir().unwrap();
    let staging = outer.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    std::fs::write(outer.path().join("sentinel"), b"unchanged").unwrap();
    (outer, staging)
}
#[test]
fn hostile_zip_names_and_bomb_limits_cannot_touch_sentinel() {
    for name in [
        "../sentinel",
        "/sentinel",
        "C:\\sentinel",
        "folder\\..\\sentinel",
        "NUL.txt",
        "trailing.",
    ] {
        let (outer, staging) = root();
        let bytes = zip_data(name, b"changed");
        let mut dir = SafeDir::new(
            open_staging(&staging).unwrap(),
            Limits::default(),
            bytes.len() as u64,
            true,
        );
        assert!(
            extract(
                Cursor::new(bytes),
                Format::Zip,
                &mut dir,
                &ArchiveOptions::default(),
                &mut NoopControl
            )
            .is_err(),
            "{name}"
        );
        assert_eq!(
            std::fs::read(outer.path().join("sentinel")).unwrap(),
            b"unchanged"
        );
    }
    let (outer, staging) = root();
    let bytes = zip_data("bomb", &vec![0; 1024 * 1024]);
    let mut dir = SafeDir::new(
        open_staging(&staging).unwrap(),
        Limits {
            max_expansion_ratio: 2,
            ..Limits::default()
        },
        bytes.len() as u64,
        true,
    );
    assert!(
        extract(
            Cursor::new(bytes),
            Format::Zip,
            &mut dir,
            &ArchiveOptions::default(),
            &mut NoopControl
        )
        .is_err()
    );
    assert!(!staging.join("bomb").exists());
    assert_eq!(
        std::fs::read(outer.path().join("sentinel")).unwrap(),
        b"unchanged"
    );
}

fn tar_data(kind: u8, name: &str, target: Option<&str>, contents: &[u8], mode: u32) -> Vec<u8> {
    let mut header = tar::Header::new_gnu();
    // Raw name bytes intentionally avoid the builder's path sanitiser.
    header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
    header.set_size(contents.len() as u64);
    header.set_mode(mode);
    header.set_entry_type(tar::EntryType::new(kind));
    if let Some(target) = target {
        header.set_link_name(target).unwrap();
    }
    header.set_cksum();
    let mut bytes = header.as_bytes().to_vec();
    bytes.extend(contents);
    bytes.resize(bytes.len().div_ceil(512) * 512, 0);
    bytes.extend([0; 1024]);
    bytes
}
#[test]
fn tar_traversal_links_devices_fifos_and_truncation_are_rejected() {
    for bytes in [
        tar_data(b'0', "../sentinel", None, b"changed", 0o644),
        tar_data(b'2', "escape", Some("../sentinel"), b"", 0o777),
        tar_data(b'1', "hard", Some("../sentinel"), b"", 0o644),
        tar_data(b'3', "device", None, b"", 0o644),
        tar_data(b'6', "fifo", None, b"", 0o644),
    ] {
        let (outer, staging) = root();
        let mut dir = SafeDir::new(
            open_staging(&staging).unwrap(),
            Limits::default(),
            bytes.len() as u64,
            true,
        );
        assert!(
            extract(
                Cursor::new(bytes),
                Format::Tar,
                &mut dir,
                &ArchiveOptions::default(),
                &mut NoopControl
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read(outer.path().join("sentinel")).unwrap(),
            b"unchanged"
        );
    }
    let bytes = tar_data(b'0', "file", None, b"payload", 0o6755);
    let truncated = bytes[..515].to_vec();
    let (_outer, staging) = root();
    let mut dir = SafeDir::new(
        open_staging(&staging).unwrap(),
        Limits::default(),
        truncated.len() as u64,
        true,
    );
    assert!(
        extract(
            Cursor::new(truncated),
            Format::Tar,
            &mut dir,
            &ArchiveOptions::default(),
            &mut NoopControl
        )
        .is_err()
    );
    assert!(!staging.join("file").exists());
}
#[test]
fn bounded_gnu_and_pax_long_names_and_deferred_links() {
    let long = format!("{}/file", ["longcomponent"; 12].join("/"));
    let mut bytes = vec![];
    {
        let mut builder = tar::Builder::new(&mut bytes);
        let mut h = tar::Header::new_gnu();
        h.set_size(7);
        h.set_mode(0o644);
        h.set_cksum();
        builder
            .append_data(&mut h, &long, Cursor::new(b"payload"))
            .unwrap();
        builder.finish().unwrap();
    }
    let (_outer, staging) = root();
    let mut dir = SafeDir::new(
        open_staging(&staging).unwrap(),
        Limits::default(),
        bytes.len() as u64,
        true,
    );
    extract(
        Cursor::new(bytes),
        Format::Tar,
        &mut dir,
        &ArchiveOptions::default(),
        &mut NoopControl,
    )
    .unwrap();
    assert_eq!(std::fs::read(staging.join(&long)).unwrap(), b"payload");
    let too_large = tar_data(b'L', "././@LongLink", None, &vec![b'a'; 1024], 0o644);
    let options = ArchiveOptions {
        max_metadata_bytes: 512,
        ..ArchiveOptions::default()
    };
    assert!(matches!(
        list(Cursor::new(too_large), Format::Tar, &options),
        Err(ArchiveError::MemoryLimit)
    ));
}

#[cfg(unix)]
#[test]
fn symlink_chains_are_deferred_and_privileged_modes_are_removed() {
    let mut bytes = vec![];
    {
        let mut builder = tar::Builder::new(&mut bytes);
        for (name, target) in [("chain", "link"), ("link", "data")] {
            let mut h = tar::Header::new_gnu();
            h.set_size(0);
            h.set_mode(0o777);
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_cksum();
            builder.append_link(&mut h, name, target).unwrap();
        }
        let mut h = tar::Header::new_gnu();
        h.set_size(7);
        h.set_mode(0o6755);
        h.set_cksum();
        builder
            .append_data(&mut h, "data", Cursor::new(b"payload"))
            .unwrap();
        builder.finish().unwrap();
    }
    let (_outer, staging) = root();
    let mut dir = SafeDir::new(
        open_staging(&staging).unwrap(),
        Limits::default(),
        bytes.len() as u64,
        true,
    );
    extract(
        Cursor::new(bytes),
        Format::Tar,
        &mut dir,
        &ArchiveOptions::default(),
        &mut NoopControl,
    )
    .unwrap();
    assert_eq!(std::fs::read(staging.join("chain")).unwrap(), b"payload");
    assert_eq!(
        std::fs::read_link(staging.join("chain")).unwrap(),
        std::path::Path::new("link")
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(staging.join("data"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o755
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "T10: generates and extracts a 3 GiB sparse TAR; run explicitly in the heavy lane"]
fn three_gib_archive_keeps_peak_rss_below_256_mib() {
    use std::io::{Read, Seek, SeekFrom};
    let temp = tempfile::tempdir().unwrap();
    let archive_path = temp.path().join("large.tar");
    let size = 3_u64 * 1024 * 1024 * 1024;
    let mut header = tar::Header::new_gnu();
    header.set_path("payload.bin").unwrap();
    header.set_size(size);
    header.set_mode(0o644);
    header.set_cksum();
    let mut archive = std::fs::File::create(&archive_path).unwrap();
    archive.write_all(header.as_bytes()).unwrap();
    archive.seek(SeekFrom::Start(512 + size)).unwrap();
    archive.write_all(&[0; 1024]).unwrap();
    drop(archive);
    let staging = temp.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    let packed = archive_path.metadata().unwrap().len();
    let mut safe = SafeDir::new(
        open_staging(&staging).unwrap(),
        Limits::default(),
        packed,
        true,
    );
    extract(
        std::fs::File::open(&archive_path).unwrap(),
        Format::Tar,
        &mut safe,
        &ArchiveOptions::default(),
        &mut NoopControl,
    )
    .unwrap();
    assert_eq!(staging.join("payload.bin").metadata().unwrap().len(), size);
    assert_eq!(safe.actual_bytes(), size);
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let peak_kib: u64 = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    eprintln!("3 GiB archive peak RSS: {peak_kib} KiB");
    assert!(peak_kib < 256 * 1024, "peak RSS {peak_kib} KiB");
    let mut file = std::fs::File::open(staging.join("payload.bin")).unwrap();
    file.seek(SeekFrom::End(-4)).unwrap();
    let mut tail = [1; 4];
    file.read_exact(&mut tail).unwrap();
    assert_eq!(tail, [0; 4]);
}

#[test]
fn pax_paths_are_applied_and_non_executable_tar_modes_are_preserved() {
    let name = format!("{}/script", ["paxcomponent"; 12].join("/"));
    let mut bytes = vec![];
    {
        let mut builder = tar::Builder::new(&mut bytes);
        builder
            .append_pax_extensions([("path", name.as_bytes())])
            .unwrap();
        let mut h = tar::Header::new_ustar();
        h.set_size(10);
        h.set_mode(0o644);
        h.set_cksum();
        builder
            .append_data(&mut h, "placeholder", Cursor::new(b"#!/bin/sh\n"))
            .unwrap();
        builder.finish().unwrap();
    }
    let (_outer, staging) = root();
    let mut dir = SafeDir::new(
        open_staging(&staging).unwrap(),
        Limits::default(),
        bytes.len() as u64,
        true,
    );
    extract(
        Cursor::new(bytes),
        Format::Tar,
        &mut dir,
        &ArchiveOptions::default(),
        &mut NoopControl,
    )
    .unwrap();
    assert_eq!(std::fs::read(staging.join(&name)).unwrap(), b"#!/bin/sh\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(staging.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
    }
}

#[test]
fn compressed_tar_checks_trailers_and_seven_dictionary_limits() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin");
    for (name, format) in [
        ("humble-linux.tar.gz", Format::Gzip),
        ("linux.tar.xz", Format::Xz),
        ("linux.tar.bz2", Format::Bzip2),
    ] {
        let mut bytes = std::fs::read(root.join(name)).unwrap();
        bytes.pop();
        let (_outer, staging) = self::root();
        let mut dir = SafeDir::new(
            open_staging(&staging).unwrap(),
            Limits::default(),
            bytes.len() as u64,
            true,
        );
        assert!(
            extract(
                Cursor::new(bytes),
                format,
                &mut dir,
                &ArchiveOptions::default(),
                &mut NoopControl
            )
            .is_err(),
            "{name}"
        );
    }
    let options = ArchiveOptions {
        max_decoder_bytes: 1,
        ..ArchiveOptions::default()
    };
    assert!(matches!(
        list_path(&root.join("unity.7z"), &options),
        Err(ArchiveError::MemoryLimit)
    ));
}

#[test]
fn lazy_archive_listing_uses_probe_read_budget_and_attaches_children() {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/unity.zip");
    let mut engine = caskade_installer::probe::ProbeEngine::default();
    engine.register(Box::new(ArchiveProbe));
    let mut probe = engine.probe_path(&source).unwrap();
    assert!(!probe.root.children_complete);
    assert!(probe.root.children.is_empty());
    let id = probe.root.id.clone();
    let stats = engine
        .list_children(
            &mut std::fs::File::open(source).unwrap(),
            &mut probe.root,
            &id,
        )
        .unwrap();
    assert!(probe.root.children_complete);
    assert!(!probe.root.children.is_empty());
    assert!(stats.bytes_read < 8 * 1024 * 1024);
}
