//! Local artwork in `appcache/librarycache`: the old flat layout
//! (`<id>_icon.jpg`, `<id>_library_600x900.jpg`, …) and the per-app folder
//! layout (`<id>/library_600x900.jpg`, `<id>/<sha1>.jpg` for the icon).

use super::super::{is_file, join, list};
use crate::{
    env::HostEnv,
    types::{Artwork, ArtworkKind},
};
use std::path::{Path, PathBuf};

pub(super) fn artwork(env: &HostEnv, root: &Path, app_id: u32) -> (Option<PathBuf>, Vec<Artwork>) {
    let cache = join(root, "appcache/librarycache");
    let folder = join(&cache, &app_id.to_string());
    let folder_files = list(env, &folder).unwrap_or_default();
    let find = |flat: &str, nested: &[&str]| {
        let flat = join(&cache, &format!("{app_id}_{flat}"));
        if is_file(env, &flat) {
            return Some(flat);
        }
        nested
            .iter()
            .find(|name| folder_files.iter().any(|file| file == *name))
            .map(|name| join(&folder, name))
    };
    let icon = find("icon.jpg", &[]).or_else(|| {
        folder_files
            .iter()
            .find(|name| {
                name.strip_suffix(".jpg").is_some_and(|hash| {
                    hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            })
            .map(|name| join(&folder, name))
    });
    let art = [
        (
            ArtworkKind::Cover,
            find("library_600x900.jpg", &["library_600x900.jpg"]),
        ),
        (ArtworkKind::Header, find("header.jpg", &["header.jpg"])),
        (
            ArtworkKind::Hero,
            find("library_hero.jpg", &["library_hero.jpg"]),
        ),
        (ArtworkKind::Logo, find("logo.png", &["logo.png"])),
    ]
    .into_iter()
    .filter_map(|(kind, path)| Some(Artwork { kind, path: path? }))
    .collect();
    (icon, art)
}
