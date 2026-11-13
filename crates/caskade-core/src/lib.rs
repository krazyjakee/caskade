//! Caskade's application core: domain model, SQLite
//! library store, artwork cache, settings, launch pipeline, process
//! tracking, play sessions and the in-process hook bus.
//!
//! The crate has no Tauri dependency: `caskade-app` builds every app
//! shell's commands on it, so everything here is unit-testable. The library
//! emits `tracing` events and spans without setting a global subscriber.

pub mod accounts;
pub mod artwork;
pub mod backup_error;
pub mod browse;
pub mod bus;
mod cache_db;
pub mod db;
pub mod device_type;
#[cfg(feature = "net")]
pub mod devices;
pub mod emulation;
pub mod error;
pub mod hasheous;
pub mod i18n;
pub mod icons;
pub mod igdb;
pub mod install;
pub mod launch;
pub mod login_item;
pub mod ludusavi;
pub mod managed;
pub mod manual;
pub mod metadata;
pub mod model;
pub mod paths;
pub mod ports;
pub mod program;
#[cfg(feature = "net")]
pub(crate) mod remote_file;
#[cfg(not(feature = "net"))]
pub(crate) mod remote_file {
    //! Built without network support: nothing is fetched.
    use crate::Result;
    #[expect(dead_code, reason = "network-free builds only read what is cached")]
    pub(crate) enum Response {
        Unchanged,
        Changed {
            bytes: Vec<u8>,
            etag: Option<String>,
        },
    }
    pub(crate) async fn fetch(_: &str, _: Option<&str>, _: &str) -> Result<Response> {
        Err(crate::Error::Invalid(
            "built without network support".into(),
        ))
    }
}
pub mod quiet;
#[cfg(feature = "net")]
pub mod remote_play;
pub mod romm;
pub mod saves;
pub mod scripts;
pub mod secrets;
pub mod sessions;
pub mod settings;
pub mod storage;
pub mod store_accounts;
pub mod sync;
pub mod themes;
// Tests that write a program and run it (#118); siblings use `test-utils`.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_programs;
pub mod tracking;
#[cfg(feature = "net")]
pub mod transfer;
pub mod updates;
pub mod web;

pub use caskade_jobs as jobs;

/// The core's version, as built.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Milliseconds since the Unix epoch, as stored in the database; 0 before
/// the epoch, saturating at `i64::MAX`.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

/// The `(year, month, day)` of `days` since the Unix epoch, in the
/// proleptic Gregorian calendar (Howard Hinnant's `civil_from_days`).
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

pub use error::{Error, Result};
pub use paths::AppPaths;

/// Locks a mutex, carrying on with the data a panicking holder left.
pub fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Read-locks an `RwLock`, carrying on with the data a panicking holder left.
pub fn read<T>(lock: &std::sync::RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Write-locks an `RwLock`, carrying on with the data a panicking holder left.
pub fn write<T>(lock: &std::sync::RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
