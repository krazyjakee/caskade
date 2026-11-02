use crate::{DiscoveryService, Error, Result};
use caskade_core::{
    bus::{Event, LibraryImport, Outcome, SourceCount},
    db::SourceRemoval,
    model::*,
};
use caskade_discovery::{
    Cancel, Confidence, Config, DetectedGame, GameGroup, ScanDelta, ScanReport, SourceReport, uri,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(default, rename_all = "camelCase")]
pub struct ImportOptions {
    /// Seed aggregate Steam/Lutris/Heroic history on first import only.
    pub seed_playtime: bool,
    /// Review candidates require explicit user acceptance (#101).
    pub accept_maybe: bool,
}
impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            seed_playtime: true,
            accept_maybe: false,
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub imported: Vec<ImportedGame>,
    pub review: Vec<GameGroup>,
    pub skipped: Vec<SkippedEntry>,
    pub warnings: Vec<String>,
    pub sources: Vec<SourceReport>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportedGame {
    pub game_id: String,
    pub created: bool,
    pub entries: u32,
    pub seeded_history: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct SkippedEntry {
    pub source: SourceId,
    pub key: String,
    pub reason: String,
}

impl DiscoveryService {
    pub async fn import_report(
        &self,
        report: &ScanReport,
        options: &ImportOptions,
        cancel: &Cancel,
    ) -> Result<ImportOutcome> {
        let _activity = caskade_jobs::global().activity("Importing library");
        let _guard = self.imports.lock().await;
        check(cancel)?;
        let outcome = self
            .import_groups(&report.groups, &report.sources, options, cancel)
            .await?;
        // Only successful, scanned sources are authoritative about absence.
        // Failed or timed-out sources retain their previous entries; turned
        // off ones lose them through `remove_unscanned`.
        let seen = keys(&report.groups);
        for source in report.sources.iter().filter(|s| s.error.is_none()) {
            for entry in self
                .store
                .entries_for_source(source.source.as_str())
                .await?
            {
                check(cancel)?;
                if !seen.contains(&(entry.source_id.clone(), entry.external_id.clone())) {
                    self.mark_absent(entry).await?;
                }
            }
        }
        self.completed(&report.sources).await;
        Ok(outcome)
    }

    pub async fn apply_delta(
        &self,
        delta: &ScanDelta,
        options: &ImportOptions,
        cancel: &Cancel,
    ) -> Result<ImportOutcome> {
        let _activity = caskade_jobs::global().activity("Importing library");
        let _guard = self.imports.lock().await;
        check(cancel)?;
        let groups: Vec<_> = delta.added.iter().chain(&delta.updated).cloned().collect();
        // Capture the previous membership before imports merge or update it.
        let mut previous = Vec::new();
        for group in &groups {
            for game in &group.entries {
                if let Some(old) = self
                    .store
                    .entry_by_source(game.source.as_str(), &game.key)
                    .await?
                {
                    previous.extend(self.store.entries_for_game(&old.game_id).await?);
                }
            }
        }
        for group in &delta.removed {
            for game in &group.entries {
                if let Some(old) = self
                    .store
                    .entry_by_source(game.source.as_str(), &game.key)
                    .await?
                {
                    previous.push(old);
                }
            }
        }
        let outcome = self
            .import_groups(&groups, &delta.sources, options, cancel)
            .await?;
        let seen = keys(&groups);
        let successful: BTreeSet<_> = delta
            .sources
            .iter()
            .filter(|s| s.error.is_none())
            .map(|s| &s.source)
            .collect();
        let mut visited = BTreeSet::new();
        for old in previous {
            check(cancel)?;
            if successful.contains(&old.source_id)
                && !seen.contains(&(old.source_id.clone(), old.external_id.clone()))
                && visited.insert(old.id.clone())
            {
                // Reload after a possible group merge to avoid stale game IDs.
                if let Some(entry) = self.store.get_entry(&old.id).await? {
                    self.mark_absent(entry).await?;
                }
            }
        }
        self.completed(&delta.sources).await;
        Ok(outcome)
    }

    async fn import_groups(
        &self,
        groups: &[GameGroup],
        sources: &[SourceReport],
        options: &ImportOptions,
        cancel: &Cancel,
    ) -> Result<ImportOutcome> {
        let mut outcome = ImportOutcome {
            sources: sources.to_vec(),
            ..Default::default()
        };
        for group in groups {
            check(cancel)?;
            let Some(primary) = group.primary_entry() else {
                outcome
                    .warnings
                    .push("discovery group has no valid primary".into());
                continue;
            };
            if primary.confidence == Confidence::Maybe && !options.accept_maybe {
                outcome.review.push(group.clone());
                continue;
            }
            let mut accepted = Vec::new();
            // Identity proxies (a desktop shortcut to steam://rungameid/730)
            // name the owner's installation. Discovery already merged their
            // ids into the primary; they are not launch entries of their own.
            let (proxies, owners): (Vec<_>, Vec<_>) = group
                .entries
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != group.primary as usize)
                .map(|(_, e)| e)
                .partition(|e| uri::is_proxy(e));
            let retired: Vec<_> = proxies
                .iter()
                .map(|e| (e.source.clone(), e.key.clone()))
                .collect();
            // Primary first, so cancellation of the primary naturally promotes
            // the next accepted entry without creating an empty canonical game.
            for entry in std::iter::once(primary).chain(owners) {
                check(cancel)?;
                let intercepted = self.bus.run::<LibraryImport>(entry.clone()).await;
                for (id, why) in intercepted.skipped {
                    outcome.warnings.push(format!("library.import {id}: {why}"));
                }
                match intercepted.outcome {
                    Outcome::Continue(entry) => accepted.push(entry),
                    Outcome::Cancelled { by, reason } => outcome.skipped.push(SkippedEntry {
                        source: entry.source.clone(),
                        key: entry.key.clone(),
                        reason: format!("{by}: {reason}"),
                    }),
                }
            }
            check(cancel)?;
            let Some(primary) = accepted.first() else {
                continue;
            };
            let mut game = Game::new(&primary.title);
            game.aliases = accepted
                .iter()
                .filter(|e| e.title != game.title)
                .map(|e| e.title.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let entries: Vec<_> = accepted
                .iter()
                .map(|detected| GameEntry::from_detected(&game.id, detected))
                .collect();
            let history = if options.seed_playtime {
                seed(&accepted, &entries, &mut outcome.warnings)
            } else {
                None
            };
            match self
                .store
                .import_group_retiring(&game, &entries, &retired, history.as_ref())
                .await
            {
                Ok(saved) => {
                    let event = if saved.created {
                        Event::LibraryGameAdded {
                            game_id: saved.game_id.clone(),
                        }
                    } else {
                        Event::LibraryGameUpdated {
                            game_id: saved.game_id.clone(),
                        }
                    };
                    self.bus.emit_and_wait(event).await;
                    outcome.imported.push(ImportedGame {
                        game_id: saved.game_id,
                        created: saved.created,
                        entries: saved.entry_ids.len() as u32,
                        seeded_history: saved.seeded_history,
                    });
                }
                Err(error) => outcome.warnings.push(format!("{}: {error}", game.title)),
            }
        }
        Ok(outcome)
    }

    /// Takes out of the library what the sources `config` leaves out found:
    /// a launcher the user turned off takes its games with it. Games another
    /// source also found stay, with their plays (`remove_source_entries`).
    pub async fn remove_unscanned(&self, config: &Config, cancel: &Cancel) -> Result<()> {
        let _guard = self.imports.lock().await;
        for source in &self.sources {
            let id = source.id();
            if config.sources.includes(&id) {
                continue;
            }
            check(cancel)?;
            for change in self.store.remove_source_entries(id.as_str()).await? {
                self.bus
                    .emit_and_wait(match change {
                        SourceRemoval::Updated(game_id) => Event::LibraryGameUpdated { game_id },
                        SourceRemoval::Removed(game_id) => Event::LibraryGameRemoved { game_id },
                    })
                    .await;
            }
        }
        Ok(())
    }

    async fn mark_absent(&self, mut entry: GameEntry) -> Result<()> {
        if matches!(entry.location, Some(InstallLocation::Managed { .. }))
            || matches!(entry.state, InstallState::Partial)
        {
            return Ok(());
        }
        entry.state = InstallState::Partial;
        entry.size = None;
        self.store.upsert_entry(&entry).await?;
        self.bus
            .emit_and_wait(Event::LibraryGameUpdated {
                game_id: entry.game_id,
            })
            .await;
        Ok(())
    }
    async fn completed(&self, sources: &[SourceReport]) {
        self.bus
            .emit_and_wait(Event::LibraryScanCompleted {
                sources: sources
                    .iter()
                    .map(|s| SourceCount {
                        source: s.source.to_string(),
                        count: s.count,
                    })
                    .collect(),
            })
            .await;
    }
}
fn check(cancel: &Cancel) -> Result<()> {
    if cancel.is_cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
fn keys(groups: &[GameGroup]) -> BTreeSet<(SourceId, String)> {
    groups
        .iter()
        .flat_map(|g| &g.entries)
        .map(|e| (e.source.clone(), e.key.clone()))
        .collect()
}
fn seed(
    detected: &[DetectedGame],
    entries: &[GameEntry],
    warnings: &mut Vec<String>,
) -> Option<PlaySession> {
    // These are aggregate totals, possibly duplicated across launchers. Keep
    // the largest known total for the group, never sum duplicate interfaces.
    let (index, game) = detected
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            matches!(
                e.source.as_str(),
                "steam" | "lutris" | "heroic" | "legendary"
            ) && (e.playtime_secs.is_some_and(|t| t > 0) || e.last_played_unix.is_some())
        })
        .max_by_key(|(_, e)| {
            (
                e.playtime_secs.unwrap_or(0),
                e.last_played_unix.unwrap_or(0),
            )
        })?;
    let duration = game
        .playtime_secs
        .unwrap_or(0)
        .checked_mul(1000)
        .and_then(|v| i64::try_from(v).ok())?;
    let Some(last) = game
        .last_played_unix
        .unwrap_or(0)
        .checked_mul(1000)
        .and_then(|v| i64::try_from(v).ok())
    else {
        warnings.push(format!(
            "{}: play history timestamp exceeds SQLite range",
            game.title
        ));
        return None;
    };
    let entry = &entries[index];
    let mut session = PlaySession::new(&entry.game_id, &entry.id, last);
    session.ended_at_ms = Some(last);
    session.duration_ms = duration as u64;
    session.tracking = Some(
        serde_json::json!({"discoverySeed":{"source":game.source,"key":game.key,"aggregate":true}}),
    );
    Some(session)
}
