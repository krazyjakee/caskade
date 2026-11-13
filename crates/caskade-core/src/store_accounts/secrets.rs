//! Store accounts' sign-ins (#165): in the OS keychain under their own
//! service, never RomM's, keyed by store account id. When the keychain
//! can't be used, a sign-in lasts only this session, in memory. Never in
//! the database, settings, IPC answers, logs or backups.

use super::StoreCredentials;
use crate::{
    Result,
    secrets::{MemorySecrets, SecretStore},
};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

/// The keychain service store accounts' entries are filed under.
pub const KEYCHAIN_SERVICE: &str = "dev.caskade.store-accounts";

/// Sign-ins, in `keychain` when it works and in memory for the session
/// when it doesn't.
#[derive(Clone)]
pub(super) struct Credentials {
    keychain: Arc<dyn SecretStore>,
    session: MemorySecrets,
    /// Accounts whose sign-in is only in memory.
    session_only: Arc<Mutex<BTreeSet<String>>>,
}

impl Credentials {
    pub fn new(keychain: Arc<dyn SecretStore>) -> Self {
        Self {
            keychain,
            session: MemorySecrets::default(),
            session_only: Arc::default(),
        }
    }

    pub fn session_only(&self, account: &str) -> bool {
        crate::lock(&self.session_only).contains(account)
    }

    pub async fn get(&self, account: &str) -> Option<StoreCredentials> {
        let text = if self.session_only(account) {
            self.session.get(account).await.ok().flatten()
        } else {
            match self.keychain.get(account).await {
                Ok(text) => text,
                Err(error) => {
                    tracing::warn!(%error, "store sign-in not read from the keychain");
                    self.session.get(account).await.ok().flatten()
                }
            }
        }?;
        serde_json::from_str(&text).ok()
    }

    pub async fn set(&self, account: &str, credentials: &StoreCredentials) -> Result<()> {
        let text = serde_json::to_string(credentials)?;
        match self.keychain.set(account, &text).await {
            Ok(()) => {
                crate::lock(&self.session_only).remove(account);
                self.session.delete(account).await
            }
            Err(error) => {
                tracing::warn!(%error, "no keychain: the store sign-in lasts this session only");
                crate::lock(&self.session_only).insert(account.to_owned());
                self.session.set(account, &text).await
            }
        }
    }

    pub async fn delete(&self, account: &str) -> Result<()> {
        crate::lock(&self.session_only).remove(account);
        self.session.delete(account).await?;
        self.keychain.delete(account).await
    }
}
