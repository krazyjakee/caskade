//! The host's web credentials (STREAMING-PLAN §4.1): a random user name and
//! password, made once and kept in the OS keychain under
//! `dev.caskade.streaming`, never in the database, settings, logs, IPC or
//! backups.

use crate::{
    Error, Result,
    romm::secrets::{MemorySecrets, SecretStore},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The keychain service streaming's secrets are filed under.
pub const KEYCHAIN_SERVICE: &str = "dev.caskade.streaming";
/// The entry of Caskade's own host.
const HOST: &str = "host";
/// The entry of a user's own host Caskade was allowed to use (#258).
const EXISTING: &str = "existing";

/// A user name and password for the host's web API.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub user: String,
    pub password: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("user", &self.user)
            .field("password", &"…")
            .finish()
    }
}

impl Credentials {
    /// New random ones: 244 random bits of password.
    pub fn random() -> Self {
        let token = || uuid::Uuid::new_v4().simple().to_string();
        Self {
            user: format!("caskade-{}", &token()[..8]),
            password: format!("{}{}", token(), token()),
        }
    }
}

/// Where the host's credentials live.
#[derive(Clone)]
pub struct HostSecrets {
    store: Arc<dyn SecretStore>,
}

impl HostSecrets {
    /// The OS keychain, under [`KEYCHAIN_SERVICE`].
    pub fn keychain() -> Self {
        Self {
            store: Arc::new(Keychain),
        }
    }

    /// In memory (tests).
    pub fn memory() -> Self {
        Self {
            store: Arc::new(MemorySecrets::default()),
        }
    }

    pub async fn get(&self) -> Result<Option<Credentials>> {
        Ok(self
            .store
            .get(HOST)
            .await?
            .and_then(|text| serde_json::from_str(&text).ok()))
    }

    pub async fn set(&self, credentials: &Credentials) -> Result<()> {
        self.store
            .set(HOST, &serde_json::to_string(credentials)?)
            .await
    }

    /// The credentials of the user's own host Caskade uses.
    pub async fn existing(&self) -> Result<Option<Credentials>> {
        Ok(self
            .store
            .get(EXISTING)
            .await?
            .and_then(|text| serde_json::from_str(&text).ok()))
    }

    pub async fn set_existing(&self, credentials: &Credentials) -> Result<()> {
        self.store
            .set(EXISTING, &serde_json::to_string(credentials)?)
            .await
    }
}

/// The OS keychain, on a blocking thread (the platform APIs may wait on
/// D-Bus or a system prompt).
struct Keychain;

fn keychain(error: keyring::Error) -> Error {
    Error::Invalid(crate::tr!("system keychain: {error}", error = error))
}

async fn on_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| Error::Io(std::io::Error::other(error)))?
}

#[async_trait::async_trait]
impl SecretStore for Keychain {
    async fn get(&self, account: &str) -> Result<Option<String>> {
        let account = account.to_owned();
        on_thread(move || {
            let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &account).map_err(keychain)?;
            match entry.get_password() {
                Ok(secret) => Ok(Some(secret)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(error) => Err(keychain(error)),
            }
        })
        .await
    }

    async fn set(&self, account: &str, secret: &str) -> Result<()> {
        let (account, secret) = (account.to_owned(), secret.to_owned());
        on_thread(move || {
            keyring::Entry::new(KEYCHAIN_SERVICE, &account)
                .and_then(|entry| entry.set_password(&secret))
                .map_err(keychain)
        })
        .await
    }

    async fn delete(&self, account: &str) -> Result<()> {
        let account = account.to_owned();
        on_thread(move || {
            let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &account).map_err(keychain)?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(keychain(error)),
            }
        })
        .await
    }
}
