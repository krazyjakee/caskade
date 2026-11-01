use std::fs;
use xtask::{checks::FIXTURE_ROOT, fixtures, workspace_root};

/// Static checks only: the runtime is never executed here.
#[test]
fn genuine_appimage_has_type2_runtime_squashfs_and_matching_golden() {
    let root = workspace_root().join(FIXTURE_ROOT);
    let corpus = fixtures::load_appimage(&root).unwrap();
    assert_eq!(corpus.len(), 1);
    let fixture = &corpus[0];
    assert_eq!(fixture.expected.format, "appimage");
    let bytes = fs::read(fixture.input()).unwrap();
    assert!(bytes.starts_with(b"\x7fELF\x02\x01"));
    assert_eq!(&bytes[8..11], b"AI\x02");
    let word = |at: usize, width: usize| {
        bytes[at..at + width]
            .iter()
            .rev()
            .fold(0u64, |value, byte| value << 8 | u64::from(*byte))
    };
    let squashfs = (word(0x28, 8) + word(0x3a, 2) * word(0x3c, 2)) as usize;
    assert_eq!(&bytes[squashfs..squashfs + 4], b"hsqs");
    assert_eq!(word(squashfs + 20, 2), 6, "appimagetool default zstd");
    let installed = tempfile::TempDir::new().unwrap();
    fs::copy(
        fixture.input(),
        installed.path().join(&fixture.expected.launch_target),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = installed.path().join(&fixture.expected.launch_target);
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fixture.assert_tree(installed.path()).unwrap();
}
