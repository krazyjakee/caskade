use std::{fs, io::Cursor};
use xtask::{checks::FIXTURE_ROOT, fixtures, workspace_root};

#[test]
fn genuine_shell_payloads_match_goldens_and_launch_without_installer_execution() {
    let root = workspace_root().join(FIXTURE_ROOT);
    let corpus = fixtures::load_linux_shell(&root).unwrap();
    assert_eq!(corpus.len(), 2);
    for fixture in corpus {
        let temporary = tempfile::TempDir::new().unwrap();
        let extracted = temporary.path().join("extracted");
        let bytes = fs::read(fixture.input()).unwrap();
        assert!(bytes.starts_with(b"#!/bin/sh\n"));
        if fixture.expected.format == "makeself" {
            let header = String::from_utf8_lossy(&bytes[..bytes.len().min(20_000)]);
            let skip: usize = header
                .lines()
                .find_map(|line| {
                    line.strip_prefix("skip=\"")?
                        .strip_suffix('"')?
                        .parse()
                        .ok()
                })
                .unwrap();
            let mut newlines = 0;
            let offset = bytes
                .iter()
                .position(|byte| {
                    if *byte == b'\n' {
                        newlines += 1;
                    }
                    newlines == skip
                })
                .unwrap()
                + 1;
            let decoder = flate2::read::GzDecoder::new(Cursor::new(&bytes[offset..]));
            tar::Archive::new(decoder).unpack(&extracted).unwrap();
        } else {
            assert!(String::from_utf8_lossy(&bytes[..20_000]).contains("MojoSetup"));
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
            archive.extract(temporary.path()).unwrap();
            fs::rename(temporary.path().join("data/noarch"), &extracted).unwrap();
        }
        fixture.assert_tree(&extracted).unwrap();
        #[cfg(target_os = "linux")]
        {
            let token = temporary.path().join("launch-token");
            let output = std::process::Command::new(extracted.join("start.sh"))
                .arg("--token")
                .arg(&token)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                fs::read_to_string(token)
                    .unwrap()
                    .trim()
                    .ends_with("/FixtureGame.x86_64")
            );
        }
    }
}
