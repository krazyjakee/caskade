//! Secrets kept out of SQLite and settings (RomM credentials, store
//! sign-ins, streaming hosts' passwords): in the OS keychain (Secret Service
//! on Linux, Keychain on macOS, Credential Manager on Windows) through
//! [`Keychain`], or in memory for tests.

use crate::{Error, Result};
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

/// Where secrets live, keyed by an id (a server, account or host).
#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>>;
    async fn set(&self, key: &str, secret: &str) -> Result<()>;
    /// Removing a missing secret is not an error.
    async fn delete(&self, key: &str) -> Result<()>;
}

/// The OS keychain, with entries filed under `service`. Calls run on a
/// blocking thread: the platform APIs are synchronous and may wait on a
/// D-Bus or system prompt.
#[derive(Debug, Clone, Copy)]
pub struct Keychain {
    service: &'static str,
}

impl Keychain {
    pub const fn new(service: &'static str) -> Self {
        Self { service }
    }

    async fn on_thread<T: Send + 'static>(
        self,
        key: &str,
        work: impl FnOnce(keyring::Entry) -> keyring::Result<T> + Send + 'static,
    ) -> Result<T> {
        let key = key.to_owned();
        tokio::task::spawn_blocking(move || {
            keyring::Entry::new(self.service, &key)
                .and_then(work)
                .map_err(|error| {
                    Error::Invalid(crate::tr!("system keychain: {error}", error = error))
                })
        })
        .await?
    }
}

#[async_trait]
impl SecretStore for Keychain {
    async fn get(&self, key: &str) -> Result<Option<String>> {
        self.on_thread(key, |entry| match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error),
        })
        .await
    }

    async fn set(&self, key: &str, secret: &str) -> Result<()> {
        let secret = secret.to_owned();
        self.on_thread(key, move |entry| entry.set_password(&secret))
            .await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.on_thread(key, |entry| match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error),
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
    async fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.map().get(key).cloned())
    }

    async fn set(&self, key: &str, secret: &str) -> Result<()> {
        self.map().insert(key.to_owned(), secret.to_owned());
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.map().remove(key);
        Ok(())
    }
}
