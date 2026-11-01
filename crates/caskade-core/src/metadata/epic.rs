//! Epic's store offers for library games. IGDB knows an Epic game by the
//! offer the store sells it as, which neither the launcher nor Epic's
//! library names: they give the game's namespace, catalogue item and app.
//! Each namespace's base-game offers come from Epic's public catalogue,
//! asked as the launcher's client without a sign-in, and are kept for the
//! session.

use crate::store_accounts::{
    ProviderError,
    epic::{ACCOUNT, CATALOG, CLIENT_ID, CLIENT_SECRET},
    http,
};
use serde::Deserialize;
use std::{
    collections::{BTreeSet, HashMap},
    sync::Mutex,
    time::{Duration, Instant},
};
use url::Url;

const EPIC: &str = "Epic Games";
/// The category of the offer that sells a game itself, not an edition,
/// add-on or test build.
const BASE_GAME: &str = "games/edition/base";
/// Offers asked for per namespace; a namespace has one base game or a few.
const COUNT: &str = "100";
/// How long before its expiry a token is renewed.
const TOKEN_MARGIN: Duration = Duration::from_secs(60);

pub(super) struct EpicOffers {
    account: String,
    catalog: String,
    token: tokio::sync::Mutex<Option<(String, Instant)>>,
    /// Each namespace's base-game offers; none for a namespace Epic
    /// doesn't know.
    known: Mutex<HashMap<String, Vec<String>>>,
}

impl Default for EpicOffers {
    fn default() -> Self {
        Self::with_base_urls(ACCOUNT, CATALOG)
    }
}

impl EpicOffers {
    pub(super) fn with_base_urls(account: &str, catalog: &str) -> Self {
        Self {
            account: account.to_owned(),
            catalog: catalog.to_owned(),
            token: tokio::sync::Mutex::default(),
            known: Mutex::default(),
        }
    }

    /// The base-game offers of each of `namespaces` that Epic answered
    /// for. One left out couldn't be asked about: after the first failure
    /// the rest wait for the next run.
    pub(super) async fn base_games(
        &self,
        namespaces: &BTreeSet<String>,
    ) -> HashMap<String, Vec<String>> {
        let mut found = HashMap::new();
        let mut ask = Vec::new();
        {
            let known = self.known.lock().unwrap_or_else(|error| error.into_inner());
            for namespace in namespaces {
                match known.get(namespace) {
                    Some(offers) => {
                        found.insert(namespace.clone(), offers.clone());
                    }
                    None => ask.push(namespace),
                }
            }
        }
        for namespace in ask {
            match self.offers(namespace).await {
                Ok(offers) => {
                    self.known
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .insert(namespace.clone(), offers.clone());
                    found.insert(namespace.clone(), offers);
                }
                Err(error) => {
                    tracing::info!(%error, namespace, "Epic's store offers couldn't be read");
                    break;
                }
            }
        }
        found
    }

    async fn offers(&self, namespace: &str) -> Result<Vec<String>, ProviderError> {
        let token = self.token().await?;
        let mut url = Url::parse(&self.catalog)
            .map_err(|_| ProviderError::Other(crate::tr!("bad Epic Games address")))?;
        url.path_segments_mut()
            .map_err(|()| ProviderError::Other(crate::tr!("bad Epic Games address")))?
            .pop_if_empty()
            .extend(["catalog", "api", "shared", "namespace", namespace, "offers"]);
        url.query_pairs_mut()
            .append_pair("category", BASE_GAME)
            .append_pair("count", COUNT);
        let request = http::client()?.get(url).bearer_auth(token);
        let page: Option<Page> = match http::json_if_found(request, EPIC).await {
            Err(ProviderError::Expired(error)) => {
                *self.token.lock().await = None;
                return Err(ProviderError::Expired(error));
            }
            answer => answer?,
        };
        // None for private sandboxes and namespaces Epic retired.
        Ok(page
            .map(|page| page.elements)
            .unwrap_or_default()
            .into_iter()
            .filter(|offer| offer.categories.iter().any(|path| path.path == BASE_GAME))
            .map(|offer| offer.id)
            .collect())
    }

    /// A client token, renewed shortly before it expires.
    async fn token(&self) -> Result<String, ProviderError> {
        let mut token = self.token.lock().await;
        if let Some((access, expires)) = token.as_ref()
            && Instant::now() + TOKEN_MARGIN < *expires
        {
            return Ok(access.clone());
        }
        let url = format!(
            "{}/account/api/oauth/token",
            self.account.trim_end_matches('/')
        );
        let request = http::client()?
            .post(url)
            .basic_auth(CLIENT_ID, Some(CLIENT_SECRET))
            .form(&[("grant_type", "client_credentials")]);
        let answer: Token = http::json(request, EPIC).await?;
        let expires = Instant::now() + Duration::from_secs(answer.expires_in);
        *token = Some((answer.access_token.clone(), expires));
        Ok(answer.access_token)
    }
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    elements: Vec<Offer>,
}

#[derive(Deserialize)]
struct Offer {
    id: String,
    #[serde(default)]
    categories: Vec<Category>,
}

#[derive(Deserialize)]
struct Category {
    path: String,
}
