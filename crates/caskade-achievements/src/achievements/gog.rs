//! GOG achievements (#187), the way GOG Galaxy reads them; GOG publishes no
//! other achievement interface. With the GOG account's own sign-in (#167):
//!
//! 1. The game's Galaxy client: the newest build of the product
//!    (`content-system.gog.com/products/<id>/os/<os>/builds?generation=2`,
//!    Windows first, then macOS) links to a manifest (zlib-compressed JSON)
//!    that names the game's `clientId` and `clientSecret`. A product with no
//!    such build, or a manifest without a client, doesn't use Galaxy's
//!    achievements: unsupported, never "all locked".
//! 2. A token for that client: `auth.gog.com/token` with the account's
//!    refresh token (`grant_type=refresh_token`, `without_new_session=1`, as
//!    Galaxy does, so the account's own sign-in stays as it was).
//! 3. The definitions and the account's unlocks, together:
//!    `gameplay.gog.com/clients/<client>/users/<user>/achievements`, page by
//!    page (`page_token`). `visible: false` is a hidden achievement;
//!    `date_unlocked` (ISO 8601) is the unlock; `rarity` is a percentage.
//!    A page that fails leaves an incomplete answer, which changes only
//!    what it lists.
//!
//! GOG gives no progress towards an achievement or points: they stay
//! unknown. Only reads; nothing is ever unlocked on GOG.

use super::{
    AchievementCapabilities, AchievementDefinition, AchievementProvider, AchievementState,
    FetchError, Fetched, net,
};
use async_trait::async_trait;
use caskade_core::store_accounts::{ProviderError, StoreAccount, StoreCredentials};
use caskade_discovery::ExternalId;
use reqwest::StatusCode;
use serde::Deserialize;
use std::{collections::HashMap, io::Read};
use tokio_util::sync::CancellationToken;
use url::Url;

const GOG: &str = "GOG";
const CONTENT: &str = "https://content-system.gog.com";
const AUTH: &str = "https://auth.gog.com";
const GAMEPLAY: &str = "https://gameplay.gog.com";
/// At most this many pages of achievements.
const MAX_PAGES: u32 = 50;

/// A game's Galaxy client.
#[derive(Clone)]
struct Client {
    id: String,
    secret: String,
}

/// GOG achievements through Galaxy's services.
pub struct GogAchievements {
    content: Url,
    auth: Url,
    gameplay: Url,
    /// Each product's Galaxy client (`None`: it has none), as found.
    clients: tokio::sync::Mutex<HashMap<String, Option<Client>>>,
}

impl Default for GogAchievements {
    fn default() -> Self {
        Self::with_bases(
            Url::parse(CONTENT).expect("GOG content URL"),
            Url::parse(AUTH).expect("GOG auth URL"),
            Url::parse(GAMEPLAY).expect("GOG gameplay URL"),
        )
    }
}

impl GogAchievements {
    pub fn new() -> Self {
        Self::default()
    }

    /// Other roots for GOG's services (tests).
    pub fn with_bases(content: Url, auth: Url, gameplay: Url) -> Self {
        Self {
            content,
            auth,
            gameplay,
            clients: Default::default(),
        }
    }

    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Option<T>, ProviderError> {
        let (status, body) = net::send(request, GOG).await?;
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(ProviderError::Expired(caskade_core::tr!(
                "{store} refused the sign-in (HTTP {status})",
                store = GOG,
                status = status.as_u16()
            )));
        }
        if !status.is_success() {
            return Err(net::unexpected(GOG, status));
        }
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(|_| net::unreadable(GOG))
    }

    /// The product's Galaxy client, found once and kept.
    async fn client(&self, product: &str) -> Result<Option<Client>, ProviderError> {
        if let Some(known) = self.clients.lock().await.get(product) {
            return Ok(known.clone());
        }
        let mut found = None;
        for os in ["windows", "osx"] {
            let url = self
                .content
                .join(&format!("products/{product}/os/{os}/builds"))
                .map_err(|error| ProviderError::Other(error.to_string()))?;
            let builds: Option<Builds> = self
                .json(net::client()?.get(url).query(&[("generation", "2")]))
                .await?;
            let Some(link) = builds
                .and_then(|builds| builds.items.into_iter().next())
                .map(|build| build.link)
            else {
                continue;
            };
            let (status, body) = net::send(net::client()?.get(&link), GOG).await?;
            if !status.is_success() {
                return Err(net::unexpected(GOG, status));
            }
            let manifest = inflate(&body);
            let manifest: Manifest =
                serde_json::from_slice(&manifest).map_err(|_| net::unreadable(GOG))?;
            if let (Some(id), Some(secret)) = (manifest.client_id, manifest.client_secret) {
                found = Some(Client { id, secret });
            }
            break;
        }
        self.clients
            .lock()
            .await
            .insert(product.into(), found.clone());
        Ok(found)
    }
}

/// A gen-2 manifest is zlib-compressed JSON; an older one is plain JSON.
fn inflate(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    match flate2::read::ZlibDecoder::new(bytes)
        .take(16 * 1024 * 1024)
        .read_to_end(&mut out)
    {
        Ok(_) => out,
        Err(_) => bytes.to_vec(),
    }
}

#[derive(Deserialize)]
struct Builds {
    #[serde(default)]
    items: Vec<Build>,
}
#[derive(Deserialize)]
struct Build {
    link: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
}
#[derive(Deserialize)]
struct Token {
    access_token: String,
}
#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    items: Vec<Item>,
    #[serde(default)]
    next_page_token: Option<String>,
}
#[derive(Deserialize)]
struct Item {
    achievement_key: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default = "visible")]
    visible: bool,
    #[serde(default)]
    image_url_unlocked: Option<String>,
    #[serde(default)]
    image_url_locked: Option<String>,
    #[serde(default)]
    date_unlocked: Option<String>,
    #[serde(default)]
    rarity: Option<f32>,
}
fn visible() -> bool {
    true
}

#[async_trait]
impl AchievementProvider for GogAchievements {
    fn id(&self) -> String {
        caskade_core::store_accounts::gog::ID.into()
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
            ExternalId::Gog { product_id } => Some(product_id.to_string()),
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
        let refresh = credentials
            .refresh_token
            .as_deref()
            .ok_or_else(|| ProviderError::Expired(caskade_core::tr!("not signed in")))?;
        let user = credentials
            .extra
            .get("user_id")
            .map(String::as_str)
            .unwrap_or(&account.external_id);
        let Some(client) = self.client(product).await? else {
            return Err(FetchError::Unsupported(caskade_core::tr!(
                "this game doesn't use GOG Galaxy's achievements"
            )));
        };
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled.into());
        }
        let token_url = self
            .auth
            .join("token")
            .map_err(|error| ProviderError::Other(error.to_string()))?;
        let token: Token = self
            .json(net::client()?.get(token_url).query(&[
                ("client_id", client.id.as_str()),
                ("client_secret", client.secret.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
                ("without_new_session", "1"),
            ]))
            .await?
            .ok_or_else(|| {
                ProviderError::Expired(caskade_core::tr!("GOG didn't renew the sign-in"))
            })?;
        let url = self
            .gameplay
            .join(&format!("clients/{}/users/{user}/achievements", client.id))
            .map_err(|error| ProviderError::Other(error.to_string()))?;
        let mut fetched = Fetched {
            complete: true,
            ..Default::default()
        };
        let mut page_token: Option<String> = None;
        for index in 0.. {
            if index == MAX_PAGES || cancel.is_cancelled() {
                fetched.complete = false;
                break;
            }
            let mut request = net::client()?
                .get(url.clone())
                .bearer_auth(&token.access_token);
            if let Some(next) = &page_token {
                request = request.query(&[("page_token", next.as_str())]);
            }
            let page: Page = match (self.json(request).await, index) {
                (Ok(Some(page)), _) => page,
                (Ok(None), 0) => {
                    return Err(FetchError::Unsupported(caskade_core::tr!(
                        "GOG keeps no achievements for this game"
                    )));
                }
                (Err(error), 0) => return Err(error.into()),
                // A later page that failed: what came stays, incomplete.
                _ => {
                    fetched.complete = false;
                    break;
                }
            };
            for item in page.items {
                let unlocked_at = item.date_unlocked.as_deref().and_then(net::iso_ms);
                fetched.states.insert(
                    item.achievement_key.clone(),
                    AchievementState {
                        unlocked: item.date_unlocked.is_some(),
                        unlocked_at_ms: unlocked_at,
                        ..Default::default()
                    },
                );
                fetched.definitions.push(AchievementDefinition {
                    name: item
                        .name
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| item.achievement_key.clone()),
                    description: item.description.filter(|d| !d.trim().is_empty()),
                    hidden: !item.visible,
                    icon_url: item.image_url_unlocked.filter(|url| !url.is_empty()),
                    locked_icon_url: item.image_url_locked.filter(|url| !url.is_empty()),
                    progress_max: None,
                    rarity_percent: item.rarity.map(|r| r.clamp(0.0, 100.0)),
                    points: None,
                    id: item.achievement_key,
                });
            }
            match page.next_page_token.filter(|next| !next.is_empty()) {
                Some(next) => page_token = Some(next),
                None => break,
            }
        }
        Ok(fetched)
    }
}

#[cfg(test)]
#[path = "gog_tests.rs"]
mod tests;
