use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

use image::{DynamicImage, ImageFormat, ImageReader};

use super::{DiscoveryConfig, tree::Tree, warning};
use crate::{LaunchTarget, Warning};

pub(super) fn normalize(
    tree: &Tree,
    supplied: &[PathBuf],
    embedded: &[(PathBuf, Vec<u8>)],
    targets: &[LaunchTarget],
    config: DiscoveryConfig,
    warnings: &mut Vec<Warning>,
) -> Option<PathBuf> {
    for (label, bytes) in embedded {
        match decode_image(bytes) {
            Ok(image) => match write_icon(tree, image) {
                Ok(path) => return Some(path),
                Err(error) => warnings.push(warning(
                    "icon_write_failed",
                    &error.to_string(),
                    Some(label.clone()),
                )),
            },
            Err(error) => warnings.push(warning(
                "icon_decode_failed",
                &error.to_string(),
                Some(label.clone()),
            )),
        }
    }
    let mut paths = supplied.to_vec();
    if tree.resolve(Path::new(".DirIcon")).is_some() {
        paths.push(".DirIcon".into());
    }
    for entry in &tree.entries {
        if !entry.directory
            && entry.path.file_name().is_some_and(|name| {
                name.eq_ignore_ascii_case("icon.png") || name.eq_ignore_ascii_case("game.ico")
            })
        {
            paths.push(entry.path.clone());
        }
    }
    for path in paths {
        match decode_path(tree, &path, config.max_resource_bytes) {
            Ok(image) => match write_icon(tree, image) {
                Ok(path) => return Some(path),
                Err(error) => {
                    warnings.push(warning("icon_write_failed", &error.to_string(), Some(path)))
                }
            },
            Err(error) => warnings.push(warning(
                "icon_decode_failed",
                &error.to_string(),
                Some(path),
            )),
        }
    }
    for target in targets {
        if !target
            .path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            continue;
        }
        let Some(image) = pe_icon(tree, &target.path, config.max_resource_bytes) else {
            continue;
        };
        match write_icon(tree, image) {
            Ok(path) => return Some(path),
            Err(error) => warnings.push(warning(
                "icon_write_failed",
                &error.to_string(),
                Some(target.path.clone()),
            )),
        }
    }
    None
}

fn decode_path(tree: &Tree, path: &Path, limit: u64) -> std::io::Result<DynamicImage> {
    decode_bytes(tree.read(path, limit)?)
}

/// An ICNS family's largest supported icon, or any other image format.
fn decode_bytes(bytes: Vec<u8>) -> std::io::Result<DynamicImage> {
    if bytes.starts_with(b"icns") {
        let family = icns::IconFamily::read(Cursor::new(bytes))?;
        let mut available = family.available_icons();
        available.sort_by_key(|kind| std::cmp::Reverse(kind.pixel_width()));
        for kind in available {
            if let Ok(icon) = family.get_icon_with_type(kind)
                && let icon = icon.convert_to(icns::PixelFormat::RGBA)
                && let Some(image) =
                    image::RgbaImage::from_raw(icon.width(), icon.height(), icon.data().to_vec())
            {
                return Ok(DynamicImage::ImageRgba8(image));
            }
        }
        return Err(std::io::Error::other(
            "ICNS contains no supported icon (PNG and legacy bitmap entries supported)",
        ));
    }
    decode_image(&bytes)
}

fn decode_image(bytes: &[u8]) -> std::io::Result<DynamicImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(std::io::Error::other)
}

fn pe_icon(tree: &Tree, path: &Path, limit: u64) -> Option<DynamicImage> {
    pe_icon_bytes(&tree.read(path, limit).ok()?, limit)
}

/// The first icon group of a PE file's resources that decodes.
fn pe_icon_bytes(bytes: &[u8], limit: u64) -> Option<DynamicImage> {
    let pe = crate::probe::parse_pe(bytes)?;
    let resources = pe.resources().ok()?;
    for (_, group) in resources.icons().filter_map(Result::ok) {
        if group.entries().len() > 64 {
            continue;
        }
        let mut total = 0_u64;
        let mut valid = true;
        for entry in group.entries() {
            match group.image(entry.nId) {
                Ok(image) => {
                    total = total.saturating_add(image.len() as u64);
                    if total > limit {
                        valid = false;
                        break;
                    }
                }
                Err(_) => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            continue;
        }
        let mut ico = vec![];
        if group.write(&mut ico).is_ok()
            && let Ok(image) = decode_image(&ico)
        {
            return Some(image);
        }
    }
    None
}

fn write_icon(tree: &Tree, image: DynamicImage) -> std::io::Result<PathBuf> {
    let image = if image.width() > 512 || image.height() > 512 {
        image.thumbnail(512, 512)
    } else {
        image
    };
    let dir = cap_std::fs::Dir::open_ambient_dir(&tree.root, cap_std::ambient_authority())?;
    dir.create_dir_all(".caskade")?;
    let mut options = cap_std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let temporary = ".caskade/icon.png.part";
    let mut file = dir.open_with(temporary, &options)?;
    let result = (|| {
        image
            .write_to(&mut file, ImageFormat::Png)
            .map_err(std::io::Error::other)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        dir.rename(temporary, &dir, ".caskade/icon.png")
    })();
    if result.is_err() {
        let _ = dir.remove_file(temporary);
    }
    result.map(|_| PathBuf::from(".caskade/icon.png"))
}

/// The icon of a game's executable as a PNG of at most 512 px: a Windows
/// `.exe`'s icon resource, a macOS `.app` bundle's `CFBundleIconFile`, or an
/// image file (`.ico`, `.png`, `.icns`) itself. `Ok(None)` when it has none,
/// as native Linux binaries never do. Reads at most `max_bytes` of a file.
pub fn executable_icon(path: &Path, max_bytes: u64) -> std::io::Result<Option<Vec<u8>>> {
    let image = if path.is_dir() {
        bundle_icon(path, max_bytes)?
    } else {
        let bytes = read_bounded(path, max_bytes)?;
        if bytes.starts_with(b"MZ") {
            pe_icon_bytes(&bytes, max_bytes)
        } else {
            decode_bytes(bytes).ok()
        }
    };
    image
        .map(|image| {
            let image = if image.width() > 512 || image.height() > 512 {
                image.thumbnail(512, 512)
            } else {
                image
            };
            let mut png = Cursor::new(Vec::new());
            image
                .write_to(&mut png, ImageFormat::Png)
                .map_err(std::io::Error::other)?;
            Ok(png.into_inner())
        })
        .transpose()
}

fn read_bounded(path: &Path, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(std::io::Error::other(
            "file is larger than the icon read limit",
        ));
    }
    Ok(bytes)
}

/// The icon an `.app` bundle's `Info.plist` names.
#[cfg(feature = "macos-formats")]
fn bundle_icon(bundle: &Path, max_bytes: u64) -> std::io::Result<Option<DynamicImage>> {
    use crate::formats::macos::app::{MAX_PLIST_BYTES, parse_info};
    let info = read_bounded(&bundle.join("Contents/Info.plist"), MAX_PLIST_BYTES as u64)?;
    let Some(icon) = parse_info(&info).map_err(std::io::Error::other)?.icon else {
        return Ok(None);
    };
    Ok(decode_bytes(read_bounded(&bundle.join(icon), max_bytes)?).ok())
}

#[cfg(not(feature = "macos-formats"))]
fn bundle_icon(_bundle: &Path, _max_bytes: u64) -> std::io::Result<Option<DynamicImage>> {
    Ok(None)
}
