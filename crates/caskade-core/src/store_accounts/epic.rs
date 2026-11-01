//! Epic Games Store accounts (#168), through the endpoints the Epic Games
//! Launcher uses, as Legendary and Heroic document and use them.
//!
//! - Sign-in: Epic's login page, sent on to its redirect API with the
//!   launcher's client id, shows a small JSON answer holding an
//!   `authorizationCode`. The user pastes it (or the whole answer), and the
//!   account service's OAuth token endpoint exchanges it for an access
//!   token (two hours) and a refresh token, which renews it. The token
//!   names the account (`account_id`, `displayName`).
//! - Library: the library service's items, every page by cursor. Its
//!   records carry no titles: the catalogue's bulk items, per namespace,
//!   give each its title, categories and, for an add-on, its main game.
//!   Imported: public items in the `games` category that aren't an add-on
//!   of another game. Not imported: DLC and add-ons, Unreal Engine
//!   marketplace items (namespace `ue`), private sandboxes and items the
//!   catalogue doesn't know. Claimed free games are in the library like
//!   bought ones. An entitlement that ends (a time-limited one) leaves the
//!   library, and a complete refresh then marks it lost.
//! - Ids are `ExternalId::Epic { namespace, catalog_item, app_name }`, as
//!   Epic launcher, Heroic and Legendary discovery record them; owned games
//!   match local copies on namespace and catalog item, so two editions with
//!   one title stay two games.
//! - Play time: the library service's play time per app (seconds, as
//!   minutes); its failure leaves play time unknown, not the library.
//!   Epic doesn't report a last play.
//! - Any library or catalogue page that fails fails the refresh: the
//!   library already known stays.

use super::{
    AccountProvider, AuthChallenge, AuthInput, AuthKind, CancellationToken, LibrarySnapshot,
    OwnedProduct, ProviderCapabilities, ProviderError, SignedIn, StoreAccount, StoreCredentials,
    http,
};
use async_trait::async_trait;
use caskade_discovery::ExternalId;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use url::Url;

pub const ID: &str = "epic";
const EPIC: &str = "Epic Games";
/// The Epic Games Launcher's client, which Legendary and Heroic sign in as.
pub(crate) const CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
pub(crate) const CLIENT_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";
const LOGIN: &str = "https://www.epicgames.com/id/login";
pub(crate) const ACCOUNT: &str = "https://account-public-service-prod03.ol.epicgames.com";
const LIBRARY: &str = "https://library-service.live.use1a.on.epicgames.com";
pub(crate) const CATALOG: &str = "https://catalog-public-service-prod06.ol.epicgames.com";
const MAX_PAGES: u32 = 1000;
/// Catalogue items asked for at once.
const BATCH: usize = 50;

#[derive(Debug, Clone)]
pub struct EpicProvider {
    account: Url,
    library: Url,
    catalog: Url,
}

impl Default for EpicProvider {
    fn default() -> Self {
        let url = |text: &str| Url::parse(text).expect("an Epic URL parses");
        Self {
            account: url(ACCOUNT),
            library: url(LIBRARY),
            catalog: url(CATALOG),
        }
    }
}

impl EpicProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Other roots for the account, library and catalogue services (tests).
    pub fn with_bases(account: Url, library: Url, catalog: Url) -> Self {
        Self {
            account,
            library,
            catalog,
        }
    }

    fn url(base: &Url, path: &str) -> Result<Url, ProviderError> {
        base.join(path)
            .map_err(|_| ProviderError::Other(crate::tr!("bad Epic Games address")))
    }

    async fn token(
        &self,
        form: &[(&str, &str)],
    ) -> Result<(Token, StoreCredentials), ProviderError> {
        let request = http::client()?
            .post(Self::url(&self.account, "account/api/oauth/token")?)
            .basic_auth(CLIENT_ID, Some(CLIENT_SECRET))
            .form(&[form, &[("token_type", "eg1")]].concat());
        let token: Token = http::json(request, EPIC).await?;
        let credentials = StoreCredentials {
            access_token: token.access_token.clone(),
            refresh_token: Some(token.refresh_token.clone()),
            expires_at_ms: Some(
                crate::now_ms() + i64::try_from(token.expires_in.saturating_mul(1000)).unwrap_or(0),
            ),
            extra: BTreeMap::from([("account_id".to_owned(), token.account_id.clone())]),
        };
        Ok((token, credentials))
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        url: Url,
        token: &str,
    ) -> Result<T, ProviderError> {
        http::json(http::client()?.get(url).bearer_auth(token), EPIC).await
    }
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    account_id: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Items {
    #[serde(default)]
    records: Vec<Record>,
    response_metadata: Option<Metadata>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    next_cursor: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Record {
    namespace: String,
    catalog_item_id: String,
    #[serde(default)]
    app_name: String,
    #[serde(default)]
    sandbox_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogItem {
    title: Option<String>,
    #[serde(default)]
    categories: Vec<Category>,
    main_game_item: Option<serde_json::Value>,
    #[serde(default)]
    release_info: Vec<ReleaseInfo>,
}

#[derive(Deserialize)]
struct Category {
    path: String,
}

#[derive(Deserialize)]
struct ReleaseInfo {
    #[serde(default)]
    platform: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Playtime {
    artifact_id: String,
    total_time: u64,
}

/// The authorization code the login page showed: the code, or its JSON.
fn code(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if input.starts_with('{') {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Shown {
            authorization_code: Option<String>,
        }
        return serde_json::from_str::<Shown>(input)
            .ok()?
            .authorization_code
            .filter(|code| !code.is_empty());
    }
    Some(input.trim_matches('"').to_owned())
}

/// Whether a record and its catalogue item are a game to import.
fn importable(record: &Record, item: &CatalogItem) -> bool {
    record.namespace != "ue"
        && record
            .sandbox_type
            .as_deref()
            .is_none_or(|sandbox| sandbox == "PUBLIC")
        && item.main_game_item.is_none()
        && item.categories.iter().any(|c| c.path == "games")
        && !item
            .categories
            .iter()
            .any(|c| c.path == "addons" || c.path == "digitalextras")
}

fn systems(item: &CatalogItem) -> Vec<String> {
    let mut systems = Vec::new();
    for platform in item.release_info.iter().flat_map(|info| &info.platform) {
        let system = match platform.as_str() {
            "Windows" | "Win32" => "windows",
            "Mac" => "mac",
            "Linux" => "linux",
            _ => continue,
        };
        if !systems.iter().any(|s| s == system) {
            systems.push(system.to_owned());
        }
    }
    systems
}

#[async_trait]
impl AccountProvider for EpicProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: ID.into(),
            name: EPIC.into(),
            auth: AuthKind::Browser,
            library: true,
            playtime: true,
            install: true,
        }
    }

    fn install_target(&self, product: &OwnedProduct) -> Option<super::InstallTarget> {
        let ExternalId::Epic {
            namespace,
            catalog_item,
            app_name,
        } = product.ids.first()?
        else {
            return None;
        };
        let encode = |part: &str| {
            percent_encoding::utf8_percent_encode(part, percent_encoding::NON_ALPHANUMERIC)
                .to_string()
        };
        Some(super::InstallTarget {
            launcher: "epic".into(),
            name: "Epic Games Launcher".into(),
            uri: format!(
                "com.epicgames.launcher://apps/{}%3A{}%3A{}?action=install",
                encode(namespace),
                encode(catalog_item),
                encode(app_name)
            ),
        })
    }

    async fn begin_auth(&self, _flow_id: &str) -> Result<AuthChallenge, ProviderError> {
        let mut redirect = Url::parse("https://www.epicgames.com/id/api/redirect")
            .expect("Epic's redirect URL parses");
        redirect
            .query_pairs_mut()
            .append_pair("clientId", CLIENT_ID)
            .append_pair("responseType", "code");
        let mut url = Url::parse(LOGIN).expect("Epic's login URL parses");
        url.query_pairs_mut()
            .append_pair("redirectUrl", redirect.as_str());
        Ok(AuthChallenge {
            url: Some(url.into()),
            user_code: None,
            fields: Vec::new(),
            instructions: Some(crate::tr!(
                "Sign in on Epic Games' page. It then shows a short text with an “authorizationCode”: copy the code (or the whole text) and paste it below."
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
            ProviderError::Rejected(crate::tr!("paste the authorization code Epic Games showed"))
        })?;
        let (token, credentials) = self
            .token(&[
                ("grant_type", "authorization_code"),
                ("code", code.as_str()),
            ])
            .await
            .map_err(|error| match error {
                ProviderError::Expired(_) | ProviderError::Other(_) => ProviderError::Rejected(
                    crate::tr!("Epic Games didn't accept that code; sign in again for a new one"),
                ),
                other => other,
            })?;
        Ok(SignedIn {
            display_name: token
                .display_name
                .unwrap_or_else(|| token.account_id.clone()),
            external_id: token.account_id,
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
        self.token(&[("grant_type", "refresh_token"), ("refresh_token", refresh)])
            .await
            .map(|(_, renewed)| renewed)
            .map_err(|error| match error {
                ProviderError::Other(why) => ProviderError::Expired(why),
                other => other,
            })
    }

    async fn library(
        &self,
        account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        cancel: &CancellationToken,
    ) -> Result<LibrarySnapshot, ProviderError> {
        let credentials =
            credentials.ok_or_else(|| ProviderError::Expired(crate::tr!("not signed in")))?;
        let token = &credentials.access_token;
        // Every library record, page by page.
        let mut records: Vec<Record> = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            let mut url = Self::url(&self.library, "library/api/public/items")?;
            url.query_pairs_mut().append_pair("includeMetadata", "true");
            if let Some(cursor) = &cursor {
                url.query_pairs_mut().append_pair("cursor", cursor);
            }
            let page: Items = self.get(url, token).await?;
            records.extend(page.records);
            match page.response_metadata.and_then(|m| m.next_cursor) {
                Some(next) if Some(&next) != cursor.as_ref() && !next.is_empty() => {
                    cursor = Some(next);
                }
                _ => break,
            }
        }
        // Titles and kinds from the catalogue, per namespace.
        let mut by_namespace: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for record in &records {
            if record.namespace != "ue" {
                by_namespace
                    .entry(record.namespace.clone())
                    .or_default()
                    .push(record.catalog_item_id.clone());
            }
        }
        let mut catalog: HashMap<(String, String), CatalogItem> = HashMap::new();
        for (namespace, ids) in by_namespace {
            for batch in ids.chunks(BATCH) {
                if cancel.is_cancelled() {
                    return Err(ProviderError::Cancelled);
                }
                let mut url = Self::url(
                    &self.catalog,
                    &format!("catalog/api/shared/namespace/{namespace}/bulk/items"),
                )?;
                {
                    let mut query = url.query_pairs_mut();
                    for id in batch {
                        query.append_pair("id", id);
                    }
                    query
                        .append_pair("includeDLCDetails", "true")
                        .append_pair("includeMainGameDetails", "true")
                        .append_pair("country", "US")
                        .append_pair("locale", "en");
                }
                let items: HashMap<String, CatalogItem> = self.get(url, token).await?;
                for (id, item) in items {
                    catalog.insert((namespace.clone(), id), item);
                }
            }
        }
        // Play time, when Epic gives it.
        let account_id = credentials
            .extra
            .get("account_id")
            .cloned()
            .unwrap_or_else(|| account.external_id.clone());
        let playtime: HashMap<String, u64> = match Self::url(
            &self.library,
            &format!("library/api/public/playtime/account/{account_id}/all"),
        ) {
            Ok(url) => match self.get::<Vec<Playtime>>(url, token).await {
                Ok(times) => times
                    .into_iter()
                    .map(|time| (time.artifact_id, time.total_time / 60))
                    .collect(),
                Err(error) => {
                    tracing::info!(%error, "Epic play time unavailable");
                    HashMap::new()
                }
            },
            Err(_) => HashMap::new(),
        };
        let products = records
            .into_iter()
            .filter_map(|record| {
                let item =
                    catalog.get(&(record.namespace.clone(), record.catalog_item_id.clone()))?;
                importable(&record, item).then(|| OwnedProduct {
                    product_id: format!("{}:{}", record.namespace, record.catalog_item_id),
                    title: item
                        .title
                        .clone()
                        .filter(|title| !title.trim().is_empty())
                        .unwrap_or_else(|| record.app_name.clone()),
                    systems: systems(item),
                    playtime_minutes: playtime.get(&record.app_name).copied(),
                    last_played_ms: None,
                    platforms: Vec::new(),
                    ids: vec![ExternalId::Epic {
                        namespace: record.namespace,
                        catalog_item: record.catalog_item_id,
                        app_name: record.app_name,
                    }],
                })
            })
            .collect();
        Ok(LibrarySnapshot {
            products,
            complete: true,
        })
    }
}

#[cfg(test)]
#[path = "epic_tests.rs"]
mod tests;
