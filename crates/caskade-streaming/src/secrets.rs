//! The host's web credentials: a random user name and
//! password, made once and kept in the OS keychain under
//! `dev.caskade.streaming`, never in the database, settings, logs, IPC or
//! backups.

use caskade_core::{
    Result,
    secrets::{Keychain, MemorySecrets, SecretStore},
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
            store: Arc::new(Keychain::new(KEYCHAIN_SERVICE)),
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

    /// Drops Caskade's own credentials, as a keychain that lost them
    /// would (tests).
    #[cfg(test)]
    pub async fn forget(&self) -> Result<()> {
        self.store.delete(HOST).await
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
