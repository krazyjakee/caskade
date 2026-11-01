//! GOG accounts (#167), through GOG's own sign-in and library endpoints, as
//! GOG Galaxy and other launchers (Heroic, Lutris, Playnite) use them.
//!
//! - Sign-in: GOG's login page (`auth.gog.com/auth`, the Galaxy client id)
//!   ends on a page whose address carries `code=…`; the user pastes that
//!   address (or the code), and `auth.gog.com/token` exchanges it for an
//!   access token (an hour) and a refresh token, which renews it.
//!   `embed.gog.com/userData.json` names the account; its `userId` is the
//!   ownership namespace.
//! - Library: `embed.gog.com/account/getFilteredProducts?mediaType=1`, every
//!   page. Only games (`isGame`) are imported: movies aren't, and DLC and
//!   extras (goodies) aren't listed as products of their own. Each game's
//!   id is `ExternalId::Gog { product_id }`, as GOG Galaxy, Heroic,
//!   Minigalaxy and Lutris discovery and managed installs of GOG offline
//!   installers (`goggame-*.info`) record it, so every copy is the same
//!   game. `worksOn` is kept as the systems it runs on; no install location
//!   is invented.
//! - A page that fails, is cancelled or is cut short fails the whole
//!   refresh: the library already known stays.
//! - GOG gives no play time here: capabilities say so.

use super::{
    AccountProvider, AuthChallenge, AuthInput, AuthKind, CancellationToken, LibrarySnapshot,
    OwnedProduct, ProviderCapabilities, ProviderError, SignedIn, StoreAccount, StoreCredentials,
    http,
};
use async_trait::async_trait;
use caskade_discovery::ExternalId;
use serde::Deserialize;
use url::Url;

pub const ID: &str = "gog";
const GOG: &str = "GOG";
/// GOG Galaxy's client: the one GOG's login page and token endpoint accept
/// for launchers.
const CLIENT_ID: &str = "46899977096215655";
const CLIENT_SECRET: &str = "9d85c43b1482497dbbce61f6e4aa173a433796eeae2ca8c5f6129f2dc4de46d9";
const REDIRECT: &str = "https://embed.gog.com/on_login_success?origin=client";
const AUTH: &str = "https://auth.gog.com";
const EMBED: &str = "https://embed.gog.com";
/// At most this many pages: GOG's largest libraries are far below.
const MAX_PAGES: u32 = 500;

#[derive(Debug, Clone)]
pub struct GogProvider {
    auth: Url,
    embed: Url,
}

impl Default for GogProvider {
    fn default() -> Self {
        Self {
            auth: Url::parse(AUTH).expect("the GOG auth URL parses"),
            embed: Url::parse(EMBED).expect("the GOG embed URL parses"),
        }
    }
}

impl GogProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Other roots for the auth and embed hosts (tests).
    pub fn with_bases(auth: Url, embed: Url) -> Self {
        Self { auth, embed }
    }

    fn url(base: &Url, path: &str) -> Result<Url, ProviderError> {
        base.join(path)
            .map_err(|_| ProviderError::Other(crate::tr!("bad GOG address")))
    }

    async fn token(&self, query: &[(&str, &str)]) -> Result<StoreCredentials, ProviderError> {
        let request = http::client()?
            .get(Self::url(&self.auth, "token")?)
            .query(&[("client_id", CLIENT_ID), ("client_secret", CLIENT_SECRET)])
            .query(query);
        let token: Token = http::json(request, GOG).await?;
        Ok(StoreCredentials {
            access_token: token.access_token,
            refresh_token: Some(token.refresh_token),
            expires_at_ms: Some(
                crate::now_ms() + i64::try_from(token.expires_in.saturating_mul(1000)).unwrap_or(0),
            ),
            extra: token
                .user_id
                .map(|id| [("user_id".to_owned(), id)].into_iter().collect())
                .unwrap_or_default(),
        })
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        token: &str,
        query: &[(&str, String)],
    ) -> Result<T, ProviderError> {
        let request = http::client()?
            .get(Self::url(&self.embed, path)?)
            .bearer_auth(token)
            .query(query);
        http::json(request, GOG).await
    }
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    user_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserData {
    #[serde(default)]
    is_logged_in: bool,
    user_id: Option<String>,
    username: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    total_pages: u32,
    #[serde(default)]
    products: Vec<Product>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Product {
    id: u64,
    title: Option<String>,
    #[serde(default = "yes")]
    is_game: bool,
    #[serde(default)]
    works_on: WorksOn,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize, Default)]
struct WorksOn {
    #[serde(rename = "Windows", default)]
    windows: bool,
    #[serde(rename = "Mac", default)]
    mac: bool,
    #[serde(rename = "Linux", default)]
    linux: bool,
}

/// The code a pasted address carries, or the code itself.
fn code(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    match Url::parse(input) {
        Ok(url) => url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, code)| code.into_owned()),
        Err(_) => Some(input.to_owned()),
    }
}

#[async_trait]
impl AccountProvider for GogProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: ID.into(),
            name: GOG.into(),
            auth: AuthKind::Browser,
            library: true,
            playtime: false,
            install: true,
        }
    }

    fn install_target(&self, product: &OwnedProduct) -> Option<super::InstallTarget> {
        Some(super::InstallTarget {
            launcher: "gog".into(),
            name: "GOG Galaxy".into(),
            uri: format!("goggalaxy://openGameView/{}", product.product_id),
        })
    }

    async fn begin_auth(&self, _flow_id: &str) -> Result<AuthChallenge, ProviderError> {
        let mut url = Self::url(&self.auth, "auth")?;
        url.query_pairs_mut()
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", REDIRECT)
            .append_pair("response_type", "code")
            .append_pair("layout", "client2");
        Ok(AuthChallenge {
            url: Some(url.into()),
            user_code: None,
            fields: Vec::new(),
            instructions: Some(crate::tr!(
                "Sign in on GOG's page. It ends on an almost empty page: copy that page's address (it contains “code=”) and paste it below."
            )),
        })
    }

    async fn complete_auth(
        &self,
        _flow_id: &str,
        input: &AuthInput,
    ) -> Result<SignedIn, ProviderError> {
        let code = match input {
            AuthInput::Code { code: pasted } => code(pasted),
            AuthInput::Callback { params } => params.get("code").cloned(),
            _ => None,
        }
        .ok_or_else(|| {
            ProviderError::Rejected(crate::tr!(
                "paste the address GOG's page ended on, or the code in it"
            ))
        })?;
        let credentials = self
            .token(&[
                ("grant_type", "authorization_code"),
                ("code", code.as_str()),
                ("redirect_uri", REDIRECT),
            ])
            .await
            .map_err(|error| match error {
                ProviderError::Expired(_) | ProviderError::Other(_) => ProviderError::Rejected(
                    crate::tr!("GOG didn't accept that code; sign in again for a new one"),
                ),
                other => other,
            })?;
        let user: UserData = self
            .get("userData.json", &credentials.access_token, &[])
            .await?;
        let user_id = user
            .user_id
            .or_else(|| credentials.extra.get("user_id").cloned())
            .filter(|_| user.is_logged_in)
            .ok_or_else(|| ProviderError::Rejected(crate::tr!("GOG didn't sign the account in")))?;
        Ok(SignedIn {
            display_name: user.username.unwrap_or_else(|| user_id.clone()),
            external_id: user_id,
            credentials: Some(credentials),
        })
    }

    async fn refresh_credentials(
        &self,
        credentials: &StoreCredentials,
    ) -> Result<StoreCredentials, ProviderError> {
        let refresh = credentials
            .refresh_token
            .as_deref()
            .ok_or_else(|| ProviderError::Expired(crate::tr!("it can't be renewed")))?;
        let mut renewed = self
            .token(&[("grant_type", "refresh_token"), ("refresh_token", refresh)])
            .await
            .map_err(|error| match error {
                // A refused refresh token: sign in again.
                ProviderError::Other(why) => ProviderError::Expired(why),
                other => other,
            })?;
        for (key, value) in &credentials.extra {
            renewed
                .extra
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        Ok(renewed)
    }

    async fn library(
        &self,
        _account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        cancel: &CancellationToken,
    ) -> Result<LibrarySnapshot, ProviderError> {
        let token = &credentials
            .ok_or_else(|| ProviderError::Expired(crate::tr!("not signed in")))?
            .access_token;
        let mut products = Vec::new();
        let mut page = 1;
        loop {
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            let answer: Page = self
                .get(
                    "account/getFilteredProducts",
                    token,
                    &[("mediaType", "1".into()), ("page", page.to_string())],
                )
                .await?;
            products.extend(answer.products.into_iter().filter(|p| p.is_game).map(|p| {
                let systems = [
                    (p.works_on.windows, "windows"),
                    (p.works_on.mac, "mac"),
                    (p.works_on.linux, "linux"),
                ]
                .into_iter()
                .filter(|(works, _)| *works)
                .map(|(_, system)| system.to_owned())
                .collect();
                OwnedProduct {
                    product_id: p.id.to_string(),
                    title: p
                        .title
                        .filter(|title| !title.trim().is_empty())
                        .unwrap_or_else(|| format!("GOG {}", p.id)),
                    ids: vec![ExternalId::Gog { product_id: p.id }],
                    platforms: Vec::new(),
                    playtime_minutes: None,
                    last_played_ms: None,
                    systems,
                }
            }));
            if page >= answer.total_pages.min(MAX_PAGES) {
                break;
            }
            page += 1;
        }
        Ok(LibrarySnapshot {
            products,
            complete: true,
        })
    }
}

#[cfg(test)]
#[path = "gog_tests.rs"]
mod tests;
