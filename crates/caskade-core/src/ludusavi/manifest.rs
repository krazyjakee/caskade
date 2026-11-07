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

/// Each game of a manifest in turn, as [`parse`] reads them, without the
/// whole manifest in memory at once: parsed whole, Ludusavi's 17 MB manifest
/// took 600 MB. It is one mapping with a game's title at the start of each
/// top-level line, so each game is read from its own lines. Errors as
/// [`parse`] does; `each` is called with games read so far before then.
pub(super) fn each_game(
    bytes: &[u8],
    mut each: impl FnMut(String, Game) -> Result<()>,
) -> Result<()> {
    let mut any = false;
    for lines in top_level(bytes) {
        let games: BTreeMap<String, Game> = serde_yaml_ng::from_slice(lines)
            .map_err(|error| super::invalid(format!("invalid Ludusavi manifest: {error}")))?;
        for (title, mut game) in games {
            if title.trim().is_empty() {
                return Err(super::invalid("empty Ludusavi game title"));
            }
            game.title.clone_from(&title);
            any = true;
            each(title, game)?;
        }
    }
    if !any {
        return Err(super::invalid("empty Ludusavi manifest"));
    }
    Ok(())
}

/// A YAML mapping's entries, split where a line starts with neither space,
/// tab nor `#`: each begins with its key, and the lines after it, until the
/// next such line, hold its value. The document marker is left out.
fn top_level(bytes: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let begins = !matches!(
            line.first(),
            None | Some(b' ' | b'\t' | b'#' | b'\r' | b'\n')
        );
        if begins {
            starts.push(offset);
        }
        offset += line.len();
    }
    starts.push(bytes.len());
    starts
        .windows(2)
        .map(|pair| &bytes[pair[0]..pair[1]])
        .filter(|lines| !is_marker(lines))
        .collect()
}

/// `---` or `...` alone on its line.
fn is_marker(lines: &[u8]) -> bool {
    let text = lines.trim_ascii();
    text == b"---" || text == b"..."
}

/// The game `title` names, following aliases (`aliases`: each title's, if
/// any). None for an alias that leads nowhere or round in a circle.
pub(super) fn target<'a>(
    aliases: &'a BTreeMap<String, Option<String>>,
    title: &'a str,
) -> Option<&'a str> {
    let mut title = title;
    let mut seen = BTreeSet::new();
    while seen.insert(title) {
        match aliases.get(title)? {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn games(bytes: &[u8]) -> Result<Vec<(String, Game)>> {
        let mut games = Vec::new();
        each_game(bytes, |title, game| {
            games.push((title, game));
            Ok(())
        })?;
        Ok(games)
    }

    const MANIFEST: &str = r#"---
# Generated.
"A: The Game":
  files:
    "<base>/save.dat":
      tags:
        - save
  steam:
    id: 10
# Between games.
B Game:
  alias: "A: The Game"
'C "quoted"': {steam: {id: 30}, gog: {id: 31}}
D:
  registry:
    HKEY_CURRENT_USER/Software/D:
      when:
        - os: windows

      tags: [save]
  id:
    steamExtra: [40, 41]
...
"#;

    #[test]
    fn games_one_at_a_time_read_as_the_whole_manifest_does() {
        let streamed: BTreeMap<String, Game> =
            games(MANIFEST.as_bytes()).unwrap().into_iter().collect();
        assert_eq!(streamed, parse(MANIFEST.as_bytes()).unwrap());
        assert_eq!(
            streamed.keys().collect::<Vec<_>>(),
            ["A: The Game", "B Game", "C \"quoted\"", "D"]
        );
        assert_eq!(streamed["C \"quoted\""].gog.id, Some(31));
        assert_eq!(streamed["D"].id.steam_extra, [40, 41]);
    }

    #[test]
    fn a_bad_manifest_fails_as_a_whole_one_does() {
        for bad in [
            "",
            "---\n",
            "# nothing\n",
            "\"\":\n  steam:\n    id: 1\n",
            "A:\n  steam: [\n",
        ] {
            assert!(games(bad.as_bytes()).is_err(), "{bad:?}");
            assert!(parse(bad.as_bytes()).is_err(), "{bad:?}");
        }
    }
}
