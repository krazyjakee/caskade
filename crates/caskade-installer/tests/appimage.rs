#![cfg(feature = "linux-formats")]

use caskade_installer::{
    discover::{DiscoveryHints, discover},
    formats::linux::appimage::{AppImageLimits, elf_end, is_appimage, read_metadata},
    host::HostCaps,
};
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};

use crate::fixtures;

fn fixture() -> fixtures::Fixture {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut corpus = fixtures::load_appimage(&root).unwrap();
    assert_eq!(corpus.len(), 1);
    corpus.remove(0)
}

fn host() -> HostCaps {
    caskade_installer::pipeline::input::host(Some("linux")).unwrap()
}

fn assert_fixture_icon(path: &Path) {
    let icon = image::open(path).unwrap().to_rgba8();
    assert_eq!(icon.dimensions(), (16, 16));
    assert_eq!(icon.get_pixel(0, 0).0, [20, 80, 160, 255]);
}

#[test]
fn genuine_appimage_metadata_is_read_from_its_squashfs() {
    let fixture = fixture();
    let bytes = fs::read(fixture.input()).unwrap();
    assert!(is_appimage(&bytes));
    let metadata = read_metadata(
        fs::File::open(fixture.input()).unwrap(),
        AppImageLimits::default(),
    )
    .unwrap();
    assert_eq!(metadata.squashfs_offset, elf_end(&bytes).unwrap());
    assert_eq!(
        metadata.desktop_file.as_deref(),
        Some(Path::new("io.caskade.IndieGame.desktop"))
    );
    assert_eq!(metadata.name.as_deref(), Some("Indie Game"));
    assert_eq!(metadata.exec.as_deref(), Some("indie-game"));
    assert_eq!(metadata.categories.as_deref(), Some("Game;"));
    assert_eq!(metadata.icon_name.as_deref(), Some("indie-game"));
    assert_eq!(
        metadata.appstream_id.as_deref(),
        Some("io.caskade.IndieGame")
    );
    assert_eq!(metadata.appstream_name.as_deref(), Some("Indie Game"));
    // .DirIcon is a symlink to indie-game.png; both resolve inside the image.
    let paths: Vec<_> = metadata.icons.iter().map(|icon| &icon.path).collect();
    assert_eq!(paths, [Path::new(".DirIcon"), Path::new("indie-game.png")]);
    assert_eq!(metadata.icons[0].bytes, metadata.icons[1].bytes);
    let icon = image::load_from_memory(&metadata.icons[0].bytes)
        .unwrap()
        .to_rgba8();
    assert_eq!(icon.dimensions(), (16, 16));
}

#[test]
fn installed_appimage_is_discovered_with_title_icon_and_launch_target() {
    let fixture = fixture();
    let game = tempfile::tempdir().unwrap();
    fs::copy(
        fixture.input(),
        game.path().join(&fixture.expected.launch_target),
    )
    .unwrap();
    let discovery = discover(game.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert_eq!(discovery.title, fixture.expected.title);
    assert_eq!(discovery.ids["appstream"], "io.caskade.IndieGame");
    assert_eq!(
        discovery.launch_targets[0].path,
        Path::new(&fixture.expected.launch_target)
    );
    assert!(discovery.launch_targets[0].args.is_empty());
    assert_eq!(discovery.launch_targets[0].reason, "AppImage desktop entry");
    assert!(discovery.question.is_none());
    assert_fixture_icon(&game.path().join(discovery.icon.unwrap()));
}

#[test]
fn damaged_images_fail_without_panicking() {
    let bytes = fs::read(fixture().input()).unwrap();
    let offset = elf_end(&bytes).unwrap() as usize;
    let limits = AppImageLimits::default();
    for length in [0, 10, 64, offset, offset + 4, offset + 96, bytes.len() / 2] {
        let _ = read_metadata(Cursor::new(&bytes[..length]), limits);
    }
    let mut state = 0x2545_f491_u32;
    for _ in 0..64 {
        let mut damaged = bytes.clone();
        for _ in 0..8 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let at = offset + state as usize % (damaged.len() - offset);
            damaged[at] ^= (state >> 24) as u8 | 1;
        }
        let _ = read_metadata(Cursor::new(damaged), limits);
    }
    let tight = AppImageLimits {
        max_entries: 3,
        ..limits
    };
    assert!(read_metadata(Cursor::new(&bytes), tight).is_err());
}

/// Every squashfs compressor AppImage tooling emits, decoded by the bundled
/// pure-Rust decoders. Images are made by the host's mksquashfs when present.
#[test]
fn squashfs_compressors_decode_with_pure_rust_backends() {
    let Ok(help) = Command::new("mksquashfs").arg("-help").output() else {
        eprintln!("skipping: mksquashfs is not installed");
        return;
    };
    let help = String::from_utf8_lossy(&help.stdout) + String::from_utf8_lossy(&help.stderr);
    let bytes = fs::read(fixture().input()).unwrap();
    let runtime = &bytes[..elf_end(&bytes).unwrap() as usize];
    let work = tempfile::tempdir().unwrap();
    let appdir = work.path().join("AppDir");
    fs::create_dir_all(appdir.join("usr/share/metainfo")).unwrap();
    fs::write(
        appdir.join("game.desktop"),
        "[Desktop Entry]\nType=Application\nName=Compressed\nExec=game --flag %U\nIcon=game\n",
    )
    .unwrap();
    // Incompressible-enough bytes spanning several 4 KiB blocks plus a fragment.
    let payload: Vec<u8> = (0..20_000u32).map(|i| (i * 7 + i / 13) as u8).collect();
    fs::write(appdir.join("game.png"), &payload).unwrap();
    let mut tested = vec![];
    for compressor in ["gzip", "xz", "lz4", "zstd"] {
        if !help.contains(compressor) {
            continue;
        }
        let squashfs: PathBuf = work.path().join(format!("{compressor}.squashfs"));
        let status = Command::new("mksquashfs")
            .arg(&appdir)
            .arg(&squashfs)
            .args(["-comp", compressor, "-b", "4096", "-noappend", "-quiet"])
            .args(["-root-owned", "-no-progress"])
            .status()
            .unwrap();
        assert!(status.success(), "{compressor}");
        let mut image = runtime.to_vec();
        image.extend(fs::read(&squashfs).unwrap());
        let metadata = read_metadata(Cursor::new(image), AppImageLimits::default())
            .unwrap_or_else(|error| panic!("{compressor}: {error}"));
        assert_eq!(metadata.name.as_deref(), Some("Compressed"), "{compressor}");
        assert_eq!(metadata.icons[0].path, Path::new("game.png"));
        assert_eq!(metadata.icons[0].bytes, payload, "{compressor}");
        tested.push(compressor);
    }
    eprintln!("decoded squashfs compressors: {tested:?}");
}

#[cfg(all(feature = "cli", target_os = "linux"))]
#[test]
fn appimage_indie_installs_through_the_binary_with_title_icon_and_launch_target() {
    use caskade_installer::testing::TestRoot;
    use std::os::unix::fs::PermissionsExt;

    let fixture = fixture();
    let root = TestRoot::new().unwrap();
    let run = |args: &[&std::ffi::OsStr]| {
        let output = Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .arg("--json")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let input = fixture.input();
    let probed = run(&["probe".as_ref(), input.as_os_str()]);
    assert_eq!(probed["artifacts"][0]["format"], "app_image");
    let installed = run(&[
        "install".as_ref(),
        input.as_os_str(),
        "--root".as_ref(),
        root.path().as_os_str(),
    ]);
    assert_eq!(installed["type"], "installed");
    let value = &installed["value"];
    assert_eq!(value["title"], fixture.expected.title);
    assert_eq!(value["ids"]["appstream"], "io.caskade.IndieGame");
    assert_eq!(
        value["launch_targets"][0]["path"],
        fixture.expected.launch_target
    );
    assert_eq!(value["runtime"]["type"], "native");
    let game = root.path().join(value["dir"].as_str().unwrap());
    let launch = game.join(&fixture.expected.launch_target);
    assert_eq!(fs::read(&launch).unwrap(), fs::read(&input).unwrap());
    assert_ne!(
        fs::metadata(&launch).unwrap().permissions().mode() & 0o111,
        0
    );
    assert_fixture_icon(&game.join(".caskade/icon.png"));
    let mut tree = fixtures::snapshot_tree(&game).unwrap();
    tree.retain(|entry| entry.path != ".caskade" && !entry.path.starts_with(".caskade/"));
    assert_eq!(tree, fixture.tree);
    let removed = run(&["uninstall".as_ref(), game.as_os_str()]);
    assert_eq!(removed["value"]["game_removed"], true);
    assert!(input.is_file(), "local source retained");
}

/// #21 robustness report: squashfs table offsets near u64::MAX overflowed
/// backhand's offset reader (`offset + start`). The image is the minimised
/// squashfs behind the robustness target's 64-byte runtime header.
#[test]
fn regression_21_huge_squashfs_table_offset_is_an_error() {
    let mut image = vec![0u8; 64];
    image[..4].copy_from_slice(b"\x7fELF");
    image[4] = 2;
    image[5] = 1;
    image[8..11].copy_from_slice(b"AI\x02");
    image[0x28..0x30].copy_from_slice(&64u64.to_le_bytes());
    image.extend_from_slice(include_bytes!(
        "regressions/36-appimage-offset-overflow.squashfs"
    ));
    assert!(read_metadata(Cursor::new(image), AppImageLimits::default()).is_err());
}
