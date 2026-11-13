//! RetroAchievements (#189): a read-only account connection and the
//! achievement progress of emulated games verified by ROM hash.
//!
//! - Connecting takes the user's name and their web API key
//!   (<https://retroachievements.org/settings>), checked with
//!   `API_GetUserProfile`. It is a store account with no library: the key
//!   lives in the keychain like any sign-in.
//! - A release is linked to a RetroAchievements game only when the
//!   RetroAchievements hash of each of its ROMs (see [`hash`]) is one of
//!   that game's hashes in `API_GetGameList` (`h=1`) for the system.
//!   Systems hashed in ways Caskade doesn't compute (discs), archives,
//!   unknown hashes (another revision or region, a hack) and hashes of
//!   several games stay unresolved with the reason; a title is never used.
//!   The user can link a game by hand instead.
//! - Progress is `API_GetGameInfoAndUserProgress`: softcore and hardcore
//!   unlocks are kept apart (`DateEarned`, `DateEarnedHardcore`, UTC).
//!   RetroAchievements has no hidden achievements. Rarity is unlocks over
//!   the game's distinct players; points are the achievement's points.
//!
//! Caskade only reads: achievements are earned in an emulator core that
//! supports RetroAchievements, signed in there.

pub mod hash;

use super::{
    AchievementCapabilities, AchievementDefinition, AchievementProvider, AchievementState,
    FetchError, Fetched, HardcoreUnlock, LinkOutcome, ReleaseLinker, net::iso_ms,
};
use async_trait::async_trait;
use caskade_core::{
    db::LibraryStore,
    model::{GameEntry, LaunchSpec},
    store_accounts::{
        AccountProvider, AuthChallenge, AuthField, AuthInput, AuthKind, LibrarySnapshot,
        ProviderCapabilities, ProviderError, SignedIn, StoreAccount, StoreCredentials, http,
    },
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use url::Url;

pub const ID: &str = "retroachievements";
const NAME: &str = "RetroAchievements";
const SITE: &str = "https://retroachievements.org";
const MEDIA: &str = "https://media.retroachievements.org";
/// Where people find their web API key.
pub const KEY_PAGE: &str = "https://retroachievements.org/settings";
/// A system's game list is asked for again after this long.
const LIST_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// A system's games with their hashes, shared by every link that reads it.
type Games = Arc<Vec<ListedGame>>;

/// RetroAchievements through its web API.
pub struct RetroAchievements {
    base: Url,
    media: String,
    /// Each system's games with their hashes, as last fetched.
    lists: tokio::sync::Mutex<HashMap<u32, (Instant, Games)>>,
}

impl Default for RetroAchievements {
    fn default() -> Self {
        Self::with_base(
            Url::parse(SITE).expect("the RetroAchievements URL parses"),
            MEDIA,
        )
    }
}

impl RetroAchievements {
    pub fn new() -> Self {
        Self::default()
    }

    /// Another site and media root (tests).
    pub fn with_base(base: Url, media: &str) -> Self {
        Self {
            base,
            media: media.trim_end_matches('/').into(),
            lists: Default::default(),
        }
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        key: &str,
        query: &[(&str, &str)],
    ) -> Result<T, ProviderError> {
        let url = self
            .base
            .join(&format!("API/{endpoint}"))
            .map_err(|error| ProviderError::Other(error.to_string()))?;
        let mut params: Vec<(&str, &str)> = query.to_vec();
        params.push(("y", key));
        http::json(http::client()?.get(url).query(&params), NAME).await
    }

    /// The games of `console` with their hashes, cached for a day.
    async fn games(&self, console: u32, key: &str) -> Result<Games, ProviderError> {
        let mut lists = self.lists.lock().await;
        if let Some((at, games)) = lists.get(&console)
            && at.elapsed() < LIST_TTL
        {
            return Ok(games.clone());
        }
        let console_id = console.to_string();
        let games: Games = Arc::new(
            self.get(
                "API_GetGameList.php",
                key,
                &[("i", &console_id), ("h", "1"), ("f", "1")],
            )
            .await?,
        );
        lists.insert(console, (Instant::now(), games.clone()));
        Ok(games)
    }

    fn badge(&self, name: &str, locked: bool) -> String {
        format!(
            "{}/Badge/{name}{}.png",
            self.media,
            if locked { "_lock" } else { "" }
        )
    }
}

fn key(credentials: Option<&StoreCredentials>) -> Result<(&str, &str), ProviderError> {
    let credentials =
        credentials.ok_or_else(|| ProviderError::Expired(caskade_core::tr!("not signed in")))?;
    let user = credentials
        .extra
        .get("username")
        .ok_or_else(|| ProviderError::Expired(caskade_core::tr!("not signed in")))?;
    Ok((user, &credentials.access_token))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListedGame {
    #[serde(rename = "ID")]
    id: u64,
    #[serde(default)]
    hashes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Profile {
    user: String,
    #[serde(rename = "ULID", default)]
    ulid: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Progress {
    #[serde(rename = "ID", default)]
    id: Option<u64>,
    #[serde(default)]
    num_distinct_players: Option<u64>,
    /// A map by id; an empty list when the game has none.
    #[serde(default)]
    achievements: Achievements,
}

#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
enum Achievements {
    Map(BTreeMap<String, RaAchievement>),
    List(Vec<RaAchievement>),
    #[default]
    None,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RaAchievement {
    #[serde(rename = "ID")]
    id: u64,
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    points: Option<u32>,
    #[serde(default)]
    badge_name: Option<String>,
    #[serde(default)]
    display_order: Option<i64>,
    #[serde(default)]
    num_awarded: Option<u64>,
    #[serde(default)]
    date_earned: Option<String>,
    #[serde(default)]
    date_earned_hardcore: Option<String>,
}

#[async_trait]
impl AccountProvider for RetroAchievements {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: ID.into(),
            name: NAME.into(),
            auth: AuthKind::ApiKey,
            library: false,
            playtime: false,
            install: false,
        }
    }

    async fn begin_auth(&self, _flow_id: &str) -> Result<AuthChallenge, ProviderError> {
        Ok(AuthChallenge {
            url: Some(KEY_PAGE.into()),
            user_code: None,
            fields: vec![
                AuthField {
                    id: "username".into(),
                    label: caskade_core::tr!("RetroAchievements user name"),
                    description: None,
                    secret: false,
                },
                AuthField {
                    id: "key".into(),
                    label: caskade_core::tr!("Web API key"),
                    description: Some(caskade_core::tr!(
                        "From the Keys section of your RetroAchievements settings."
                    )),
                    secret: true,
                },
            ],
            instructions: Some(caskade_core::tr!(
                "Caskade reads your progress only. Achievements are earned in an emulator core that supports RetroAchievements, signed in there."
            )),
        })
    }

    async fn complete_auth(
        &self,
        _flow_id: &str,
        input: &AuthInput,
    ) -> Result<SignedIn, ProviderError> {
        let AuthInput::Fields { fields } = input else {
            return Err(ProviderError::Rejected(caskade_core::tr!(
                "enter your user name and web API key"
            )));
        };
        let user = fields.get("username").map(|v| v.trim()).unwrap_or_default();
        let key = fields.get("key").map(|v| v.trim()).unwrap_or_default();
        if user.is_empty() || key.is_empty() {
            return Err(ProviderError::Rejected(caskade_core::tr!(
                "enter your user name and web API key"
            )));
        }
        let profile: Profile = self
            .get("API_GetUserProfile.php", key, &[("u", user)])
            .await
            .map_err(|error| match error {
                ProviderError::Expired(_) => ProviderError::Rejected(caskade_core::tr!(
                    "RetroAchievements didn't accept that user name and key"
                )),
                other => other,
            })?;
        Ok(SignedIn {
            external_id: profile.ulid.clone().unwrap_or_else(|| profile.user.clone()),
            display_name: profile.user.clone(),
            credentials: Some(StoreCredentials {
                access_token: key.into(),
                refresh_token: None,
                expires_at_ms: None,
                extra: [("username".to_owned(), profile.user)].into(),
            }),
        })
    }

    async fn library(
        &self,
        _account: &StoreAccount,
        _credentials: Option<&StoreCredentials>,
        _cancel: &CancellationToken,
    ) -> Result<LibrarySnapshot, ProviderError> {
        // A connection for achievements: it owns no games.
        Ok(LibrarySnapshot {
            products: vec![],
            complete: true,
        })
    }
}

#[async_trait]
impl AchievementProvider for RetroAchievements {
    fn id(&self) -> String {
        ID.into()
    }

    fn capabilities(&self) -> AchievementCapabilities {
        AchievementCapabilities {
            unlock_times: true,
            progress: false,
            rarity: true,
            hardcore: true,
            points: true,
        }
    }

    async fn fetch(
        &self,
        _account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        product: &str,
        cancel: &CancellationToken,
    ) -> Result<Fetched, FetchError> {
        let (user, key) = key(credentials)?;
        let query = [("g", product), ("u", user), ("a", "1")];
        let request = self.get("API_GetGameInfoAndUserProgress.php", key, &query);
        let progress: Progress = tokio::select! {
            _ = cancel.cancelled() => return Err(ProviderError::Cancelled.into()),
            progress = request => progress?,
        };
        if progress.id.is_none() {
            return Err(FetchError::Unsupported(caskade_core::tr!(
                "RetroAchievements has no game {id}",
                id = product
            )));
        }
        let mut achievements = match progress.achievements {
            Achievements::Map(map) => map.into_values().collect(),
            Achievements::List(list) => list,
            Achievements::None => vec![],
        };
        achievements.sort_by_key(|a| (a.display_order.unwrap_or(i64::MAX), a.id));
        let players = progress.num_distinct_players.filter(|n| *n > 0);
        let mut fetched = Fetched {
            complete: true,
            ..Default::default()
        };
        for achievement in achievements {
            let id = achievement.id.to_string();
            let earned = achievement.date_earned.as_deref().and_then(iso_ms);
            let hardcore = achievement.date_earned_hardcore.as_deref().and_then(iso_ms);
            fetched.states.insert(
                id.clone(),
                AchievementState {
                    // A hardcore unlock is also a softcore one.
                    unlocked: earned.is_some() || hardcore.is_some(),
                    unlocked_at_ms: earned.or(hardcore),
                    progress: None,
                    hardcore: Some(HardcoreUnlock {
                        unlocked: hardcore.is_some(),
                        unlocked_at_ms: hardcore,
                    }),
                },
            );
            fetched.definitions.push(AchievementDefinition {
                id,
                name: achievement.title,
                description: achievement.description.filter(|d| !d.is_empty()),
                hidden: false,
                icon_url: achievement
                    .badge_name
                    .as_deref()
                    .map(|b| self.badge(b, false)),
                locked_icon_url: achievement
                    .badge_name
                    .as_deref()
                    .map(|b| self.badge(b, true)),
                progress_max: None,
                rarity_percent: match (achievement.num_awarded, players) {
                    (Some(awarded), Some(players)) => {
                        Some((awarded as f64 / players as f64 * 100.0).min(100.0) as f32)
                    }
                    _ => None,
                },
                points: achievement.points,
            });
        }
        Ok(fetched)
    }
}

#[async_trait]
impl ReleaseLinker for RetroAchievements {
    fn provider(&self) -> String {
        ID.into()
    }

    fn applies(&self, entry: &GameEntry) -> bool {
        matches!(&entry.launch, Some(LaunchSpec::Emulator { roms, .. }) if !roms.is_empty())
    }

    async fn link(
        &self,
        store: &LibraryStore,
        entry: &GameEntry,
        _account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        cancel: &CancellationToken,
    ) -> Result<LinkOutcome, FetchError> {
        let Some(LaunchSpec::Emulator {
            platform_id, roms, ..
        }) = &entry.launch
        else {
            return Ok(LinkOutcome::Unresolved(caskade_core::tr!(
                "not an emulated game"
            )));
        };
        let Some((console, rule)) = hash::console(platform_id) else {
            return Ok(LinkOutcome::Unresolved(caskade_core::tr!(
                "RetroAchievements doesn't cover this system"
            )));
        };
        let mut hashes = vec![];
        for rom in roms {
            let path = match rom.path(store).await {
                Ok(path) => path,
                Err(error) => return Ok(LinkOutcome::Unresolved(error.to_string())),
            };
            match hash::hash_file(&path, rule).await {
                Ok(hash) => hashes.push(hash),
                Err(hash::HashError::UnsupportedSystem) => {
                    return Ok(LinkOutcome::Unresolved(caskade_core::tr!(
                        "RetroAchievements identifies this system's games by a hash Caskade doesn't compute: link the game by hand"
                    )));
                }
                Err(hash::HashError::Archive) => {
                    return Ok(LinkOutcome::Unresolved(caskade_core::tr!(
                        "the ROM is compressed; RetroAchievements identifies what is inside: link the game by hand"
                    )));
                }
                Err(hash::HashError::TooLarge) => {
                    return Ok(LinkOutcome::Unresolved(caskade_core::tr!(
                        "the ROM is too large to be a cartridge: link the game by hand"
                    )));
                }
                Err(hash::HashError::Read(why)) => {
                    return Ok(LinkOutcome::Unresolved(caskade_core::tr!(
                        "the ROM couldn't be read: {why}",
                        why = why
                    )));
                }
            }
        }
        let (_, key) = key(credentials)?;
        let games = tokio::select! {
            _ = cancel.cancelled() => return Err(ProviderError::Cancelled.into()),
            games = self.games(console, key) => games?,
        };
        // Every ROM of the release must be one game's: discs of one game
        // share it, and a revision or region with another hash isn't it.
        let mut found = BTreeSet::new();
        for hash in &hashes {
            let owners: Vec<u64> = games
                .iter()
                .filter(|game| {
                    game.hashes
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(hash))
                })
                .map(|game| game.id)
                .collect();
            match owners.as_slice() {
                [] => {
                    return Ok(LinkOutcome::Unresolved(caskade_core::tr!(
                        "RetroAchievements doesn't know this ROM (hash {hash}): another revision, region or a modified copy",
                        hash = hash
                    )));
                }
                [one] => {
                    found.insert(*one);
                }
                _ => {
                    return Ok(LinkOutcome::Unresolved(caskade_core::tr!(
                        "this ROM's hash belongs to more than one RetroAchievements game: link it by hand"
                    )));
                }
            }
        }
        match found.into_iter().collect::<Vec<_>>().as_slice() {
            [game] => Ok(LinkOutcome::Linked {
                product: game.to_string(),
                hash: hashes.join(","),
            }),
            _ => Ok(LinkOutcome::Unresolved(caskade_core::tr!(
                "the release's ROMs are different RetroAchievements games: link it by hand"
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
