//! A torrent's file list: names checked against the staging rules and the
//! limits, and grouped into the payloads a user can choose between.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use librqbit::ValidatedTorrentMetaV1Info;
use unicode_normalization::UnicodeNormalization;
use url::Url;

use super::TorrentConfig;
use crate::{
    DownloadAsset, Question,
    fetch::{FetchError, Result, safe_file_name},
    safety::SafetyError,
};

/// One file of the torrent, its name checked.
pub(super) struct Listed {
    /// The torrent's own file index (padding files count).
    pub(super) index: usize,
    parts: Vec<String>,
    pub(super) path: PathBuf,
    pub(super) len: u64,
}

/// A torrent's files, checked against the staging rules and the limits.
pub(super) struct Listing {
    /// A safe folder name for a multi-file torrent.
    pub(super) name: String,
    pub(super) multi: bool,
    pub(super) files: Vec<Listed>,
    total: u64,
}

/// Files that install together: a top-level folder, or a top-level file
/// with the other volumes of its set.
pub(super) struct Group {
    label: String,
    pub(super) files: Vec<usize>,
    /// The folder, or the set's first file, relative to the torrent's folder.
    pub(super) payload: PathBuf,
    bytes: u64,
}

impl Listing {
    pub(super) fn inspect<B: AsRef<[u8]>>(
        info: &ValidatedTorrentMetaV1Info<B>,
        hash: &str,
        config: &TorrentConfig,
    ) -> Result<Self> {
        let key = |parts: &[String]| parts.join("/").nfc().collect::<String>().to_lowercase();
        let mut files = Vec::new();
        let mut names = BTreeSet::new();
        let mut folders = BTreeSet::new();
        let mut total = 0_u64;
        for (index, file) in info.iter_file_details().enumerate() {
            let attrs = file.attrs();
            if attrs.padding {
                continue;
            }
            if attrs.symlink {
                return Err(FetchError::Torrent {
                    message: "the torrent contains symbolic links".into(),
                });
            }
            let parts: Vec<String> = file
                .filename
                .iter_components()
                .map(|part| part.into_owned())
                .collect();
            if parts.len() > config.max_depth {
                return Err(SafetyError::DepthLimit.into());
            }
            for part in &parts {
                if part.contains('\u{FFFD}') {
                    return Err(SafetyError::InvalidEncoding.into());
                }
                safe_file_name(part)?;
            }
            let path: PathBuf = parts.iter().collect();
            // Names that differ only in case, or that are both a file and a
            // folder, collide on Windows and macOS.
            for depth in 1..parts.len() {
                let folder = key(&parts[..depth]);
                if names.contains(&folder) {
                    return Err(SafetyError::Collision(path).into());
                }
                folders.insert(folder);
            }
            let name = key(&parts[..]);
            if folders.contains(&name) || !names.insert(name) {
                return Err(SafetyError::Collision(path).into());
            }
            total = total
                .checked_add(file.len)
                .filter(|total| *total <= config.max_bytes)
                .ok_or(FetchError::TooLarge {
                    limit: config.max_bytes,
                })?;
            files.push(Listed {
                index,
                parts,
                path,
                len: file.len,
            });
            if files.len() > config.max_files {
                return Err(SafetyError::EntryLimit.into());
            }
        }
        if files.is_empty() {
            return Err(FetchError::Torrent {
                message: "the torrent has no files".into(),
            });
        }
        let name = info
            .name()
            .map(|name| name.into_owned())
            .filter(|name| !name.contains('\u{FFFD}') && safe_file_name(name).is_ok())
            .unwrap_or_else(|| format!("torrent-{}", &hash[..8]));
        Ok(Self {
            name,
            multi: info.info().files.is_some(),
            files,
            total,
        })
    }

    /// The groups that hold something installable. Two or more make the
    /// payload ambiguous.
    pub(super) fn groups(&self) -> Vec<Group> {
        if !self.multi {
            return Vec::new();
        }
        let mut groups: BTreeMap<String, (Group, bool)> = BTreeMap::new();
        for file in &self.files {
            let first = &file.parts[0];
            let nested = file.parts.len() > 1;
            let key = if nested {
                format!("/{}", first.to_lowercase())
            } else {
                crate::input::set_key(first)
            };
            let (group, payload) = groups.entry(key).or_insert_with(|| {
                (
                    Group {
                        label: String::new(),
                        files: Vec::new(),
                        payload: PathBuf::from(first),
                        bytes: 0,
                    },
                    false,
                )
            });
            group.files.push(file.index);
            group.bytes += file.len;
            *payload |= crate::input::payload_name(&file.parts[file.parts.len() - 1]);
            // A set installs from its leading file: setup.exe, game.zip,
            // game.rar or the first volume.
            if !nested && primary(first) < primary(&group.payload.to_string_lossy()) {
                group.payload = PathBuf::from(first);
            }
        }
        groups
            .into_iter()
            .filter(|(_, (_, payload))| *payload)
            .map(|(key, (mut group, _))| {
                let name = group.payload.to_string_lossy().into_owned();
                group.label = if key.starts_with('/') {
                    format!("{name}/")
                } else if group.files.len() > 1 {
                    format!("{name} (+{} more)", group.files.len() - 1)
                } else {
                    name
                };
                group
            })
            .collect()
    }

    /// Which payload to download: everything, or one group.
    pub(super) fn question(&self, hash: &str, groups: &[Group]) -> Question {
        let magnet = |files: Option<&[usize]>| {
            let mut url = Url::parse("magnet:").expect("a magnet URL");
            url.query_pairs_mut()
                .append_pair("xt", &format!("urn:btih:{hash}"))
                .append_pair("dn", &self.name);
            if let Some(files) = files {
                url.query_pairs_mut().append_pair("so", &ranges(files));
            }
            url
        };
        let mut assets = vec![DownloadAsset {
            name: format!("Everything in “{}” ({} files)", self.name, self.files.len()),
            url: magnet(None),
            size: Some(self.total),
            sha256: None,
        }];
        assets.extend(groups.iter().map(|group| DownloadAsset {
            name: group.label.clone(),
            url: magnet(Some(group.files.as_slice())),
            size: Some(group.bytes),
            sha256: None,
        }));
        Question::ChooseAsset { assets }
    }
}

/// Orders a set's files: unnumbered leaders first, then by name.
fn primary(name: &str) -> (bool, String) {
    let lower = name.to_lowercase();
    (crate::input::numbered_name(&lower), lower)
}

/// BEP 53 file ranges: `0-3,5,7-9`.
fn ranges(files: &[usize]) -> String {
    let mut sorted = files.to_vec();
    sorted.sort_unstable();
    let mut parts = Vec::new();
    let mut iter = sorted.into_iter().peekable();
    while let Some(start) = iter.next() {
        let mut end = start;
        while iter.peek() == Some(&(end + 1)) {
            end += 1;
            iter.next();
        }
        parts.push(if end == start {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
    }
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn file_ranges_are_compact() {
        assert_eq!(ranges(&[7, 0, 1, 2, 3, 5, 8, 9]), "0-3,5,7-9");
        assert_eq!(ranges(&[4]), "4");
    }

    fn listing(names: &[&str]) -> Listing {
        Listing {
            name: "Pack".into(),
            multi: true,
            files: names
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    let parts: Vec<String> = name.split('/').map(str::to_owned).collect();
                    Listed {
                        index,
                        path: parts.iter().collect(),
                        parts,
                        len: 10,
                    }
                })
                .collect(),
            total: 10 * names.len() as u64,
        }
    }

    #[test]
    fn multipart_companions_and_folders_stay_together() {
        let listing = listing(&[
            "Game A/setup.exe",
            "Game A/data.bin",
            "game.part1.rar",
            "game.part2.rar",
            "other.7z.001",
            "other.7z.002",
            "setup-1.bin",
            "setup.exe",
            "readme.txt",
            "Extras/cover.jpg",
        ]);
        let groups = listing.groups();
        let labels: Vec<_> = groups.iter().map(|group| group.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Game A/",
                "game.part1.rar (+1 more)",
                "other.7z.001 (+1 more)",
                "setup.exe (+1 more)"
            ]
        );
        assert_eq!(groups[1].files, [2, 3]);
        assert_eq!(groups[3].payload, Path::new("setup.exe"));
        let Question::ChooseAsset { assets } = listing.question(&"a".repeat(40), &groups) else {
            panic!("a payload question");
        };
        assert_eq!(assets.len(), 5);
        assert_eq!(assets[0].size, Some(100));
        assert!(
            assets[1].url.as_str().contains("so=0-1"),
            "{}",
            assets[1].url
        );
    }

    #[test]
    fn one_payload_with_extras_asks_nothing() {
        let listing = listing(&[
            "Game/game.exe",
            "Game/data.pak",
            "readme.nfo",
            "Covers/a.png",
        ]);
        assert_eq!(listing.groups().len(), 1);
    }
}
