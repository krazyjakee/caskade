//! Offline web game bundles (WEB-GAMES-PLAN §2, §6): everything a game
//! loaded, kept by URL, in a folder that plays without a network.
//!
//! `caskade-web.json` maps each URL to a file under `files/`, named after
//! a hash of the URL so query strings, long paths and case-insensitive
//! filesystems don't matter. Bodies are kept decoded, so a bundle answers
//! without `Content-Encoding`. A POST's response is kept by its URL and a
//! hash of what was posted (`<url>#post=<hash>`), since games load levels
//! and data that way too.

use super::GameSize;
use crate::lock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};
use url::Url;

/// The bundle's manifest, in its folder. Root scans skip a folder that has
/// one.
pub const MANIFEST: &str = caskade_installer::reconcile::WEB_BUNDLE_MANIFEST;
const FILES: &str = "files";
const VERSION: u32 = 1;
/// Between a POST's URL and the hash of its body, in its key.
const POST: &str = "#post=";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    /// The page the game starts at.
    pub start: String,
    /// How big the game is, when its page said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<GameSize>,
    /// Every URL kept, without its fragment; POSTs by [`post_key`].
    pub items: BTreeMap<String, Item>,
    /// Each URL posted to: the key of the last response kept for it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub posts: BTreeMap<String, String>,
}

/// One URL's response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// Its file, relative to the bundle's folder.
    pub file: String,
    pub status: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    pub size: u64,
}

/// A response from the bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub status: u16,
    pub content_type: Option<String>,
    pub path: PathBuf,
}

/// A bundle's folder and manifest. Safe to share between threads; keep
/// writes the manifest with [`Bundle::save`].
#[derive(Debug)]
pub struct Bundle {
    dir: PathBuf,
    manifest: Mutex<Manifest>,
}

impl Bundle {
    /// Opens the bundle in `dir`, or starts one at `start` when it has none.
    /// An existing bundle keeps its start page.
    pub fn create(dir: impl Into<PathBuf>, start: &Url) -> io::Result<Self> {
        let dir = dir.into();
        if dir.join(MANIFEST).is_file() {
            return Self::open(dir);
        }
        std::fs::create_dir_all(dir.join(FILES))?;
        let bundle = Self {
            dir,
            manifest: Mutex::new(Manifest {
                version: VERSION,
                start: key(start),
                size: None,
                items: BTreeMap::new(),
                posts: BTreeMap::new(),
            }),
        };
        bundle.save()?;
        Ok(bundle)
    }

    /// Opens the bundle in `dir`.
    pub fn open(dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = dir.into();
        let manifest: Manifest = serde_json::from_slice(&std::fs::read(dir.join(MANIFEST))?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if manifest.version != VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "web game bundle version {} isn't supported",
                    manifest.version
                ),
            ));
        }
        Ok(Self {
            dir,
            manifest: Mutex::new(manifest),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The page the game starts at.
    pub fn start(&self) -> String {
        lock(&self.manifest).start.clone()
    }

    /// Starts the game at `start` instead: the game found in the page it
    /// was captured in. Only in memory until [`Bundle::save`].
    pub fn set_start(&self, start: &Url) {
        lock(&self.manifest).start = key(start);
    }

    /// Drops every URL `keep` refuses, with its file, and returns how many
    /// went. The manifest changes in memory until [`Bundle::save`].
    pub fn retain(&self, keep: impl Fn(&str) -> bool) -> io::Result<usize> {
        let dropped: Vec<(String, String)> = {
            let mut manifest = lock(&self.manifest);
            let gone: Vec<String> = manifest
                .items
                .keys()
                .filter(|url| !keep(url))
                .cloned()
                .collect();
            let dropped: Vec<_> = gone
                .into_iter()
                .filter_map(|url| {
                    let item = manifest.items.remove(&url)?;
                    Some((url, item.file))
                })
                .collect();
            let Manifest { items, posts, .. } = &mut *manifest;
            posts.retain(|_, last| items.contains_key(last));
            dropped
        };
        for (_, file) in &dropped {
            match std::fs::remove_file(self.dir.join(file)) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        Ok(dropped.len())
    }

    /// How big the game is, when its page said.
    pub fn size(&self) -> Option<GameSize> {
        lock(&self.manifest).size
    }

    /// Keeps the game's size; only in memory until [`Bundle::save`].
    pub fn set_size(&self, size: Option<GameSize>) {
        lock(&self.manifest).size = size;
    }

    /// The response kept for `url`. A URL the bundle doesn't have finds one
    /// that differs only in its query, since games often add a
    /// cache-busting `?v=` or `?t=`.
    pub fn get(&self, url: &Url) -> Option<Stored> {
        let manifest = lock(&self.manifest);
        let wanted = key(url);
        let item = manifest.items.get(&wanted).or_else(|| {
            let bare = without_query(&wanted);
            manifest
                .items
                .iter()
                .find(|(kept, _)| !kept.contains(POST) && without_query(kept) == bare)
                .map(|(_, item)| item)
        })?;
        Some(self.stored(item))
    }

    /// The response kept for posting `body` to `url`.
    pub fn get_post(&self, url: &Url, body: &[u8]) -> Option<Stored> {
        let manifest = lock(&self.manifest);
        Some(self.stored(manifest.items.get(&post_key(url, body))?))
    }

    /// The last response kept for a POST to `url`: offline, a game posting
    /// something the capture didn't gets what it had.
    pub fn last_post(&self, url: &Url) -> Option<Stored> {
        let manifest = lock(&self.manifest);
        Some(self.stored(manifest.items.get(manifest.posts.get(&key(url))?)?))
    }

    fn stored(&self, item: &Item) -> Stored {
        Stored {
            status: item.status,
            content_type: item.content_type.clone(),
            path: self.dir.join(&item.file),
        }
    }

    /// Whether the bundle has `url` itself, query included.
    pub fn has(&self, url: &Url) -> bool {
        lock(&self.manifest).items.contains_key(&key(url))
    }

    /// Keeps `body` as `url`'s response. Only the manifest in memory
    /// changes until [`Bundle::save`].
    pub fn put(
        &self,
        url: &Url,
        status: u16,
        content_type: Option<&str>,
        body: &[u8],
    ) -> io::Result<()> {
        self.keep(key(url), status, content_type, body)
    }

    /// Keeps `body` as the response to posting `posted` to `url`, and as
    /// the last for `url`. Only in memory until [`Bundle::save`].
    pub fn put_post(
        &self,
        url: &Url,
        posted: &[u8],
        status: u16,
        content_type: Option<&str>,
        body: &[u8],
    ) -> io::Result<()> {
        let post = post_key(url, posted);
        self.keep(post.clone(), status, content_type, body)?;
        lock(&self.manifest).posts.insert(key(url), post);
        Ok(())
    }

    fn keep(
        &self,
        url: String,
        status: u16,
        content_type: Option<&str>,
        body: &[u8],
    ) -> io::Result<()> {
        let file = format!("{FILES}/{}", file_name(&url));
        let path = self.dir.join(&file);
        let partial = path.with_extension("partial");
        std::fs::write(&partial, body)?;
        std::fs::rename(&partial, &path)?;
        lock(&self.manifest).items.insert(
            url,
            Item {
                file,
                status,
                content_type: content_type.map(str::to_owned),
                size: body.len() as u64,
            },
        );
        Ok(())
    }

    /// Writes the manifest, replacing the last one whole.
    pub fn save(&self) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(&*lock(&self.manifest))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let path = self.dir.join(MANIFEST);
        let partial = path.with_extension("json.partial");
        std::fs::write(&partial, json)?;
        std::fs::rename(&partial, &path)
    }

    /// How many URLs it keeps, and their bytes.
    pub fn stats(&self) -> (usize, u64) {
        let manifest = lock(&self.manifest);
        (
            manifest.items.len(),
            manifest.items.values().map(|item| item.size).sum(),
        )
    }
}

/// A URL as the manifest keys it: without its fragment.
fn key(url: &Url) -> String {
    let mut url = url.clone();
    url.set_fragment(None);
    url.into()
}

/// A POST's key: its URL's, then [`POST`] and a hash of its body.
pub fn post_key(url: &Url, body: &[u8]) -> String {
    format!("{}{POST}{}", key(url), hex(&Sha256::digest(body)))
}

fn without_query(url: &str) -> &str {
    url.split_once('?').map_or(url, |(bare, _)| bare)
}

fn file_name(url: &str) -> String {
    hex(&Sha256::digest(url.as_bytes()))
}

/// The first 16 bytes of a hash, in hex.
fn hex(hash: &[u8]) -> String {
    hash.iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn a_bundle_moves_to_the_game_it_found_and_drops_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let page = url("https://portal.example/g/robo");
        let game = url("https://g1.cdn.example/v2/index.html?country=GB");
        let bundle = Bundle::create(dir.path(), &page).unwrap();
        for kept in [&page, &game, &url("https://portal.example/app.js")] {
            bundle.put(kept, 200, None, b"x").unwrap();
        }
        let app = bundle
            .get(&url("https://portal.example/app.js"))
            .unwrap()
            .path;
        bundle.set_start(&game);
        bundle.set_size(GameSize::new(836, 470));
        assert_eq!(
            bundle
                .retain(|kept| !kept.starts_with("https://portal.example/app"))
                .unwrap(),
            1
        );
        assert!(!app.exists(), "its file is gone");
        bundle.save().unwrap();
        let reopened = Bundle::open(dir.path()).unwrap();
        assert_eq!(reopened.start(), game.as_str());
        assert_eq!(reopened.size(), GameSize::new(836, 470));
        assert_eq!(reopened.stats().0, 2);
    }

    #[test]
    fn posts_are_kept_by_what_was_posted_and_the_last_for_each_url() {
        let dir = tempfile::tempdir().unwrap();
        let level = url("https://games.example/get_level.hw");
        let bundle = Bundle::create(dir.path(), &url("https://games.example/")).unwrap();
        bundle.put_post(&level, b"id=1", 200, None, b"one").unwrap();
        bundle.put_post(&level, b"id=2", 200, None, b"two").unwrap();
        let get = url("https://games.example/get_level.hw?x");
        bundle.put(&get, 200, None, b"get").unwrap();
        let read = |stored: Option<Stored>| std::fs::read(stored.unwrap().path).unwrap();
        assert_eq!(read(bundle.get_post(&level, b"id=1")), b"one");
        assert!(bundle.get_post(&level, b"id=3").is_none());
        assert_eq!(read(bundle.last_post(&level)), b"two");
        assert_eq!(
            read(bundle.get(&level)),
            b"get",
            "a GET never gets a POST's"
        );
        assert!(bundle.last_post(&get).is_none());
        bundle.save().unwrap();
        let reopened = Bundle::open(dir.path()).unwrap();
        assert_eq!(read(reopened.get_post(&level, b"id=1")), b"one");
        // Dropping the last forgets it.
        let last = post_key(&level, b"id=2");
        reopened.retain(|kept| kept != last).unwrap();
        assert_eq!(read(reopened.get_post(&level, b"id=1")), b"one");
        assert!(reopened.last_post(&level).is_none());
    }

    #[test]
    fn a_bundle_keeps_responses_by_url_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let start = url("https://games.example/needle/index.html#play");
        let bundle = Bundle::create(dir.path(), &start).unwrap();
        assert_eq!(bundle.start(), "https://games.example/needle/index.html");
        bundle
            .put(&start, 200, Some("text/html"), b"<canvas></canvas>")
            .unwrap();
        bundle
            .put(&url("https://cdn.example/level.json?v=3"), 200, None, b"{}")
            .unwrap();
        assert_eq!(bundle.stats(), (2, 19));
        bundle.save().unwrap();

        let reopened = Bundle::create(dir.path(), &url("https://other.example/")).unwrap();
        assert_eq!(reopened.start(), "https://games.example/needle/index.html");
        let page = reopened.get(&start).unwrap();
        assert_eq!(page.content_type.as_deref(), Some("text/html"));
        assert_eq!(std::fs::read(page.path).unwrap(), b"<canvas></canvas>");
        // Another cache-busting query finds the same file.
        let level = reopened
            .get(&url("https://cdn.example/level.json?v=4"))
            .unwrap();
        assert_eq!(std::fs::read(level.path).unwrap(), b"{}");
        assert!(!reopened.has(&url("https://cdn.example/level.json?v=4")));
        assert!(
            reopened
                .get(&url("https://cdn.example/other.json"))
                .is_none()
        );
    }
}
