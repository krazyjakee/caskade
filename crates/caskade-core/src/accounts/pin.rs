//! PIN hashes and the wrong-PIN throttle. PINs are hashed with argon2id
//! (the crate's defaults: 19 MiB, 2 passes) and a salt of their own, off the
//! async runtime. Nothing here logs or returns a PIN.
use crate::{Error, Result};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};

/// Wrong tries before a wait.
pub(crate) const TRIES: u32 = 5;
const FIRST_WAIT_MS: i64 = 30_000;
const LONGEST_WAIT_MS: i64 = 15 * 60_000;

/// Whether `pin` is 4–8 digits.
pub(crate) fn valid(pin: &str) -> bool {
    (4..=8).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

/// A PHC string with the PIN's argon2id hash and its own salt.
pub(crate) async fn hash(pin: String) -> Result<String> {
    if !valid(&pin) {
        return Err(Error::Invalid(crate::tr!("a PIN is 4 to 8 digits")));
    }
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(pin.as_bytes(), &salt)
            .map(|hash| hash.to_string())
    })
    .await
    .map_err(|_| Error::Invalid(crate::tr!("could not hash the PIN")))?
    .map_err(|_| Error::Invalid(crate::tr!("could not hash the PIN")))
}

/// Whether `pin` matches `hash`; an unreadable hash matches nothing.
pub(crate) async fn verify(pin: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(pin.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false)
}

/// An account's wrong PINs: after [`TRIES`] a wait of 30 s, doubling with
/// each run of wrong tries up to 15 min. Stored with the account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Throttle {
    /// Wrong tries since the last wait.
    pub failures: u32,
    /// Waits since the last right PIN.
    pub lockouts: u32,
    /// Unix ms; tries are refused before it.
    pub locked_until_ms: i64,
}

impl Throttle {
    pub fn locked(&self, now: i64) -> bool {
        self.locked_until_ms > now
    }

    /// The throttle after a wrong PIN at `now`.
    #[must_use]
    pub fn wrong(self, now: i64) -> Self {
        let failures = self.failures + 1;
        if failures < TRIES {
            return Self { failures, ..self };
        }
        let lockouts = self.lockouts.saturating_add(1);
        Self {
            failures: 0,
            lockouts,
            locked_until_ms: now.saturating_add(wait_ms(lockouts)),
        }
    }
}

/// The wait after the `lockouts`th run of wrong tries.
pub(crate) fn wait_ms(lockouts: u32) -> i64 {
    let doublings = lockouts.saturating_sub(1).min(16);
    (FIRST_WAIT_MS << doublings).min(LONGEST_WAIT_MS)
}
