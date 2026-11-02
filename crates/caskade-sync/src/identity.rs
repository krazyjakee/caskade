//! This device's identity: an ed25519 key whose public half is its endpoint
//! id, the name every paired device knows it by.
//!
//! The secret half lives in the OS keychain under [`KEYCHAIN_SERVICE`]
//! (beside `dev.caskade.store-accounts`), and nowhere else: never the
//! database, settings, logs, events, backups or the webview. [`Identity`] has
//! no `Serialize`, a `Debug` that prints only the public id, and hands the key
//! only to the endpoint builder inside this crate.
//!
//! ## Never a new identity without asking
//!
//! A new key is a new device to everything it was paired with: they refuse it
//! until it pairs again. So [`Identity::load`] creates one by itself only on
//! first use, when this device never had an identity (the caller says whether
//! it did). An entry that is missing or unreadable after that is
//! [`Loaded::NeedsConfirmation`]; the app asks the user and calls
//! [`Identity::create`] only when they agree.
//!
//! ## A keychain that never answers
//!
//! On Linux a locked keychain can block a read forever, waiting for an unlock
//! prompt nothing draws. Every keychain call runs on its own thread with a
//! deadline ([`KEYCHAIN_DEADLINE`]); a call that misses it is
//! [`Loaded::Unavailable`], and the thread is left behind rather than joined
//! (a join would be the hang). Not `spawn_blocking`: the runtime waits for
//! those at shutdown, which would turn a locked keychain into an app that
//! can't quit. Callers load the identity off the main thread, so the window
//! comes up regardless and only sync waits.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iroh::{EndpointId, SecretKey};
use tokio::sync::oneshot;

use crate::error::{Error, Result};
use crate::grant::hex;

/// The keychain service the device key is filed under.
pub const KEYCHAIN_SERVICE: &str = "dev.caskade.sync";

/// The entry under [`KEYCHAIN_SERVICE`].
const KEYCHAIN_ENTRY: &str = "device-key";

/// How long the keychain gets to answer one call: long enough for a person to
/// notice an unlock prompt and type into it.
pub const KEYCHAIN_DEADLINE: Duration = Duration::from_secs(20);

/// Where a device key is kept. [`Keychain`] in the app; [`MemoryKeyStore`]
/// in tests, which must never write a private key into the machine running
/// them.
pub trait KeyStore: Send + Sync + 'static {
    /// `Ok(None)` when there is no entry.
    fn get(&self) -> Result<Option<String>, Unavailable>;
    fn set(&self, value: &str) -> Result<(), Unavailable>;
}

/// The store didn't answer or refused. Carries no platform message: a store
/// may quote the value it refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unavailable;

/// The OS keychain, under [`KEYCHAIN_SERVICE`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Keychain;

impl KeyStore for Keychain {
    fn get(&self) -> Result<Option<String>, Unavailable> {
        let entry =
            keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ENTRY).map_err(|_| Unavailable)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(Unavailable),
        }
    }

    fn set(&self, value: &str) -> Result<(), Unavailable> {
        keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ENTRY)
            .and_then(|entry| entry.set_password(value))
            .map_err(|_| Unavailable)
    }
}

/// A key store in memory, for tests and tools. `refusing` makes every call
/// fail like a locked keychain.
#[derive(Debug, Default)]
pub struct MemoryKeyStore {
    value: Mutex<Option<String>>,
    refusing: bool,
}

impl MemoryKeyStore {
    pub fn holding(value: impl Into<String>) -> Self {
        Self {
            value: Mutex::new(Some(value.into())),
            refusing: false,
        }
    }

    pub fn refusing() -> Self {
        Self {
            value: Mutex::new(None),
            refusing: true,
        }
    }

    /// Whether anything is stored.
    pub fn is_empty(&self) -> bool {
        self.value
            .lock()
            .map(|value| value.is_none())
            .unwrap_or(true)
    }

    /// Forgets the stored value, as if the keychain entry were deleted.
    pub fn clear(&self) {
        if let Ok(mut value) = self.value.lock() {
            *value = None;
        }
    }
}

impl KeyStore for MemoryKeyStore {
    fn get(&self) -> Result<Option<String>, Unavailable> {
        if self.refusing {
            return Err(Unavailable);
        }
        self.value
            .lock()
            .map(|value| value.clone())
            .map_err(|_| Unavailable)
    }

    fn set(&self, value: &str) -> Result<(), Unavailable> {
        if self.refusing {
            return Err(Unavailable);
        }
        let mut stored = self.value.lock().map_err(|_| Unavailable)?;
        *stored = Some(value.to_owned());
        Ok(())
    }
}

/// What [`Identity::load`] found.
#[derive(Debug)]
pub enum Loaded {
    /// The stored identity, or a new one made on first use.
    Ready { identity: Identity, created: bool },
    /// The keychain entry is gone or unreadable, and this device had an
    /// identity before. Making a new one means paired devices must pair again,
    /// so the user is asked first; [`Identity::create`] makes it.
    NeedsConfirmation { reason: Lost },
    /// The keychain didn't answer in time or refused. Sync waits; nothing is
    /// created.
    Unavailable,
}

/// Why the stored identity can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lost {
    /// There is no keychain entry.
    Missing,
    /// The entry isn't a key.
    Unreadable,
}

/// This device's identity. Clone is cheap; iroh's `SecretKey` zeroes on drop.
#[derive(Clone)]
pub struct Identity {
    secret: Arc<SecretKey>,
}

impl Identity {
    /// Reads the device key from `store`.
    ///
    /// `known` is the endpoint id this device had last time, if it had one
    /// (the caller keeps it, public as it is). With no stored key and no
    /// `known` id this is first use and a key is made and stored; otherwise a
    /// missing or unreadable key is [`Loaded::NeedsConfirmation`].
    pub async fn load(store: Arc<dyn KeyStore>, known: Option<EndpointId>) -> Loaded {
        Self::load_within(store, known, KEYCHAIN_DEADLINE).await
    }

    pub(crate) async fn load_within(
        store: Arc<dyn KeyStore>,
        known: Option<EndpointId>,
        deadline: Duration,
    ) -> Loaded {
        let reader = store.clone();
        let stored = match within(deadline, move || reader.get()).await {
            Some(Ok(stored)) => stored,
            Some(Err(Unavailable)) | None => return Loaded::Unavailable,
        };
        let lost = match stored {
            Some(text) => match text.trim().parse::<SecretKey>() {
                Ok(secret) => {
                    return Loaded::Ready {
                        identity: Self {
                            secret: Arc::new(secret),
                        },
                        created: false,
                    };
                }
                Err(_) => Lost::Unreadable,
            },
            None => Lost::Missing,
        };
        if lost == Lost::Missing && known.is_none() {
            return match Self::create_within(store, deadline).await {
                Ok(identity) => Loaded::Ready {
                    identity,
                    created: true,
                },
                Err(_) => Loaded::Unavailable,
            };
        }
        Loaded::NeedsConfirmation { reason: lost }
    }

    /// Makes a new identity and stores it, replacing whatever was there.
    ///
    /// Only after the user agreed (see [`Loaded::NeedsConfirmation`]). Fails
    /// with [`Error::Keychain`] when it can't be stored: an identity that
    /// lasted only this session would be a new device on every start.
    pub async fn create(store: Arc<dyn KeyStore>) -> Result<Self> {
        Self::create_within(store, KEYCHAIN_DEADLINE).await
    }

    async fn create_within(store: Arc<dyn KeyStore>, deadline: Duration) -> Result<Self> {
        let secret = SecretKey::generate();
        let value = hex(&secret.to_bytes());
        match within(deadline, move || store.set(&value)).await {
            Some(Ok(())) => Ok(Self {
                secret: Arc::new(secret),
            }),
            _ => Err(Error::Keychain),
        }
    }

    /// An identity stored nowhere, for tests and throwaway endpoints.
    pub fn ephemeral() -> Self {
        Self {
            secret: Arc::new(SecretKey::generate()),
        }
    }

    /// The public key: this device's endpoint id and TLS identity.
    pub fn id(&self) -> EndpointId {
        self.secret.public()
    }

    /// For the endpoint builder, and nothing else.
    pub(crate) fn secret_key(&self) -> &SecretKey {
        &self.secret
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("id", &self.id())
            .finish_non_exhaustive()
    }
}

/// Runs `call` on a thread of its own and waits at most `deadline`. `None`
/// when it didn't answer; the thread is abandoned.
async fn within<T: Send + 'static>(
    deadline: Duration,
    call: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("caskade-sync-keychain".into())
        .spawn(move || {
            let _ = tx.send(call());
        });
    if spawned.is_err() {
        return None;
    }
    tokio::time::timeout(deadline, rx).await.ok()?.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store whose calls never return, like a locked Secret Service
    /// collection with no prompt to unlock it.
    struct Hung;

    impl KeyStore for Hung {
        fn get(&self) -> Result<Option<String>, Unavailable> {
            std::thread::sleep(Duration::from_secs(3600));
            Err(Unavailable)
        }
        fn set(&self, _: &str) -> Result<(), Unavailable> {
            std::thread::sleep(Duration::from_secs(3600));
            Err(Unavailable)
        }
    }

    fn ready(loaded: Loaded) -> (Identity, bool) {
        match loaded {
            Loaded::Ready { identity, created } => (identity, created),
            other => panic!("expected an identity, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn first_use_creates_an_identity_and_a_restart_reads_it_back() {
        let store = Arc::new(MemoryKeyStore::default());
        let (first, created) = ready(Identity::load(store.clone(), None).await);
        assert!(created);
        assert!(!store.is_empty());

        let (second, created) = ready(Identity::load(store.clone(), Some(first.id())).await);
        assert!(!created);
        assert_eq!(first.id(), second.id());
    }

    #[tokio::test]
    async fn a_missing_key_after_first_use_asks_before_making_a_new_one() {
        let store = Arc::new(MemoryKeyStore::default());
        let known = Identity::ephemeral().id();
        let loaded = Identity::load(store.clone(), Some(known)).await;
        assert!(matches!(
            loaded,
            Loaded::NeedsConfirmation {
                reason: Lost::Missing
            }
        ));
        assert!(store.is_empty(), "nothing is created without asking");

        let created = Identity::create(store.clone()).await.unwrap();
        let (again, _) = ready(Identity::load(store, Some(created.id())).await);
        assert_eq!(again.id(), created.id());
    }

    #[tokio::test]
    async fn an_unreadable_key_asks_even_on_first_use() {
        let store = Arc::new(MemoryKeyStore::holding("not a key"));
        let loaded = Identity::load(store.clone(), None).await;
        assert!(matches!(
            loaded,
            Loaded::NeedsConfirmation {
                reason: Lost::Unreadable
            }
        ));
        assert_eq!(store.get().unwrap().as_deref(), Some("not a key"));
    }

    #[tokio::test]
    async fn a_refusing_keychain_creates_nothing() {
        let store = Arc::new(MemoryKeyStore::refusing());
        assert!(matches!(
            Identity::load(store.clone(), None).await,
            Loaded::Unavailable
        ));
        assert!(matches!(
            Identity::create(store).await,
            Err(Error::Keychain)
        ));
    }

    #[tokio::test]
    async fn a_keychain_that_never_answers_is_unavailable_within_the_deadline() {
        let started = std::time::Instant::now();
        let loaded = Identity::load_within(Arc::new(Hung), None, Duration::from_millis(50)).await;
        assert!(matches!(loaded, Loaded::Unavailable));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn the_stored_form_is_iroh_s_own_and_survives_whitespace() {
        let store = Arc::new(MemoryKeyStore::default());
        let (identity, _) = ready(Identity::load(store.clone(), None).await);
        let stored = store.get().unwrap().unwrap();
        assert_eq!(stored.len(), 64);
        assert_eq!(stored.parse::<SecretKey>().unwrap().public(), identity.id());

        let padded = Arc::new(MemoryKeyStore::holding(format!("  {stored}\n")));
        let (read, created) = ready(Identity::load(padded, Some(identity.id())).await);
        assert!(!created);
        assert_eq!(read.id(), identity.id());
    }

    #[test]
    fn debug_shows_the_id_and_never_the_key() {
        let identity = Identity::ephemeral();
        let secret = hex(&identity.secret_key().to_bytes());
        let dumped = format!("{identity:?}");
        assert!(!dumped.contains(&secret));
        assert!(!dumped.contains(&secret[..12]));
        assert!(dumped.contains(&identity.id().to_string()));
    }
}
