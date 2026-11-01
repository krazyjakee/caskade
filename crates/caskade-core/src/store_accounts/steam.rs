//! Steam accounts (#166) through the Steam Web API: the user's own Web API
//! key and their profile (SteamID64, profile URL or custom URL name).
//!
//! - Sign-in resolves a custom URL name (`ISteamUser/ResolveVanityURL`),
//!   then reads the profile (`ISteamUser/GetPlayerSummaries`), which also
//!   checks the key. The key and SteamID64 are the sign-in, in the
//!   keychain; the SteamID64 is the account's ownership namespace.
//! - The library is `IPlayerService/GetOwnedGames` with app info and the
//!   free games the account has played. It lists games only: DLC, tools,
//!   soundtracks and Family Sharing loans aren't imported, and free games
//!   count once played, since only then does Steam list them as owned. A
//!   game's play time (`playtime_forever`) and last play
//!   (`rtime_last_played`, when not 0) come with it.
//! - A private profile answers with no game list at all, unless the key
//!   belongs to the same account: that is `Unavailable` with what to change,
//!   never an empty library.
//!
//! Steam doesn't say which systems a game runs on here: installing goes
//! through the Steam client (#170), which knows.

use super::{
    AccountProvider, AuthChallenge, AuthField, AuthInput, AuthKind, CancellationToken,
    LibrarySnapshot, OwnedProduct, ProviderCapabilities, ProviderError, SignedIn, StoreAccount,
    StoreCredentials, http,
};
use async_trait::async_trait;
use caskade_discovery::ExternalId;
use serde::Deserialize;
use std::collections::BTreeMap;
use url::Url;

pub const ID: &str = "steam";
/// Where people get a Web API key.
pub const KEY_PAGE: &str = "https://steamcommunity.com/dev/apikey";
const API: &str = "https://api.steampowered.com";
const STEAM: &str = "Steam";
/// SteamID64s of individual accounts start here.
const INDIVIDUAL: u64 = 0x0110_0001_0000_0000;

/// Steam through its Web API.
#[derive(Debug, Clone)]
pub struct SteamProvider {
    base: Url,
}

impl Default for SteamProvider {
    fn default() -> Self {
        Self {
            base: Url::parse(API).expect("the Steam API URL parses"),
        }
    }
}

impl SteamProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Another API root (tests).
    pub fn with_base(base: Url) -> Self {
        Self { base }
    }

    /// The Web API, for a key: what other Steam features (achievements,
    /// #186) build on, with the same error handling.
    pub fn api(&self, key: &str) -> SteamApi {
        SteamApi {
            base: self.base.clone(),
            key: key.to_owned(),
        }
    }
}

/// Calls to the Steam Web API with one key. `Debug` doesn't show the key.
#[derive(Clone)]
pub struct SteamApi {
    base: Url,
    key: String,
}

impl std::fmt::Debug for SteamApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SteamApi")
            .field("base", &self.base.as_str())
            .finish_non_exhaustive()
    }
}

impl SteamApi {
    /// `GET <interface>/<method>/v<version>/` with the key and `query`.
    pub async fn get<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        query: &[(&str, &str)],
    ) -> Result<T, ProviderError> {
        let url = self
            .base
            .join(method)
            .map_err(|_| ProviderError::Other(crate::tr!("bad Steam API method")))?;
        let request = http::client()?
            .get(url)
            .query(&[("key", self.key.as_str()), ("format", "json")])
            .query(query);
        http::json(request, STEAM).await
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    response: T,
}

#[derive(Deserialize)]
struct Vanity {
    success: u32,
    steamid: Option<String>,
}

#[derive(Deserialize, Default)]
struct Players {
    #[serde(default)]
    players: Vec<Player>,
}

#[derive(Deserialize)]
struct Player {
    steamid: String,
    personaname: Option<String>,
    /// 3 is public; anything else hides the game list from other keys.
    communityvisibilitystate: Option<u32>,
}

#[derive(Deserialize, Default)]
struct OwnedGames {
    game_count: Option<u64>,
    games: Option<Vec<OwnedGame>>,
}

#[derive(Deserialize)]
struct OwnedGame {
    appid: u32,
    name: Option<String>,
    #[serde(default)]
    playtime_forever: Option<u64>,
    #[serde(default)]
    rtime_last_played: Option<i64>,
}

/// The SteamID64 `profile` names, if it names one without asking Steam:
/// the number itself, or a `/profiles/<id>` URL.
fn steam_id(profile: &str) -> Option<String> {
    let profile = profile.trim().trim_end_matches('/');
    let id = profile
        .rsplit_once("/profiles/")
        .map_or(profile, |(_, id)| id)
        .trim_end_matches('/');
    let number: u64 = id.parse().ok()?;
    (number > INDIVIDUAL).then(|| number.to_string())
}

/// A custom URL name: `name`, or `…/id/name`.
fn vanity(profile: &str) -> &str {
    let profile = profile.trim().trim_end_matches('/');
    profile
        .rsplit_once("/id/")
        .map_or(profile, |(_, name)| name)
}

fn private() -> ProviderError {
    ProviderError::Unavailable(crate::tr!(
        "Steam keeps this profile's game details private. In Steam, set Privacy Settings → Game details to Public, or use a Web API key from this same account."
    ))
}

#[async_trait]
impl AccountProvider for SteamProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: ID.into(),
            name: STEAM.into(),
            auth: AuthKind::ApiKey,
            library: true,
            playtime: true,
            install: true,
        }
    }

    fn install_target(&self, product: &OwnedProduct) -> Option<super::InstallTarget> {
        Some(super::InstallTarget {
            launcher: "steam".into(),
            name: STEAM.into(),
            uri: format!("steam://install/{}", product.product_id),
        })
    }

    async fn begin_auth(&self, _flow_id: &str) -> Result<AuthChallenge, ProviderError> {
        Ok(AuthChallenge {
            url: Some(KEY_PAGE.into()),
            user_code: None,
            fields: vec![
                AuthField {
                    id: "profile".into(),
                    label: crate::tr!("Steam profile"),
                    description: Some(crate::tr!(
                        "Your SteamID64, profile link or custom URL name."
                    )),
                    secret: false,
                },
                AuthField {
                    id: "key".into(),
                    label: crate::tr!("Steam Web API key"),
                    description: Some(crate::tr!(
                        "From steamcommunity.com/dev/apikey while signed in to Steam."
                    )),
                    secret: true,
                },
            ],
            instructions: Some(crate::tr!(
                "Caskade reads your games with your own Steam Web API key. With a key from another account, your profile's game details must be public."
            )),
        })
    }

    async fn complete_auth(
        &self,
        _flow_id: &str,
        input: &AuthInput,
    ) -> Result<SignedIn, ProviderError> {
        let AuthInput::Fields { fields } = input else {
            return Err(ProviderError::Rejected(crate::tr!(
                "enter your Steam profile and Web API key"
            )));
        };
        let field = |id: &str| fields.get(id).map(|v| v.trim()).unwrap_or_default();
        let (profile, key) = (field("profile"), field("key"));
        if profile.is_empty() || key.is_empty() {
            return Err(ProviderError::Rejected(crate::tr!(
                "enter your Steam profile and Web API key"
            )));
        }
        let api = self.api(key);
        let refused = |error: ProviderError| match error {
            ProviderError::Expired(_) => {
                ProviderError::Rejected(crate::tr!("Steam refused the Web API key"))
            }
            other => other,
        };
        let steam_id = match steam_id(profile) {
            Some(id) => id,
            None => {
                let found: Envelope<Vanity> = api
                    .get(
                        "ISteamUser/ResolveVanityURL/v1/",
                        &[("vanityurl", vanity(profile))],
                    )
                    .await
                    .map_err(refused)?;
                match found.response {
                    Vanity {
                        success: 1,
                        steamid: Some(id),
                    } => id,
                    _ => {
                        return Err(ProviderError::Rejected(crate::tr!(
                            "Steam has no profile called “{profile}”",
                            profile = profile
                        )));
                    }
                }
            }
        };
        let players: Envelope<Players> = api
            .get(
                "ISteamUser/GetPlayerSummaries/v2/",
                &[("steamids", steam_id.as_str())],
            )
            .await
            .map_err(refused)?;
        let player = players
            .response
            .players
            .into_iter()
            .find(|player| player.steamid == steam_id)
            .ok_or_else(|| {
                ProviderError::Rejected(crate::tr!("Steam has no profile with that SteamID"))
            })?;
        if player.communityvisibilitystate.unwrap_or(3) != 3 {
            tracing::info!("the Steam profile is private; its key must be the account's own");
        }
        Ok(SignedIn {
            display_name: player.personaname.unwrap_or_else(|| steam_id.clone()),
            credentials: Some(StoreCredentials {
                access_token: key.to_owned(),
                refresh_token: None,
                expires_at_ms: None,
                extra: BTreeMap::from([("steamid".to_owned(), steam_id.clone())]),
            }),
            external_id: steam_id,
        })
    }

    async fn library(
        &self,
        account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        _cancel: &CancellationToken,
    ) -> Result<LibrarySnapshot, ProviderError> {
        let credentials =
            credentials.ok_or_else(|| ProviderError::Expired(crate::tr!("not signed in")))?;
        let steam_id = credentials
            .extra
            .get("steamid")
            .cloned()
            .unwrap_or_else(|| account.external_id.clone());
        let owned: Envelope<OwnedGames> = self
            .api(&credentials.access_token)
            .get(
                "IPlayerService/GetOwnedGames/v1/",
                &[
                    ("steamid", steam_id.as_str()),
                    ("include_appinfo", "1"),
                    ("include_played_free_games", "1"),
                    ("skip_unvetted_apps", "0"),
                ],
            )
            .await?;
        // A private library has neither field; an empty one says 0.
        let response = owned.response;
        if response.game_count.is_none() && response.games.is_none() {
            return Err(private());
        }
        let games = response.games;
        let products = games
            .unwrap_or_default()
            .into_iter()
            .map(|game| OwnedProduct {
                product_id: game.appid.to_string(),
                title: game
                    .name
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| format!("Steam app {}", game.appid)),
                ids: vec![ExternalId::Steam { app_id: game.appid }],
                platforms: Vec::new(),
                playtime_minutes: game.playtime_forever,
                last_played_ms: game
                    .rtime_last_played
                    .filter(|at| *at > 0)
                    .map(|at| at.saturating_mul(1000)),
                systems: Vec::new(),
            })
            .collect();
        Ok(LibrarySnapshot {
            products,
            complete: true,
        })
    }
}

#[cfg(test)]
#[path = "steam_tests.rs"]
mod tests;
