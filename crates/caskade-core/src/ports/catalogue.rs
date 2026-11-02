//! Decomp Buddy's published install plans (`plans.json`): for each
//! decompilation, recompilation or source-port project, the game it ports,
//! the release file per system and the game files it needs. Read leniently:
//! a project that doesn't parse is left out, never the whole catalogue.

use super::{consoles, title};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The parsed catalogue.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    pub projects: Vec<Project>,
}

/// A port's repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The catalogue's key, e.g. `github:zelda64recomp/zelda64recomp`.
    pub key: String,
    pub host: Host,
    pub owner: String,
    pub repo: String,
    /// The release the plan was written for.
    pub tag: Option<String>,
    pub games: Vec<PortGame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    GitHub,
    GitLab,
}

impl Project {
    pub fn url(&self) -> String {
        match self.host {
            Host::GitHub => format!("https://github.com/{}/{}", self.owner, self.repo),
            Host::GitLab => format!("https://gitlab.com/{}/{}", self.owner, self.repo),
        }
    }
}

/// A game a project ports, with its plan for each system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortGame {
    pub title: String,
    pub console: String,
    pub families: BTreeSet<String>,
    /// Normalized titles that name the game: its title, its parts split on
    /// " / ", and the game files' labels ("Majora's Mask ROM").
    pub keys: BTreeSet<String>,
    /// Halves of a "Series: Subtitle" title, which only match a library
    /// game's whole title.
    pub parts: BTreeSet<String>,
    /// By the catalogue's system names: `windows`, `linux`, `macos-arm64`, …
    pub targets: BTreeMap<String, Target>,
}

/// How the project installs on one system.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Target {
    /// `release`, `source`, `unsupported` or `web_only`.
    pub method: String,
    pub asset: Option<String>,
    pub tag: Option<String>,
    /// The program to start, as the plan names it.
    pub executable: Option<String>,
    /// The SHA-256 of the release file the catalogue scanned.
    pub sha256: Option<String>,
    pub files: Vec<GameFile>,
    pub notes: Vec<String>,
    pub unsure: Vec<String>,
    pub confidence: Option<String>,
}

impl Target {
    /// Installable straight from a release file.
    pub fn installable(&self) -> bool {
        self.method == "release" && self.asset.is_some() && self.tag.is_some()
    }
}

/// A file the user supplies.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GameFile {
    pub label: String,
    /// `game`, `bios`, `dlc`, `update` or `other`.
    pub kind: String,
    /// `in_app_picker`, `folder` or `command_line`.
    pub how: String,
    /// The folder inside the install it goes in, for `folder`.
    pub drop_into: Option<String>,
    pub expected_filename: Option<String>,
    pub formats: Vec<String>,
    /// `USA`, `Europe`, `Japan`, `any` or `unknown`.
    pub region: Option<String>,
    pub sha1: Vec<String>,
}

#[derive(Deserialize)]
struct Raw {
    plans: BTreeMap<String, Value>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawPlan {
    tag: Option<String>,
    targets: BTreeMap<String, Option<RawTarget>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawTarget {
    games: Vec<RawGame>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawGame {
    game_title: Option<String>,
    console: Option<String>,
    build: RawBuild,
    game_files: Vec<RawFile>,
    notes: Vec<String>,
    unsure: Vec<String>,
    confidence: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawBuild {
    method: Option<String>,
    release_asset: Option<String>,
    release_tag: Option<String>,
    executable: Option<String>,
    scanned: Option<RawScanned>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawScanned {
    sha256: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawFile {
    label: Option<String>,
    kind: Option<String>,
    how_provided: Option<String>,
    drop_into: Option<String>,
    expected_filename: Option<String>,
    accepted_formats: Vec<String>,
    expected_region: Option<String>,
    expected_hashes: Vec<RawHash>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawHash {
    algo: String,
    value: String,
}

/// Parses `plans.json`. Fails only when the file isn't the catalogue at all.
pub fn parse(bytes: &[u8]) -> serde_json::Result<Catalogue> {
    let raw: Raw = serde_json::from_slice(bytes)?;
    let projects = raw
        .plans
        .into_iter()
        .filter_map(|(key, value)| project(&key, serde_json::from_value(value).ok()?))
        .collect();
    Ok(Catalogue { projects })
}

fn project(key: &str, plan: RawPlan) -> Option<Project> {
    let (host, path) = key.split_once(':')?;
    let host = match host {
        "github" => Host::GitHub,
        "gitlab" => Host::GitLab,
        _ => return None,
    };
    let (owner, repo) = path.split_once('/')?;
    let name = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
            && part != "."
            && part != ".."
    };
    if !name(owner) || !name(repo) {
        return None;
    }
    let mut games: Vec<PortGame> = Vec::new();
    for (system, target) in plan.targets {
        for raw in target.map(|target| target.games).unwrap_or_default() {
            let (Some(game_title), Some(console)) = (raw.game_title, raw.console) else {
                continue;
            };
            let game_title = game_title.trim().to_owned();
            if game_title.is_empty() {
                continue;
            }
            let files: Vec<GameFile> = raw.game_files.into_iter().map(file).collect();
            let index = match games
                .iter()
                .position(|game| game.title == game_title && game.console == console)
            {
                Some(index) => index,
                None => {
                    games.push(PortGame {
                        families: consoles::families(&console),
                        keys: BTreeSet::new(),
                        parts: BTreeSet::new(),
                        title: game_title.clone(),
                        console,
                        targets: BTreeMap::new(),
                    });
                    games.len() - 1
                }
            };
            let game = &mut games[index];
            title::keys_of(&game_title, &mut game.keys, &mut game.parts);
            for file in files.iter().filter(|file| file.kind == "game") {
                if let Some(key) = title::label_key(&file.label) {
                    game.keys.insert(key);
                }
            }
            let sha256 = raw
                .build
                .scanned
                .and_then(|scanned| scanned.sha256)
                .map(|hash| hash.to_ascii_lowercase())
                .filter(|hash| {
                    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                });
            game.targets.insert(
                system.clone(),
                Target {
                    method: raw.build.method.unwrap_or_default(),
                    asset: raw.build.release_asset.filter(|asset| plain(asset)),
                    tag: raw.build.release_tag.filter(|tag| !tag.trim().is_empty()),
                    executable: raw.build.executable.filter(|exe| !exe.trim().is_empty()),
                    sha256,
                    files,
                    notes: raw.notes,
                    unsure: raw.unsure,
                    confidence: raw.confidence,
                },
            );
        }
    }
    games.retain(|game| !game.keys.is_empty());
    (!games.is_empty()).then(|| Project {
        key: key.to_owned(),
        host,
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        tag: plan.tag,
        games,
    })
}

/// A release file name: no folders, nothing hidden.
fn plain(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.')
}

fn file(raw: RawFile) -> GameFile {
    GameFile {
        label: raw.label.unwrap_or_default(),
        kind: raw.kind.unwrap_or_else(|| "game".into()),
        how: raw.how_provided.unwrap_or_else(|| "in_app_picker".into()),
        drop_into: raw.drop_into.filter(|folder| !folder.trim().is_empty()),
        expected_filename: raw.expected_filename,
        formats: raw
            .accepted_formats
            .into_iter()
            .map(|format| format.to_lowercase())
            .collect(),
        region: raw.expected_region,
        sha1: raw
            .expected_hashes
            .into_iter()
            .filter(|hash| hash.algo.eq_ignore_ascii_case("sha1"))
            .map(|hash| hash.value.to_ascii_lowercase())
            .collect(),
    }
}
