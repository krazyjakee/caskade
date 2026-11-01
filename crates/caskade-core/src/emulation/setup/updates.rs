//! Updates of managed emulators (EMULATOR-SETUP-PLAN §4): checked like
//! game updates, offered with Update and Skip, applied by installing the
//! newer release over the old one with the user's files carried across.

use super::{EmulatorSetup, ManagedEmulator, SetupJob, source};
use crate::{Error, Result, settings::UpdateSettings, updates::newer};
use std::time::{Duration, Instant};

/// How often [`EmulatorSetup::watch`] looks for emulators due a check.
const TICK: Duration = Duration::from_secs(15 * 60);
/// How long after a failed check the watch leaves the emulator alone,
/// rather than the whole interval of a check that worked.
const RETRY: Duration = Duration::from_secs(30 * 60);

/// A version for people: a rolling build (`latest@2026-10-25T09:30:00Z`)
/// as its tag and day.
pub fn label(version: &str) -> String {
    match version.split_once('@') {
        Some((tag, time)) => format!("{tag} ({})", time.get(..10).unwrap_or(time)),
        None => version.to_owned(),
    }
}

/// Whether the newest version found is newer than the one installed and
/// wasn't skipped.
pub fn available(managed: &ManagedEmulator) -> bool {
    match (&managed.latest, &managed.installed) {
        (Some(latest), Some(installed)) => {
            // Rolling builds compare by publish time (ISO 8601 sorts as text).
            let newer = match (latest.split_once('@'), installed.split_once('@')) {
                (Some((_, new)), Some((_, old))) => new > old,
                _ => newer(latest, installed) || hotfix(latest, installed),
            };
            newer && managed.skipped.as_ref() != Some(latest)
        }
        _ => false,
    }
}

/// A hotfix of the same release: Dolphin's `2609a` after `2609`, `2609b`
/// after `2609a`.
fn hotfix(latest: &str, installed: &str) -> bool {
    fn split(version: &str) -> (&str, &str) {
        let base = version.trim_end_matches(|c: char| c.is_ascii_lowercase());
        version.split_at(base.len())
    }
    let (new_base, new_letter) = split(latest);
    let (old_base, old_letter) = split(installed);
    new_base == old_base
        && new_base.ends_with(|c: char| c.is_ascii_digit())
        && new_letter.len() <= 1
        && new_letter > old_letter
}

impl EmulatorSetup {
    /// Checks a managed emulator's source for a newer version now. A
    /// failure is kept in `error`; the version found and the time of the
    /// last check that worked stay.
    pub async fn check(&self, emulator_id: &str) -> Result<ManagedEmulator> {
        Ok(self.check_once(emulator_id).await?.0.shown())
    }

    /// [`EmulatorSetup::check`], and the source's error when it failed.
    async fn check_once(&self, emulator_id: &str) -> Result<(ManagedEmulator, Option<Error>)> {
        let mut managed = self.managed_by_id(emulator_id).await?;
        let catalog = self.inner.emulation.catalog();
        let source = catalog
            .emulator(&managed.definition_id)
            .and_then(|definition| self.source(definition))
            .filter(|source| source.tracked())
            .cloned();
        let Some(source) = source else {
            return Ok((managed, None));
        };
        let mut failure = None;
        match source::resolve(&self.inner.hosts, &source).await {
            Ok(resolved) => {
                crate::lock(&self.inner.retry_at).remove(emulator_id);
                managed.latest = resolved.version;
                managed.latest_url = Some(resolved.url.to_string());
                managed.error = None;
                managed.checked_at_ms = crate::now_ms();
            }
            Err(error) => {
                crate::lock(&self.inner.retry_at)
                    .insert(emulator_id.to_owned(), Instant::now() + RETRY);
                // Kept as its message, translated when read (#214).
                managed.error = Some(crate::i18n::Text::of_error(&error).store());
                failure = Some(error);
            }
        }
        self.inner.store.upsert_managed_emulator(&managed).await?;
        self.changed();
        Ok((managed, failure))
    }

    /// Whether the emulator's last check failed too recently to try again.
    fn failed_recently(&self, emulator_id: &str) -> bool {
        let now = Instant::now();
        let mut retry_at = crate::lock(&self.inner.retry_at);
        retry_at.retain(|_, at| *at > now);
        retry_at.contains_key(emulator_id)
    }

    /// Stops offering `version` (the newest found, when `None`); a newer
    /// one is offered again.
    pub async fn skip(
        &self,
        emulator_id: &str,
        version: Option<String>,
    ) -> Result<ManagedEmulator> {
        let mut managed = self.managed_by_id(emulator_id).await?;
        managed.skipped = version.or_else(|| managed.latest.clone());
        self.inner.store.upsert_managed_emulator(&managed).await?;
        self.changed();
        Ok(managed.shown())
    }

    /// Installs the newest version over the installed one. Refused while a
    /// game is running in the emulator.
    pub async fn update(&self, emulator_id: &str) -> Result<SetupJob> {
        let managed = self.managed_by_id(emulator_id).await?;
        if self.in_use(emulator_id).await {
            return Err(Error::Invalid(crate::tr!(
                "a game is running in this emulator; update it after the game"
            )));
        }
        self.install(&managed.definition_id).await
    }

    /// Checks the managed emulators whose last check is older than the
    /// update interval, unless update checks are off, except those that
    /// failed in the last half hour. Stops when a source can't be reached.
    /// Returns how many.
    pub async fn check_due(&self) -> Result<usize> {
        let settings = self.update_settings().await;
        if !settings.check {
            return Ok(0);
        }
        let interval = i64::from(settings.interval_hours.max(1)) * 60 * 60 * 1000;
        let now = crate::now_ms();
        let mut checked = 0;
        for managed in self.inner.store.managed_emulators().await? {
            if now - managed.checked_at_ms < interval || self.failed_recently(&managed.emulator_id)
            {
                continue;
            }
            let (_, failure) = self.check_once(&managed.emulator_id).await?;
            checked += 1;
            if failure.as_ref().is_some_and(crate::error::unreachable) {
                break;
            }
        }
        Ok(checked)
    }

    /// Checks for updates now and then, in the background.
    pub fn watch(&self) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(TICK);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                timer.tick().await;
                match this.check_due().await {
                    Ok(0) => {}
                    Ok(checked) => tracing::info!(checked, "checked emulators for updates"),
                    Err(error) => tracing::warn!(%error, "emulator update checks failed"),
                }
            }
        })
    }

    async fn managed_by_id(&self, emulator_id: &str) -> Result<ManagedEmulator> {
        self.inner
            .store
            .managed_emulator(emulator_id)
            .await?
            .ok_or_else(|| {
                Error::Invalid(crate::tr!(
                    "{emulator} isn't installed by Caskade",
                    emulator = emulator_id
                ))
            })
    }

    async fn update_settings(&self) -> UpdateSettings {
        let Some(settings) = crate::lock(&self.inner.settings).clone() else {
            return UpdateSettings::default();
        };
        settings.read_or_default("core.updates").await
    }

    /// Whether a running game was started with this emulator.
    async fn in_use(&self, emulator_id: &str) -> bool {
        let Some(launch) = crate::lock(&self.inner.launch).clone() else {
            return false;
        };
        let Ok(sessions) = launch.running_sessions().await else {
            return false;
        };
        for session in sessions {
            if let Ok(game) = self.inner.emulation.game(&session.entry_id).await
                && game
                    .profile
                    .is_some_and(|profile| profile.emulator_id == emulator_id)
            {
                return true;
            }
        }
        false
    }

    pub(super) fn changed(&self) {
        self.inner.bus.emit(crate::bus::Event::EmulationChanged {
            scope: "emulators".into(),
        });
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    fn managed(installed: &str, latest: &str, skipped: Option<&str>) -> ManagedEmulator {
        ManagedEmulator {
            emulator_id: "e".into(),
            definition_id: "d".into(),
            source: "github".into(),
            dir: None,
            installed: Some(installed.into()),
            file: None,
            latest: Some(latest.into()),
            latest_url: None,
            checked_at_ms: 0,
            skipped: skipped.map(str::to_owned),
            error: None,
        }
    }

    #[test]
    fn newer_releases_and_rebuilt_rolling_tags_are_offered_unless_skipped() {
        assert!(available(&managed("v2.8.2", "v2.8.3", None)));
        assert!(!available(&managed("v2.8.3", "v2.8.3", None)));
        assert!(!available(&managed("v2.8.3", "v2.8.2", None)));
        assert!(!available(&managed("v2.8.2", "v2.8.3", Some("v2.8.3"))));
        assert!(available(&managed(
            "latest@2026-10-20T01:00:00Z",
            "latest@2026-10-25T09:30:00Z",
            None
        )));
        assert!(!available(&managed(
            "latest@2026-10-25T09:30:00Z",
            "latest@2026-10-20T01:00:00Z",
            None
        )));
        assert!(available(&managed("2609", "2609a", None)));
        assert!(available(&managed("2609a", "2609b", None)));
        assert!(!available(&managed("2609a", "2609", None)));
        assert_eq!(label("latest@2026-10-25T09:30:00Z"), "latest (2026-10-25)");
        assert_eq!(label("v2.8.3"), "v2.8.3");
    }
}
