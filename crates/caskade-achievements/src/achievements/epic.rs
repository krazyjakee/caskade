//! Epic Games Store achievements (#188), through the store's GraphQL
//! service (`store.epicgames.com/graphql`) as the store's achievement pages
//! and Playnite's SuccessStory read them, with the Epic account's own
//! sign-in (#168, the launcher's token and `account_id`).
//!
//! - A game's achievements belong to its sandbox, the namespace of its
//!   `ExternalId::Epic`: editions in one namespace share them, and games
//!   with the same title in other namespaces never do.
//! - Definitions: `Achievement.productAchievementsRecordBySandbox` (name,
//!   description, hidden, unlocked and locked icons, XP, rarity), which
//!   also names the sandbox's product. No record means the game uses no
//!   Epic achievements (unsupported); an empty list means none.
//! - The account's state: `PlayerProfile.playerProfile(epicAccountId)
//!   .productAchievements(productId)` (`unlocked`, `unlockDate`). An answer
//!   that isn't a success (a private profile) is `Unavailable`, never all
//!   locked.
//! - XP is kept as points. Epic's `progress` value has no documented unit,
//!   so progress stays unknown rather than guessed.
//!
//! Only reads; nothing is ever unlocked on Epic.

use super::{
    AchievementCapabilities, AchievementDefinition, AchievementProvider, AchievementState,
    FetchError, Fetched, net,
};
use async_trait::async_trait;
use caskade_core::store_accounts::{ProviderError, StoreAccount, StoreCredentials};
use caskade_discovery::ExternalId;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use url::Url;

const EPIC: &str = "Epic Games";
const GRAPHQL: &str = "https://store.epicgames.com/graphql";
const LOCALE: &str = "en-US";

const DEFINITIONS: &str = "query Achievement($sandboxId: String!, $locale: String!) {
  Achievement {
    productAchievementsRecordBySandbox(sandboxId: $sandboxId, locale: $locale) {
      productId
      sandboxId
      achievements {
        achievement {
          name hidden unlockedDisplayName lockedDisplayName unlockedDescription
          lockedDescription unlockedIconLink lockedIconLink XP rarity { percent }
        }
      }
    }
  }
}";

const PLAYER: &str = "query PlayerProfile($epicAccountId: String!, $productId: String!) {
  PlayerProfile {
    playerProfile(epicAccountId: $epicAccountId) {
      productAchievements(productId: $productId) {
        __typename
        ... on PlayerProductAchievementsResponseSuccess {
          data { playerAchievements { playerAchievement { achievementName unlocked unlockDate } } }
        }
      }
    }
  }
}";

/// Epic achievements through the store's GraphQL service.
#[derive(Debug, Clone)]
pub struct EpicAchievements {
    graphql: Url,
}

impl Default for EpicAchievements {
    fn default() -> Self {
        Self {
            graphql: Url::parse(GRAPHQL).expect("the Epic GraphQL URL parses"),
        }
    }
}

impl EpicAchievements {
    pub fn new() -> Self {
        Self::default()
    }

    /// Another GraphQL endpoint (tests).
    pub fn with_endpoint(graphql: Url) -> Self {
        Self { graphql }
    }

    async fn query<T: serde::de::DeserializeOwned>(
        &self,
        token: &str,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<Graph<T>, ProviderError> {
        let request = net::client()?
            .post(self.graphql.clone())
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(json!({"query": query, "variables": variables}).to_string());
        let (status, body) = net::send(request, EPIC).await?;
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(ProviderError::Expired(caskade_core::tr!(
                "{store} refused the sign-in (HTTP {status})",
                store = EPIC,
                status = status.as_u16()
            )));
        }
        if !status.is_success() {
            return Err(net::unexpected(EPIC, status));
        }
        serde_json::from_slice(&body).map_err(|_| net::unreadable(EPIC))
    }
}

#[derive(Deserialize)]
struct Graph<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphError>,
}
#[derive(Deserialize)]
struct GraphError {
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DefinitionsData {
    achievement: Option<AchievementRoot>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AchievementRoot {
    product_achievements_record_by_sandbox: Option<Record>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    product_id: String,
    #[serde(default)]
    achievements: Vec<Wrapped>,
}
#[derive(Deserialize)]
struct Wrapped {
    achievement: EpicAchievement,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EpicAchievement {
    name: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    unlocked_display_name: Option<String>,
    #[serde(default)]
    locked_display_name: Option<String>,
    #[serde(default)]
    unlocked_description: Option<String>,
    #[serde(default)]
    locked_description: Option<String>,
    #[serde(default)]
    unlocked_icon_link: Option<String>,
    #[serde(default)]
    locked_icon_link: Option<String>,
    #[serde(default, rename = "XP")]
    xp: Option<u32>,
    #[serde(default)]
    rarity: Option<Rarity>,
}
#[derive(Deserialize)]
struct Rarity {
    #[serde(default)]
    percent: Option<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PlayerData {
    player_profile: Option<ProfileRoot>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileRoot {
    player_profile: Option<Profile>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    product_achievements: Option<ProductAchievements>,
}
#[derive(Deserialize)]
struct ProductAchievements {
    #[serde(rename = "__typename", default)]
    typename: String,
    #[serde(default)]
    data: Option<PlayerAchievements>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerAchievements {
    #[serde(default)]
    player_achievements: Vec<PlayerWrapped>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerWrapped {
    player_achievement: PlayerAchievement,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerAchievement {
    achievement_name: String,
    #[serde(default)]
    unlocked: bool,
    #[serde(default)]
    unlock_date: Option<String>,
}

fn nonempty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

#[async_trait]
impl AchievementProvider for EpicAchievements {
    fn id(&self) -> String {
        caskade_core::store_accounts::epic::ID.into()
    }

    fn capabilities(&self) -> AchievementCapabilities {
        AchievementCapabilities {
            unlock_times: true,
            progress: false,
            rarity: true,
            hardcore: false,
            points: true,
        }
    }

    fn product(&self, id: &ExternalId) -> Option<String> {
        match id {
            ExternalId::Epic { namespace, .. } if !namespace.is_empty() => Some(namespace.clone()),
            _ => None,
        }
    }

    async fn fetch(
        &self,
        account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        product: &str,
        cancel: &CancellationToken,
    ) -> Result<Fetched, FetchError> {
        let credentials = credentials
            .ok_or_else(|| ProviderError::Expired(caskade_core::tr!("not signed in")))?;
        let account_id = credentials
            .extra
            .get("account_id")
            .map(String::as_str)
            .unwrap_or(&account.external_id);
        let token = &credentials.access_token;
        let definitions: Graph<DefinitionsData> = self
            .query(
                token,
                DEFINITIONS,
                json!({"sandboxId": product, "locale": LOCALE}),
            )
            .await?;
        let Some(record) = definitions
            .data
            .and_then(|data| data.achievement)
            .and_then(|root| root.product_achievements_record_by_sandbox)
        else {
            if let Some(error) = definitions.errors.first() {
                tracing::info!(message = %error.message, "Epic answered no achievement record");
            }
            return Err(FetchError::Unsupported(caskade_core::tr!(
                "this game uses no Epic achievements"
            )));
        };
        if record.achievements.is_empty() {
            return Ok(Fetched {
                complete: true,
                ..Default::default()
            });
        }
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled.into());
        }
        let player: Graph<PlayerData> = self
            .query(
                token,
                PLAYER,
                json!({"epicAccountId": account_id, "productId": record.product_id}),
            )
            .await?;
        let answer = player
            .data
            .and_then(|data| data.player_profile)
            .and_then(|root| root.player_profile)
            .and_then(|profile| profile.product_achievements);
        let states = match answer {
            Some(answer) if answer.typename.ends_with("Success") => answer
                .data
                .map(|data| data.player_achievements)
                .unwrap_or_default(),
            _ => {
                return Err(ProviderError::Unavailable(caskade_core::tr!(
                    "Epic doesn't show this account's achievements: check the account's privacy settings"
                ))
                .into());
            }
        };
        let mut fetched = Fetched {
            complete: true,
            ..Default::default()
        };
        for state in states {
            let state = state.player_achievement;
            fetched.states.insert(
                state.achievement_name,
                AchievementState {
                    unlocked: state.unlocked,
                    unlocked_at_ms: state
                        .unlocked
                        .then(|| state.unlock_date.as_deref().and_then(net::iso_ms))
                        .flatten(),
                    ..Default::default()
                },
            );
        }
        for wrapped in record.achievements {
            let achievement = wrapped.achievement;
            fetched.definitions.push(AchievementDefinition {
                name: nonempty(achievement.unlocked_display_name)
                    .or(nonempty(achievement.locked_display_name))
                    .unwrap_or_else(|| achievement.name.clone()),
                description: nonempty(achievement.unlocked_description)
                    .or(nonempty(achievement.locked_description)),
                hidden: achievement.hidden,
                icon_url: nonempty(achievement.unlocked_icon_link),
                locked_icon_url: nonempty(achievement.locked_icon_link),
                progress_max: None,
                rarity_percent: achievement
                    .rarity
                    .and_then(|rarity| rarity.percent)
                    .map(|percent| percent.clamp(0.0, 100.0)),
                points: achievement.xp,
                id: achievement.name,
            });
        }
        Ok(fetched)
    }
}

#[cfg(test)]
#[path = "epic_tests.rs"]
mod tests;
