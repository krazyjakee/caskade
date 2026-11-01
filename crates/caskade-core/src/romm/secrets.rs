//! RomM passwords and API tokens, kept out of SQLite and settings: in the OS
//! keychain (Secret Service on Linux, Keychain on macOS, Credential Manager
//! on Windows) through [`KeychainSecrets`], or in memory for tests.

use crate::{Error, Result};
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

/// The keychain service name entries are filed under.
pub const KEYCHAIN_SERVICE: &str = "dev.caskade.romm";

/// Where server credentials live, keyed by server id.
#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn get(&self, server: &str) -> Result<Option<String>>;
    async fn set(&self, server: &str, secret: &str) -> Result<()>;
    /// Removing a missing secret is not an error.
    async fn delete(&self, server: &str) -> Result<()>;
}

/// The OS keychain. Calls run on a blocking thread: the platform APIs are
/// synchronous and may wait on a D-Bus or system prompt.
#[derive(Debug, Clone, Default)]
pub struct KeychainSecrets;

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

#[async_trait]
impl SecretStore for KeychainSecrets {
    async fn get(&self, server: &str) -> Result<Option<String>> {
        let server = server.to_owned();
        on_thread(move || {
            let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &server).map_err(keychain)?;
            match entry.get_password() {
                Ok(secret) => Ok(Some(secret)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(error) => Err(keychain(error)),
            }
        })
        .await
    }

    async fn set(&self, server: &str, secret: &str) -> Result<()> {
        let (server, secret) = (server.to_owned(), secret.to_owned());
        on_thread(move || {
            keyring::Entry::new(KEYCHAIN_SERVICE, &server)
                .and_then(|entry| entry.set_password(&secret))
                .map_err(keychain)
        })
        .await
    }

    async fn delete(&self, server: &str) -> Result<()> {
        let server = server.to_owned();
        on_thread(move || {
            let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &server).map_err(keychain)?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(keychain(error)),
            }
        })
        .await
    }
}

/// Secrets in memory (tests, or a session without a keychain).
#[derive(Debug, Clone, Default)]
pub struct MemorySecrets(Arc<Mutex<BTreeMap<String, String>>>);

impl MemorySecrets {
    fn map(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
        crate::lock(&self.0)
    }
}

#[async_trait]
impl SecretStore for MemorySecrets {
    async fn get(&self, server: &str) -> Result<Option<String>> {
        Ok(self.map().get(server).cloned())
    }

    async fn set(&self, server: &str, secret: &str) -> Result<()> {
        self.map().insert(server.to_owned(), secret.to_owned());
        Ok(())
    }

    async fn delete(&self, server: &str) -> Result<()> {
        self.map().remove(server);
        Ok(())
    }
}
