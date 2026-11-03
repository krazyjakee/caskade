//! Built-in data tables shipped in `data/`: Steam tools, launchers,
//! publishers and engine fingerprints. They are embedded at build time and
//! validated once, on first use.

use serde::Deserialize;
use std::{collections::BTreeSet, sync::OnceLock};
use thiserror::Error;

/// Steam app ids that are not games, used when `appinfo.vdf` is unavailable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteamTools {
    #[serde(default, rename = "tool")]
    pub tools: Vec<SteamTool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteamTool {
    pub app_id: u32,
    pub name: String,
}

/// Launchers, so they can be recognised and excluded from game results.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launchers {
    #[serde(default, rename = "launcher")]
    pub launchers: Vec<Launcher>,
    /// Battle.net product codes (as in `product.db`) and their titles.
    #[serde(default, rename = "battlenet")]
    pub battlenet: Vec<BattleNetProduct>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BattleNetProduct {
    /// `product.db` product code, e.g. `wow`.
    pub code: String,
    pub title: String,
    /// The `battlenet://<launch>` code, e.g. `WoW`.
    pub launch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launcher {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub exes: Vec<String>,
    #[serde(default)]
    pub bundle_ids: Vec<String>,
    #[serde(default)]
    pub flatpak_ids: Vec<String>,
    #[serde(default)]
    pub uri_schemes: Vec<String>,
}

/// Publisher names for the classifier; matching ignores case.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publishers {
    #[serde(default)]
    pub games: Vec<String>,
    #[serde(default)]
    pub non_games: Vec<String>,
}

/// Engine and store fingerprints for the classifier.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Engines {
    #[serde(default, rename = "engine")]
    pub engines: Vec<Engine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Engine {
    pub id: String,
    pub name: String,
    /// Globs relative to the install directory.
    pub files: Vec<String>,
}

/// All built-in tables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Data {
    pub steam_tools: SteamTools,
    pub launchers: Launchers,
    pub publishers: Publishers,
    pub engines: Engines,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DataError {
    #[error("data/{file}: {message}")]
    Parse { file: &'static str, message: String },
    #[error("data/{file}: {message}")]
    Invalid { file: &'static str, message: String },
}

const STEAM_TOOLS: &str = include_str!("../data/steam-tools.toml");
const LAUNCHERS: &str = include_str!("../data/launchers.toml");
const PUBLISHERS: &str = include_str!("../data/publishers.toml");
const ENGINES: &str = include_str!("../data/engines.toml");

impl Data {
    /// The embedded tables, parsed and validated once.
    pub fn builtin() -> Result<&'static Data, DataError> {
        static DATA: OnceLock<Result<Data, DataError>> = OnceLock::new();
        DATA.get_or_init(|| Data::parse(STEAM_TOOLS, LAUNCHERS, PUBLISHERS, ENGINES))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Parses and validates the four tables from TOML text.
    pub fn parse(
        steam_tools: &str,
        launchers: &str,
        publishers: &str,
        engines: &str,
    ) -> Result<Data, DataError> {
        let data = Data {
            steam_tools: table("steam-tools.toml", steam_tools)?,
            launchers: table("launchers.toml", launchers)?,
            publishers: table("publishers.toml", publishers)?,
            engines: table("engines.toml", engines)?,
        };
        data.validate()?;
        Ok(data)
    }

    fn validate(&self) -> Result<(), DataError> {
        unique(
            "steam-tools.toml",
            "app_id",
            self.steam_tools
                .tools
                .iter()
                .map(|tool| tool.app_id.to_string()),
        )?;
        unique(
            "launchers.toml",
            "id",
            self.launchers
                .launchers
                .iter()
                .map(|launcher| launcher.id.clone()),
        )?;
        unique(
            "launchers.toml",
            "battlenet code",
            self.launchers
                .battlenet
                .iter()
                .map(|product| product.code.clone()),
        )?;
        unique(
            "publishers.toml",
            "name",
            self.publishers
                .games
                .iter()
                .chain(&self.publishers.non_games)
                .map(|name| name.to_lowercase()),
        )?;
        unique(
            "engines.toml",
            "id",
            self.engines.engines.iter().map(|engine| engine.id.clone()),
        )?;
        for engine in &self.engines.engines {
            if engine.files.is_empty() {
                return Err(DataError::Invalid {
                    file: "engines.toml",
                    message: format!("engine {:?} has no files", engine.id),
                });
            }
        }
        Ok(())
    }
}

fn table<T: for<'de> Deserialize<'de>>(file: &'static str, text: &str) -> Result<T, DataError> {
    toml::from_str(text).map_err(|error| DataError::Parse {
        file,
        message: error.message().to_owned(),
    })
}

fn unique(
    file: &'static str,
    field: &str,
    values: impl Iterator<Item = String>,
) -> Result<(), DataError> {
    let mut seen = BTreeSet::new();
    for value in values {
        if value.trim().is_empty() {
            return Err(DataError::Invalid {
                file,
                message: format!("empty {field}"),
            });
        }
        if !seen.insert(value.clone()) {
            return Err(DataError::Invalid {
                file,
                message: format!("duplicate {field} {value:?}"),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tables_load() {
        Data::builtin().unwrap();
    }

    #[test]
    fn tables_parse_and_reject_bad_rows() {
        let launchers = "[[launcher]]\nid = \"steam\"\nname = \"Steam\"\nexes = [\"steam.exe\"]\n";
        let data = Data::parse("", launchers, "games = [\"Valve\"]", "").unwrap();
        assert_eq!(data.launchers.launchers[0].exes, ["steam.exe"]);

        let duplicate = format!("{launchers}{launchers}");
        assert!(matches!(
            Data::parse("", &duplicate, "", ""),
            Err(DataError::Invalid {
                file: "launchers.toml",
                ..
            })
        ));
        assert!(matches!(
            Data::parse(
                "[[tool]]\napp_id = 1\nname = \"x\"\nextra = 1\n",
                "",
                "",
                ""
            ),
            Err(DataError::Parse {
                file: "steam-tools.toml",
                ..
            })
        ));
        assert!(matches!(
            Data::parse("", "", "games = [\"Valve\"]\nnon_games = [\"valve\"]", ""),
            Err(DataError::Invalid {
                file: "publishers.toml",
                ..
            })
        ));
        assert!(matches!(
            Data::parse(
                "",
                "",
                "",
                "[[engine]]\nid = \"u\"\nname = \"U\"\nfiles = []\n"
            ),
            Err(DataError::Invalid {
                file: "engines.toml",
                ..
            })
        ));
    }
}
