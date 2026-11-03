//! T5: generated hostile archives. Every attempt checks the outside sentinel,
//! including clean failures and partially extracted trees awaiting rollback.
use caskade_installer::{
    ArtifactNode, Format,
    formats::archive::{self, ArchiveError, ArchiveOptions, NoopControl},
    safety::{Limits, NameEncoding, SafeDir, SafetyError, open_staging, sanitize_path},
    testing::TestRoot,
};
use std::{
    fs,
    io::{BufWriter, Cursor, Write},
    path::Path,
};

use crate::fixtures;

mod budgets;
mod corpus;
mod paths;
mod seven_zip;

fn confined(root: &TestRoot) {
    fn walk(root: &Path, dir: &Path) {
        for item in fs::read_dir(dir).unwrap() {
            let path = item.unwrap().path();
            let relative = path.strip_prefix(root).unwrap();
            sanitize_path(relative.to_str().unwrap()).unwrap();
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert!(metadata.is_file() || metadata.is_dir() || metadata.is_symlink());
            // std on both sides: dunce keeps the \\?\ prefix only for names that
            // need it, and these tests make exactly such names.
            assert!(fs::canonicalize(&path).unwrap().starts_with(root));
            if metadata.is_dir() && !metadata.is_symlink() {
                walk(root, &path);
            }
            #[cfg(unix)]
            if metadata.is_file() {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(metadata.permissions().mode() & 0o6000, 0);
            }
        }
    }
    let canonical = fs::canonicalize(root.path()).unwrap();
    walk(&canonical, &canonical);
    root.assert_sentinel_untouched();
    let mut siblings: Vec<_> = fs::read_dir(root.parent_path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    siblings.sort();
    assert_eq!(siblings, ["root", "sentinel"]);
}

fn attempt(
    bytes: &[u8],
    format: Format,
    limits: Limits,
    options: &ArchiveOptions,
    sensitive: bool,
) -> (TestRoot, archive::Result<Vec<ArtifactNode>>) {
    let root = TestRoot::new().unwrap();
    let mut dir = SafeDir::new(
        open_staging(root.path()).unwrap(),
        limits,
        bytes.len() as u64,
        sensitive,
    );
    let result = archive::extract(
        Cursor::new(bytes),
        format,
        &mut dir,
        options,
        &mut NoopControl,
    );
    confined(&root);
    assert!(dir.actual_bytes() <= limits.max_total_bytes);
    (root, result)
}

fn rejected(bytes: &[u8], format: Format) {
    let (_, result) = attempt(
        bytes,
        format,
        Limits::default(),
        &ArchiveOptions::default(),
        true,
    );
    assert!(result.is_err(), "hostile {format:?} accepted");
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn u16_at(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn u32_at(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

struct ZipEntry<'a> {
    name: &'a [u8],
    body: &'a [u8],
    mode: u32,
    utf8: bool,
}

impl<'a> ZipEntry<'a> {
    fn file(name: &'a [u8], body: &'a [u8]) -> Self {
        Self {
            name,
            body,
            mode: 0o100644,
            utf8: true,
        }
    }
}

fn local(name: &[u8], body: &[u8], utf8: bool) -> [u8; 30] {
    let mut h = [0; 30];
    h[..4].copy_from_slice(b"PK\x03\x04");
    u16_at(&mut h, 4, 20);
    u16_at(&mut h, 6, if utf8 { 0x800 } else { 0 });
    u32_at(&mut h, 14, crc32(body));
    u32_at(&mut h, 18, body.len() as u32);
    u32_at(&mut h, 22, body.len() as u32);
    u16_at(&mut h, 26, name.len() as u16);
    h
}

fn central(entry: &ZipEntry<'_>, offset: u32) -> [u8; 46] {
    let mut h = [0; 46];
    h[..4].copy_from_slice(b"PK\x01\x02");
    u16_at(&mut h, 4, 3 * 256 + 20);
    u16_at(&mut h, 6, 20);
    u16_at(&mut h, 8, if entry.utf8 { 0x800 } else { 0 });
    u32_at(&mut h, 16, crc32(entry.body));
    u32_at(&mut h, 20, entry.body.len() as u32);
    u32_at(&mut h, 24, entry.body.len() as u32);
    u16_at(&mut h, 28, entry.name.len() as u16);
    u32_at(&mut h, 38, entry.mode << 16);
    u32_at(&mut h, 42, offset);
    h
}

fn raw_zip(entries: &[ZipEntry<'_>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut offsets = Vec::new();
    for entry in entries {
        offsets.push(bytes.len() as u32);
        bytes.extend(local(entry.name, entry.body, entry.utf8));
        bytes.extend(entry.name);
        bytes.extend(entry.body);
    }
    let start = bytes.len();
    for (entry, offset) in entries.iter().zip(offsets) {
        bytes.extend(central(entry, offset));
        bytes.extend(entry.name);
    }
    let mut end = [0; 22];
    end[..4].copy_from_slice(b"PK\x05\x06");
    u16_at(&mut end, 8, entries.len() as u16);
    u16_at(&mut end, 10, entries.len() as u16);
    u32_at(&mut end, 12, (bytes.len() - start) as u32);
    u32_at(&mut end, 16, start as u32);
    bytes.extend(end);
    bytes
}

fn tar_record(kind: u8, name: &[u8], target: &[u8], body: &[u8]) -> Vec<u8> {
    assert!(name.len() <= 100 && target.len() <= 100);
    let mut h = tar::Header::new_gnu();
    h.as_mut_bytes()[..name.len()].copy_from_slice(name);
    h.as_mut_bytes()[157..157 + target.len()].copy_from_slice(target);
    h.set_entry_type(tar::EntryType::new(kind));
    h.set_size(body.len() as u64);
    h.set_mode(0o6755);
    h.set_cksum();
    let mut bytes = h.as_bytes().to_vec();
    bytes.extend(body);
    bytes.resize(bytes.len().div_ceil(512) * 512, 0);
    bytes
}

fn raw_tar(records: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = records.concat();
    bytes.extend([0; 1024]);
    bytes
}

fn format(name: &str) -> Format {
    match name {
        "zip" => Format::Zip,
        "7z" | "7z-multivolume" => Format::SevenZip,
        "tar" => Format::Tar,
        "tar.gz" => Format::Gzip,
        "tar.xz" => Format::Xz,
        "tar.bz2" => Format::Bzip2,
        "tar.zst" => Format::Zstd,
        other => panic!("unhandled fixture format {other}"),
    }
}
