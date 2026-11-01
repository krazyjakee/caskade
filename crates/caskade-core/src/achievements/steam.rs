//! Steam achievements (#186) through the Steam Web API, with the Steam
//! account's own sign-in (#166: its Web API key and SteamID64).
//!
//! - Definitions: `ISteamUserStats/GetSchemaForGame/v2` (name, description,
//!   hidden, unlocked and locked icons). A game without achievements has
//!   none there and is reported as such, without asking for the player's.
//! - The account's state: `ISteamUserStats/GetPlayerAchievements/v1`
//!   (`achieved`, `unlocktime` in seconds; 0 when Steam doesn't say).
//!   Steam answers a private profile's game details with an error, never
//!   an empty list: that is `Unavailable` (private), and "no stats" is
//!   unsupported. Steam reports no progress towards an achievement here.
//! - Rarity: `GetGlobalAchievementPercentagesForApp/v2`, optional: a failure
//!   there leaves rarity out, never the achievements.
//!
//! Only reads; nothing is ever unlocked or changed on Steam.

use super::{
    AchievementCapabilities, AchievementDefinition, AchievementProvider, AchievementState,
    FetchError, Fetched, net,
};
use crate::store_accounts::{ProviderError, StoreAccount, StoreCredentials};
use async_trait::async_trait;
use caskade_discovery::ExternalId;
use reqwest::StatusCode;
use serde::Deserialize;
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;
use url::Url;

const API: &str = "https://api.steampowered.com/";
const STEAM: &str = "Steam";

/// Steam achievements through the Web API.
#[derive(Debug, Clone)]
pub struct SteamAchievements {
    base: Url,
}

impl Default for SteamAchievements {
    fn default() -> Self {
        Self {
            base: Url::parse(API).expect("the Steam API URL parses"),
        }
    }
}

impl SteamAchievements {
    pub fn new() -> Self {
        Self::default()
    }

    /// Another API root (tests).
    pub fn with_base(base: Url) -> Self {
        Self { base }
    }

    /// `GET method` with the key: the answer's body, and Steam's own error
    /// text when it refused.
    async fn get(
        &self,
        method: &str,
        key: &str,
        query: &[(&str, &str)],
    ) -> Result<(StatusCode, Vec<u8>), ProviderError> {
        let url = self
            .base
            .join(method)
            .map_err(|_| ProviderError::Other(crate::tr!("bad Steam API method")))?;
        let request = net::client()?
            .get(url)
            .query(&[("key", key), ("format", "json")])
            .query(query);
        net::send(request, STEAM).await
    }
}

fn credentials(credentials: Option<&StoreCredentials>) -> Result<(&str, &str), ProviderError> {
    let credentials =
        credentials.ok_or_else(|| ProviderError::Expired(crate::tr!("not signed in")))?;
    let steam_id = credentials
        .extra
        .get("steamid")
        .ok_or_else(|| ProviderError::Expired(crate::tr!("not signed in")))?;
    Ok((&credentials.access_token, steam_id))
}

fn unreadable() -> ProviderError {
    net::unreadable(STEAM)
}

#[derive(Deserialize, Default)]
struct Schema {
    #[serde(default)]
    game: SchemaGame,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SchemaGame {
    #[serde(default)]
    available_game_stats: Option<Stats>,
}
#[derive(Deserialize, Default)]
struct Stats {
    #[serde(default)]
    achievements: Vec<SchemaAchievement>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SchemaAchievement {
    name: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    hidden: u8,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    icongray: Option<String>,
}

#[derive(Deserialize)]
struct Player {
    playerstats: PlayerStats,
}
#[derive(Deserialize)]
struct PlayerStats {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    achievements: Vec<PlayerAchievement>,
}
#[derive(Deserialize)]
struct PlayerAchievement {
    apiname: String,
    #[serde(default)]
    achieved: u8,
    #[serde(default)]
    unlocktime: i64,
}

#[derive(Deserialize)]
struct Percentages {
    achievementpercentages: PercentageList,
}
#[derive(Deserialize)]
struct PercentageList {
    #[serde(default)]
    achievements: Vec<Percentage>,
}
#[derive(Deserialize)]
struct Percentage {
    name: String,
    /// A number, or a number in a string.
    percent: serde_json::Value,
}

#[async_trait]
impl AchievementProvider for SteamAchievements {
    fn id(&self) -> String {
        crate::store_accounts::steam::ID.into()
    }

    fn capabilities(&self) -> AchievementCapabilities {
        AchievementCapabilities {
            unlock_times: true,
            progress: false,
            rarity: true,
            hardcore: false,
            points: false,
        }
    }

    fn product(&self, id: &ExternalId) -> Option<String> {
        match id {
            ExternalId::Steam { app_id } => Some(app_id.to_string()),
            _ => None,
        }
    }

    async fn fetch(
        &self,
        _account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        product: &str,
        cancel: &CancellationToken,
    ) -> Result<Fetched, FetchError> {
        let (key, steam_id) = self::credentials(credentials)?;
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled.into());
        }
        let (status, body) = self
            .get(
                "ISteamUserStats/GetSchemaForGame/v2/",
                key,
                &[("appid", product), ("l", "english")],
            )
            .await?;
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(ProviderError::Expired(crate::tr!("Steam refused the Web API key")).into());
        }
        let schema: Schema = if status.is_success() {
            serde_json::from_slice(&body).map_err(|_| unreadable())?
        } else {
            // Steam answers 400 for an app it has no schema for.
            Schema::default()
        };
        let definitions: Vec<SchemaAchievement> = schema
            .game
            .available_game_stats
            .map(|stats| stats.achievements)
            .unwrap_or_default();
        if definitions.is_empty() {
            return Ok(Fetched {
                complete: true,
                ..Default::default()
            });
        }
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled.into());
        }
        let (status, body) = self
            .get(
                "ISteamUserStats/GetPlayerAchievements/v1/",
                key,
                &[("steamid", steam_id), ("appid", product), ("l", "english")],
            )
            .await?;
        let player: Option<Player> = serde_json::from_slice(&body).ok();
        let error = player
            .as_ref()
            .and_then(|player| player.playerstats.error.clone())
            .unwrap_or_default();
        let stats = match player {
            Some(player) if status.is_success() && player.playerstats.success => player.playerstats,
            _ if error.to_lowercase().contains("not public") => {
                return Err(ProviderError::Unavailable(crate::tr!(
                    "the Steam profile's game details are private: make them public in Steam's privacy settings"
                ))
                .into());
            }
            _ if error.to_lowercase().contains("no stats") => {
                return Err(FetchError::Unsupported(crate::tr!(
                    "Steam keeps no achievements for this game"
                )));
            }
            _ if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN => {
                return Err(
                    ProviderError::Expired(crate::tr!("Steam refused the Web API key")).into(),
                );
            }
            _ => {
                return Err(ProviderError::Other(crate::tr!(
                    "Steam answered HTTP {status}",
                    status = status.as_u16()
                ))
                .into());
            }
        };
        // Rarity is extra: without it the achievements still show.
        let rarity: BTreeMap<String, f32> = match self
            .get(
                "ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/",
                key,
                &[("gameid", product)],
            )
            .await
        {
            Ok((status, body)) if status.is_success() => {
                serde_json::from_slice::<Percentages>(&body)
                    .map(|found| {
                        found
                            .achievementpercentages
                            .achievements
                            .into_iter()
                            .filter_map(|entry| {
                                let percent = match &entry.percent {
                                    serde_json::Value::Number(n) => n.as_f64(),
                                    serde_json::Value::String(s) => s.trim().parse().ok(),
                                    _ => None,
                                }?;
                                Some((entry.name, percent.clamp(0.0, 100.0) as f32))
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            }
            _ => BTreeMap::new(),
        };
        let mut fetched = Fetched {
            complete: true,
            ..Default::default()
        };
        for state in stats.achievements {
            let unlocked = state.achieved == 1;
            fetched.states.insert(
                state.apiname,
                AchievementState {
                    unlocked,
                    unlocked_at_ms: (unlocked && state.unlocktime > 0)
                        .then(|| state.unlocktime.saturating_mul(1000)),
                    ..Default::default()
                },
            );
        }
        for definition in definitions {
            fetched.definitions.push(AchievementDefinition {
                name: definition
                    .display_name
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| definition.name.clone()),
                description: definition.description.filter(|d| !d.trim().is_empty()),
                hidden: definition.hidden == 1,
                icon_url: definition.icon.filter(|url| !url.is_empty()),
                locked_icon_url: definition.icongray.filter(|url| !url.is_empty()),
                progress_max: None,
                rarity_percent: rarity.get(&definition.name).copied(),
                points: None,
                id: definition.name,
            });
        }
        Ok(fetched)
    }
}

#[cfg(test)]
#[path = "steam_tests.rs"]
mod tests;
