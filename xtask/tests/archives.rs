use std::{fs, io::Read};
#[cfg(unix)]
use tempfile::TempDir;
use xtask::{
    checks::FIXTURE_ROOT,
    fixtures::{self, EntryKind},
    generate,
};

// These validate the builder's committed ZIP/tar payloads, without calling any
// extraction code in caskade-installer or requiring external tools at test time.
#[test]
fn committed_zips_contain_the_golden_bytes_modes_and_symlinks() {
    for fixture in fixtures::load_all(&xtask::workspace_root().join(FIXTURE_ROOT))
        .unwrap()
        .into_iter()
        .filter(|f| f.expected.format == "zip")
    {
        let mut archive = zip::ZipArchive::new(fs::File::open(fixture.input()).unwrap()).unwrap();
        assert_eq!(archive.len(), fixture.tree.len());
        for expected in fixture.tree {
            let name = if expected.kind == EntryKind::Directory {
                format!("{}/", expected.path)
            } else {
                expected.path.clone()
            };
            let mut file = archive.by_name(&name).unwrap();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            match expected.kind {
                EntryKind::Directory => assert!(file.is_dir()),
                EntryKind::Symlink => {
                    assert_eq!(file.unix_mode().unwrap() & 0o170000, 0o120000);
                    assert_eq!(
                        String::from_utf8(bytes).unwrap(),
                        expected.symlink_target.unwrap()
                    );
                }
                EntryKind::File => {
                    use sha2::{Digest, Sha256};
                    assert_eq!(bytes.len() as u64, expected.size);
                    assert_eq!(
                        format!("{:x}", Sha256::digest(bytes)),
                        expected.sha256.unwrap()
                    );
                    assert_eq!(file.unix_mode().unwrap() & 0o111 != 0, expected.executable);
                }
            }
        }
    }
}

#[test]
fn committed_tars_match_the_golden_tree_without_builder_tools() {
    for fixture in fixtures::load_all(&xtask::workspace_root().join(FIXTURE_ROOT))
        .unwrap()
        .into_iter()
        .filter(|f| matches!(f.expected.format.as_str(), "tar" | "tar.gz"))
    {
        #[cfg(unix)]
        let temp = TempDir::new().unwrap();
        let input = fs::File::open(fixture.input()).unwrap();
        let input: Box<dyn Read> = if fixture.expected.format == "tar.gz" {
            Box::new(flate2::read::GzDecoder::new(input))
        } else {
            Box::new(input)
        };
        let mut archive = tar::Archive::new(input);
        let mut actual = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry
                .path()
                .unwrap()
                .to_str()
                .unwrap()
                .trim_end_matches('/')
                .to_owned();
            let header = entry.header();
            let kind = if header.entry_type().is_dir() {
                EntryKind::Directory
            } else if header.entry_type().is_symlink() {
                EntryKind::Symlink
            } else {
                assert!(header.entry_type().is_file());
                EntryKind::File
            };
            let executable = kind == EntryKind::File && header.mode().unwrap() & 0o111 != 0;
            let symlink_target = entry
                .link_name()
                .unwrap()
                .map(|name| name.to_str().unwrap().to_owned());
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            use sha2::{Digest, Sha256};
            actual.push(fixtures::TreeEntry {
                path,
                kind: kind.clone(),
                size: bytes.len() as u64,
                sha256: (kind == EntryKind::File).then(|| format!("{:x}", Sha256::digest(bytes))),
                executable,
                symlink_target,
            });
        }
        actual.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(actual, fixture.tree);
        // Windows can inspect links without requiring developer mode or admin
        // privileges merely to test the builder. Native extraction is #9's job.
        #[cfg(unix)]
        {
            let input = fs::File::open(fixture.input()).unwrap();
            let input: Box<dyn Read> = if fixture.expected.format == "tar.gz" {
                Box::new(flate2::read::GzDecoder::new(input))
            } else {
                Box::new(input)
            };
            tar::Archive::new(input).unpack(temp.path()).unwrap();
            fixture.assert_tree(temp.path()).unwrap();
        }
    }
}

#[test]
fn builder_emits_the_declared_layout_without_reading_host_permissions() {
    let root = xtask::workspace_root();
    let corpus = generate::load_corpus(&root).unwrap();
    let entries = generate::layout_entries(
        &root,
        &root.join(FIXTURE_ROOT).join("bin"),
        "macos",
        &corpus.layouts["macos"],
        "",
    )
    .unwrap();
    let links: Vec<_> = entries
        .iter()
        .filter(|entry| entry.tree.kind == EntryKind::Symlink)
        .collect();
    assert_eq!(links.len(), 3);
    let first = generate::zip_bytes(&entries).unwrap();
    assert_eq!(first, generate::zip_bytes(&entries).unwrap());
    let tar = generate::tar_bytes(&entries).unwrap();
    assert_eq!(tar, generate::tar_bytes(&entries).unwrap());
}
