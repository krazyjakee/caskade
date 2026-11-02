//! Loose manifest schema: unknown fields, OSes, stores and tags are retained.
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy)]
pub enum StoreId {
    Steam(u64),
    Gog(u64),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraint {
    pub os: Option<String>,
    pub store: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub tags: Vec<String>,
    pub when: Vec<Constraint>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Ids {
    pub steam_extra: Vec<u64>,
    pub gog_extra: Vec<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrimaryId {
    pub id: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Game {
    /// The manifest's mapping key; assigned during indexing.
    pub title: String,
    pub alias: Option<String>,
    pub files: BTreeMap<String, Entry>,
    pub registry: BTreeMap<String, Entry>,
    pub steam: PrimaryId,
    pub gog: PrimaryId,
    pub id: Ids,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub(super) fn parse(bytes: &[u8]) -> Result<BTreeMap<String, Game>> {
    let mut games: BTreeMap<String, Game> = serde_yaml_ng::from_slice(bytes)
        .map_err(|error| super::invalid(format!("invalid Ludusavi manifest: {error}")))?;
    if games.is_empty() {
        return Err(super::invalid("empty Ludusavi manifest"));
    }
    for (title, game) in &mut games {
        if title.trim().is_empty() {
            return Err(super::invalid("empty Ludusavi game title"));
        }
        game.title.clone_from(title);
    }
    Ok(games)
}

pub(super) fn target<'a>(games: &'a BTreeMap<String, Game>, title: &'a str) -> Option<&'a str> {
    let mut title = title;
    let mut seen = BTreeSet::new();
    while seen.insert(title) {
        let game = games.get(title)?;
        match &game.alias {
            Some(alias) => title = alias,
            None => return Some(title),
        }
    }
    None
}

/// Parse a game's `.ludusavi.yaml` during discovery, before lookups. Keys in
/// this secondary manifest add to or override the primary game's fields.
/// The caller indexes/retains the result; this is never called by lookups.
pub fn apply_secondary(primary: &Game, bytes: &[u8]) -> Result<Game> {
    let mut games = parse(bytes)?;
    let Some(secondary) = games.remove(&primary.title) else {
        return Ok(primary.clone());
    };
    Ok(merge_secondary(primary, secondary))
}

pub(super) fn merge_secondary(primary: &Game, secondary: Game) -> Game {
    let mut result = primary.clone();
    result.files.extend(secondary.files);
    result.registry.extend(secondary.registry);
    if secondary.steam.id.is_some() {
        result.steam = secondary.steam;
    }
    if secondary.gog.id.is_some() {
        result.gog = secondary.gog;
    }
    result.id.steam_extra.extend(secondary.id.steam_extra);
    result.id.gog_extra.extend(secondary.id.gog_extra);
    result.id.extra.extend(secondary.id.extra);
    result.extra.extend(secondary.extra);
    result
}
