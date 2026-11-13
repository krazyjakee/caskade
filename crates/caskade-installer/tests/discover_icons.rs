use std::{fs, io::Cursor, path::Path};

use caskade_installer::{
    discover::{DiscoveryHints, discover, executable_icon},
    host::HostCaps,
    testing::pe_with_icon,
};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

fn host() -> HostCaps {
    caskade_installer::pipeline::input::host(Some("linux")).unwrap()
}

fn put(root: &Path, relative: &str, data: impl AsRef<[u8]>) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

fn png() -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(16, 16, Rgba([20, 80, 160, 255])));
    let mut output = Cursor::new(vec![]);
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    output.into_inner()
}

fn assert_icon(root: &Path) {
    let image = image::open(root.join(".caskade/icon.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(image.dimensions(), (16, 16));
    assert_eq!(image.get_pixel(0, 0).0, [20, 80, 160, 255]);
    assert!(!root.join(".caskade/icon.png.part").exists());
}

#[test]
fn desktop_icon_and_appimage_diricon_are_normalized_to_png() {
    for desktop in [false, true] {
        let root = tempfile::tempdir().unwrap();
        if desktop {
            put(
                root.path(),
                "Game.desktop",
                b"[Desktop Entry]\nType=Application\nName=Game\nExec=./start.sh\nIcon=game-art\n",
            );
            put(root.path(), "icons/game-art.png", png());
            put(root.path(), "start.sh", b"#!/bin/sh\n");
        } else {
            put(root.path(), ".DirIcon", png());
        }
        let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
        assert_eq!(result.icon.as_deref(), Some(Path::new(".caskade/icon.png")));
        assert_icon(root.path());
        // A repeated scan atomically replaces the cached icon.
        assert!(
            discover(root.path(), &host(), &DiscoveryHints::default())
                .unwrap()
                .icon
                .is_some()
        );
        assert_icon(root.path());
    }
}

#[test]
fn icns_bundle_icon_is_decoded_and_normalized() {
    let root = tempfile::tempdir().unwrap();
    let mut family = icns::IconFamily::new();
    let pixels = [20_u8, 80, 160, 255].repeat(16 * 16);
    let icon = icns::Image::from_data(icns::PixelFormat::RGBA, 16, 16, pixels).unwrap();
    family.add_icon(&icon).unwrap();
    let mut bytes = vec![];
    family.write(&mut bytes).unwrap();
    put(root.path(), "Game.app/Contents/Info.plist", br#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleName</key><string>Icon Game</string><key>CFBundleExecutable</key><string>Game</string><key>CFBundleIconFile</key><string>GameIcon</string></dict></plist>"#);
    put(root.path(), "Game.app/Contents/MacOS/Game", b"#!/bin/sh\n");
    put(
        root.path(),
        "Game.app/Contents/Resources/GameIcon.icns",
        bytes,
    );
    let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert!(result.icon.is_some());
    assert_icon(root.path());
}

#[test]
fn corrupt_icons_warn_without_losing_launch_metadata() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), ".DirIcon", b"not an image");
    put(root.path(), "start.sh", b"#!/bin/sh\n");
    let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert!(result.icon.is_none());
    assert_eq!(result.launch_targets[0].path, Path::new("start.sh"));
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.code == "icon_decode_failed")
    );
}

#[test]
fn pe_icon_resource_is_reassembled_and_normalized() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "Game.exe", pe_with_icon(&png()));
    let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert_eq!(result.launch_targets[0].path, Path::new("Game.exe"));
    assert!(
        result.icon.is_some(),
        "PE group icon must be decoded: {:?}",
        result.warnings
    );
    assert_icon(root.path());
}

#[cfg(unix)]
#[test]
fn icon_writes_cannot_escape_through_a_metadata_directory_link() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    put(root.path(), ".DirIcon", png());
    put(outside.path(), "sentinel", b"unchanged");
    std::os::unix::fs::symlink(outside.path(), root.path().join(".caskade")).unwrap();
    let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert!(result.icon.is_none());
    assert_eq!(
        fs::read(outside.path().join("sentinel")).unwrap(),
        b"unchanged"
    );
    assert!(!outside.path().join("icon.png").exists());
    assert!(!outside.path().join("icon.png.part").exists());
}

#[test]
fn executable_icon_reads_exes_bundles_and_images_without_writing() {
    let root = tempfile::tempdir().unwrap();
    let decoded = |bytes: Vec<u8>| {
        let image = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        assert_eq!(image.dimensions(), (16, 16));
        assert_eq!(image.get_pixel(0, 0).0, [20, 80, 160, 255]);
    };
    put(root.path(), "Game.exe", pe_with_icon(&png()));
    decoded(
        executable_icon(&root.path().join("Game.exe"), 1 << 20)
            .unwrap()
            .unwrap(),
    );

    let mut family = icns::IconFamily::new();
    let pixels = [20_u8, 80, 160, 255].repeat(16 * 16);
    family
        .add_icon(&icns::Image::from_data(icns::PixelFormat::RGBA, 16, 16, pixels).unwrap())
        .unwrap();
    let mut icns = vec![];
    family.write(&mut icns).unwrap();
    put(root.path(), "Game.app/Contents/Info.plist", br#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>Game</string><key>CFBundleIconFile</key><string>GameIcon</string></dict></plist>"#);
    put(
        root.path(),
        "Game.app/Contents/Resources/GameIcon.icns",
        icns,
    );
    decoded(
        executable_icon(&root.path().join("Game.app"), 1 << 20)
            .unwrap()
            .unwrap(),
    );

    put(root.path(), "icon.png", png());
    decoded(
        executable_icon(&root.path().join("icon.png"), 1 << 20)
            .unwrap()
            .unwrap(),
    );

    // A native Linux binary has no icon; a file over the limit isn't read.
    put(root.path(), "game.x86_64", b"\x7fELF\x02\x01\x01");
    assert!(
        executable_icon(&root.path().join("game.x86_64"), 1 << 20)
            .unwrap()
            .is_none()
    );
    assert!(executable_icon(&root.path().join("Game.exe"), 64).is_err());
    assert!(!root.path().join(".caskade").exists());
}
