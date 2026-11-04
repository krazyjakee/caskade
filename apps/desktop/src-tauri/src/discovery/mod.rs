//! Owns app discovery lifecycle and the retained, server-authoritative review
//! list. Review commands accept opaque IDs, never client-authored game records.
mod config;
mod lifecycle;
#[cfg(test)]
mod tests;
pub use config::OsCheck;

use caskade_core::settings::{DiscoveryExclusion, SettingsService};
use caskade_discovery::{Cancel, Config, GameGroup, LauncherStatus, ScanReport, SourceReport};
use caskade_sources::{
    DiscoveryService, ImportOptions, ImportOutcome,
    storage::{StorageScanReport, StorageSource},
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri_specta::Event;
use tokio::sync::{Mutex, watch};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItem {
    pub id: String,
    pub title: String,
    pub sources: Vec<String>,
    pub location: Option<String>,
    pub evidence: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverySnapshot {
    pub revision: u32,
    pub os: String,
    pub checks: Vec<OsCheck>,
    pub phase: String,
    #[serde(default)]
    pub active_sources: Vec<String>,
    pub launchers: Vec<LauncherStatus>,
    pub sources: Vec<SourceReport>,
    pub found: u32,
    pub imported: u32,
    pub review: Vec<ReviewItem>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub watching: bool,
    /// Managed-root repair results, including explicit unresolved questions.
    #[serde(default)]
    pub storage: Option<StorageScanReport>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct DiscoveryChanged(pub DiscoverySnapshot);

struct State {
    snapshot: DiscoverySnapshot,
    review: BTreeMap<String, GameGroup>,
    next_review: u64,
    cancel: Option<Cancel>,
    /// What the running scan scans with: a settings change cancels it only
    /// when this is out of date.
    scanning_with: Option<(Config, ImportOptions)>,
}
struct Inner {
    service: DiscoveryService,
    storage: std::sync::RwLock<Option<StorageSource>>,
    settings: SettingsService,
    state: Mutex<State>,
    operation: Mutex<()>,
    updates: watch::Sender<DiscoverySnapshot>,
    started: AtomicBool,
    stop: watch::Sender<bool>,
}
#[derive(Clone)]
pub struct DiscoveryController(Arc<Inner>);

impl DiscoveryController {
    pub fn new(service: DiscoveryService, settings: SettingsService) -> Self {
        let os = service.env().os();
        let snapshot = DiscoverySnapshot {
            revision: 0,
            os: config::os_name(os).into(),
            checks: config::os_checks(os),
            phase: "idle".into(),
            active_sources: vec![],
            launchers: vec![],
            sources: vec![],
            found: 0,
            imported: 0,
            review: vec![],
            warnings: vec![],
            error: None,
            watching: false,
            storage: None,
        };
        let updates = watch::channel(snapshot.clone()).0;
        Self(Arc::new(Inner {
            service,
            storage: std::sync::RwLock::new(None),
            settings,
            state: Mutex::new(State {
                snapshot,
                review: BTreeMap::new(),
                next_review: 0,
                cancel: None,
                scanning_with: None,
            }),
            operation: Mutex::new(()),
            updates,
            started: AtomicBool::new(false),
            stop: watch::channel(false).0,
        }))
    }
    /// Reconcile registered storage roots before discovering external games.
    pub fn with_storage(self, source: StorageSource) -> Self {
        *self.0.storage.write().unwrap_or_else(|e| e.into_inner()) = Some(source);
        self
    }
    pub async fn snapshot(&self) -> DiscoverySnapshot {
        self.0.state.lock().await.snapshot.clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<DiscoverySnapshot> {
        self.0.updates.subscribe()
    }
    fn publish(&self, state: &mut State) {
        state.snapshot.revision = state.snapshot.revision.saturating_add(1);
        state.snapshot.review = state
            .review
            .iter()
            .filter_map(|(id, group)| {
                let primary = group.primary_entry()?;
                Some(ReviewItem {
                    id: id.clone(),
                    title: primary.title.clone(),
                    sources: group.entries.iter().map(|e| e.source.to_string()).collect(),
                    location: primary
                        .install_dir
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned()),
                    evidence: primary
                        .evidence
                        .iter()
                        .map(|e| match e {
                            caskade_discovery::Evidence::File { path } => {
                                caskade_core::tr!("File: {path}", path = path.display())
                            }
                            caskade_discovery::Evidence::Registry { key, value } => match value {
                                Some(value) => caskade_core::tr!(
                                    "Registry: {key} ({value})",
                                    key = key,
                                    value = value
                                ),
                                None => caskade_core::tr!("Registry: {key}", key = key),
                            },
                            caskade_discovery::Evidence::Rule { rule, score } => {
                                format!("{rule}: {score:+}")
                            }
                        })
                        .collect(),
                })
            })
            .collect();
        self.0.updates.send_replace(state.snapshot.clone());
    }
    pub async fn launchers(&self) -> Result<DiscoverySnapshot, String> {
        let launchers = self
            .0
            .service
            .detect_launchers()
            .await
            .map_err(|e| e.to_string())?;
        let mut state = self.0.state.lock().await;
        state.snapshot.launchers = launchers;
        self.publish(&mut state);
        Ok(state.snapshot.clone())
    }
    /// No scan has run since startup.
    async fn idle(&self) -> bool {
        self.0.state.lock().await.snapshot.phase == "idle"
    }
    pub async fn scan(&self) -> Result<DiscoverySnapshot, String> {
        let (preferences, launchers) = config::preferences(&self.0.settings).await?;
        if !preferences.enabled {
            return Err(caskade_core::tr!(
                "Enable library discovery before scanning."
            ));
        }
        let config = config::scan_config(&preferences, &launchers, self.0.service.env().os())?;
        let options = caskade_sources::import_options(&self.0.settings)
            .await
            .map_err(|e| e.to_string())?;
        let mut state = self.0.state.lock().await;
        if state.cancel.is_some() {
            return Err(caskade_core::tr!("A discovery scan is already running."));
        }
        let cancel = Cancel::new();
        state.cancel = Some(cancel.clone());
        state.scanning_with = Some((config.clone(), options.clone()));
        state.snapshot.phase = "scanning".into();
        state.snapshot.active_sources.clear();
        state.snapshot.sources.clear();
        state.snapshot.found = 0;
        state.snapshot.imported = 0;
        state.snapshot.warnings.clear();
        state.snapshot.error = None;
        state.snapshot.storage = None;
        self.publish(&mut state);
        let snapshot = state.snapshot.clone();
        drop(state);
        let controller = self.clone();
        tokio::spawn(async move {
            let _activity =
                caskade_core::jobs::global().activity(caskade_core::tr!("Updating library"));
            let _operation = controller.0.operation.lock().await;
            let result = async {
                controller.refresh_storage(&cancel).await?;
                let (progress, mut updates) = tokio::sync::mpsc::unbounded_channel();
                let discovery = controller.0.service.discover_with_progress(&config, &cancel, move |update| {
                    let _ = progress.send(update);
                });
                tokio::pin!(discovery);
                let scan = loop {
                    tokio::select! {
                        result = &mut discovery => break result.map_err(|e| e.to_string())?,
                        Some(update) = updates.recv() => {
                            let mut state = controller.0.state.lock().await;
                            match update {
                                caskade_sources::DiscoveryProgress::Started(source) => {
                                    state.snapshot.active_sources.push(source.to_string());
                                }
                                caskade_sources::DiscoveryProgress::Finished { source, count, error } => {
                                    state.snapshot.active_sources.retain(|active| active != source.as_str());
                                    state.snapshot.sources.push(SourceReport {
                                        source, count, duration_ms: 0, warnings: vec![],
                                        error: error.map(|message| caskade_discovery::SourceError::Other { message }),
                                    });
                                }
                            }
                            controller.publish(&mut state);
                        }
                    }
                };
                if cancel.is_cancelled() {
                    return Err(caskade_core::tr!("Scan cancelled"));
                }
                {
                    let mut state = controller.0.state.lock().await;
                    state.snapshot.phase = "importing".into();
                    state.snapshot.active_sources.clear();
                    state.snapshot.sources = scan.sources.clone();
                    state.snapshot.found = scan.groups.len() as u32;
                    state.snapshot.launchers = scan.launchers.clone();
                    controller.publish(&mut state);
                }
                let import = controller
                    .0
                    .service
                    .import_report(&scan, &options, &cancel)
                    .await
                    .map_err(|e| e.to_string())?;
                // Launchers turned off take their games with them.
                controller
                    .0
                    .service
                    .remove_unscanned(&config, &cancel)
                    .await
                    .map_err(|e| e.to_string())?;
                controller.authorize_reviews(import, &cancel).await
            }
            .await;
            let mut state = controller.0.state.lock().await;
            state.cancel = None;
            state.snapshot.active_sources.clear();
            state.scanning_with = None;
            match result {
                Ok(import) => {
                    state.review.clear();
                    controller.apply_import(&mut state, import);
                    state.snapshot.phase = "complete".into();
                }
                Err(error) => {
                    state.snapshot.phase = if cancel.is_cancelled() {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                    state.snapshot.error = Some(error);
                }
            }
            controller.publish(&mut state);
        });
        Ok(snapshot)
    }
    /// Cancels the running scan if the settings no longer scan what it
    /// does. One started for the settings now in force carries on: a
    /// settings write that changes nothing it uses, or first-run setup
    /// finishing, doesn't throw its work away.
    async fn cancel_if_outdated(&self) {
        let current = async {
            let (preferences, launchers) = config::preferences(&self.0.settings).await?;
            if !preferences.enabled {
                return Err(String::new());
            }
            let config = config::scan_config(&preferences, &launchers, self.0.service.env().os())?;
            let options = caskade_sources::import_options(&self.0.settings)
                .await
                .map_err(|e| e.to_string())?;
            Ok((config, options))
        }
        .await
        .ok();
        let outdated = {
            let state = self.0.state.lock().await;
            state.scanning_with.is_some() && state.scanning_with != current
        };
        if outdated {
            self.cancel().await;
        }
    }
    pub async fn cancel(&self) -> DiscoverySnapshot {
        let mut state = self.0.state.lock().await;
        if let Some(cancel) = &state.cancel {
            cancel.cancel();
            state.snapshot.phase = "cancelling".into();
            self.publish(&mut state);
        }
        state.snapshot.clone()
    }
    fn apply_import(&self, state: &mut State, outcome: ImportOutcome) {
        state.snapshot.imported = outcome.imported.len() as u32;
        state.snapshot.sources = outcome.sources;
        state.snapshot.warnings = outcome.warnings;
        if let Some(storage) = &state.snapshot.storage {
            state.snapshot.warnings.extend(storage_warnings(storage));
        }
        for group in outcome.review {
            if let Some(primary) = group.primary_entry() {
                state.review.retain(|_, old| {
                    !old.entries
                        .iter()
                        .any(|entry| entry.source == primary.source && entry.key == primary.key)
                });
            }
            // Review entries stay bounded even with an unbounded watch lifetime.
            if state.review.len() < 1000 {
                state.next_review += 1;
                state.review.insert(state.next_review.to_string(), group);
            }
        }
    }
    async fn refresh_storage(&self, cancel: &Cancel) -> Result<(), String> {
        if cancel.is_cancelled() {
            return Err(caskade_core::tr!("Scan cancelled"));
        }
        let source = self
            .0
            .storage
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(source) = source else {
            return Ok(());
        };
        let token = caskade_core::storage::CancellationToken::new();
        let refresh = source.refresh(token.clone());
        tokio::pin!(refresh);
        let result = loop {
            tokio::select! {
                result = &mut refresh => break result,
                _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => {
                    if cancel.is_cancelled() { token.cancel(); }
                }
            }
        };
        if cancel.is_cancelled() {
            return Err(caskade_core::tr!("Scan cancelled"));
        }
        let report = result.map_err(|e| e.to_string())?;
        let mut state = self.0.state.lock().await;
        state.snapshot.warnings = storage_warnings(&report);
        state.snapshot.storage = Some(report);
        self.publish(&mut state);
        Ok(())
    }
    async fn authorize_reviews(
        &self,
        mut outcome: ImportOutcome,
        cancel: &Cancel,
    ) -> Result<ImportOutcome, String> {
        let (preferences, _) = config::preferences(&self.0.settings).await?;
        let (approved, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut outcome.review)
            .into_iter()
            .partition(|group| {
                group.entries.iter().any(|entry| {
                    preferences.accepted_entries.iter().any(|accepted| {
                        accepted.source == entry.source.as_str() && accepted.key == entry.key
                    })
                })
            });
        outcome.review = pending;
        if !approved.is_empty() {
            let mut options = caskade_sources::import_options(&self.0.settings)
                .await
                .map_err(|e| e.to_string())?;
            options.accept_maybe = true;
            let report = ScanReport {
                groups: approved,
                ..Default::default()
            };
            let imported = self
                .0
                .service
                .import_report(&report, &options, cancel)
                .await
                .map_err(|e| e.to_string())?;
            if imported.imported.len() != report.groups.len() {
                outcome.review.extend(report.groups);
            }
            outcome.imported.extend(imported.imported);
            outcome.warnings.extend(imported.warnings);
            outcome.skipped.extend(imported.skipped);
        }
        Ok(outcome)
    }
    pub async fn review(
        &self,
        ids: Vec<String>,
        accept: bool,
        revision: u32,
    ) -> Result<DiscoverySnapshot, String> {
        if ids.is_empty() || ids.len() > 1000 {
            return Err(caskade_core::tr!("Choose between 1 and 1000 review items."));
        }
        let _operation = self.0.operation.lock().await;
        let state = self.0.state.lock().await;
        if state.cancel.is_some() || state.snapshot.revision != revision {
            return Err(caskade_core::tr!(
                "Discovery changed. Review the latest list."
            ));
        }
        let ids: BTreeSet<_> = ids.into_iter().collect();
        let groups: Vec<_> = ids
            .iter()
            .map(|id| {
                state
                    .review
                    .get(id)
                    .cloned()
                    .ok_or_else(|| caskade_core::tr!("Review item is no longer available."))
            })
            .collect::<Result<_, _>>()?;
        drop(state);
        // Persist explicit decisions before importing. A failed import can be
        // retried safely, and a later scan does not ask for the same approval.
        let snapshot = self
            .0
            .settings
            .get("core.discovery")
            .await
            .map_err(|e| e.to_string())?;
        let mut preferences: caskade_core::settings::DiscoverySettings =
            serde_json::from_value(snapshot.value).map_err(|e| e.to_string())?;
        let decisions = if accept {
            &mut preferences.accepted_entries
        } else {
            &mut preferences.ignored_entries
        };
        for entry in groups.iter().flat_map(|g| &g.entries) {
            let decision = DiscoveryExclusion {
                source: entry.source.to_string(),
                key: entry.key.clone(),
                title: Some(entry.title.chars().take(512).collect())
                    .filter(|t: &String| !t.is_empty()),
            };
            if !decisions
                .iter()
                .any(|d| d.source == decision.source && d.key == decision.key)
            {
                decisions.push(decision);
            }
        }
        self.0
            .settings
            .set(
                "core.discovery",
                serde_json::to_value(preferences).map_err(|e| e.to_string())?,
                Some(snapshot.revision),
            )
            .await
            .map_err(|e| e.to_string())?;
        if accept {
            let mut options = caskade_sources::import_options(&self.0.settings)
                .await
                .map_err(|e| e.to_string())?;
            options.accept_maybe = true;
            // No successful-source authority here: accepting a subset must not
            // mark unrelated library entries absent.
            let report = ScanReport {
                groups,
                ..Default::default()
            };
            let result = self
                .0
                .service
                .import_report(&report, &options, &Cancel::new())
                .await
                .map_err(|e| e.to_string())?;
            if !result.warnings.is_empty() || !result.skipped.is_empty() {
                return Err(caskade_core::tr!(
                    "Some entries could not be imported; the review list was retained. Check the library before retrying."
                ));
            }
        }
        let mut state = self.0.state.lock().await;
        for id in &ids {
            state.review.remove(id);
        }
        self.publish(&mut state);
        Ok(state.snapshot.clone())
    }
}

fn storage_warnings(report: &StorageScanReport) -> Vec<String> {
    let mut warnings = Vec::new();
    for root in &report.roots {
        if let Some(error) = &root.error {
            warnings.push(caskade_core::tr!(
                "Storage {root}: {error}",
                root = root.root.id,
                error = error
            ));
        }
        warnings.extend(root.warnings.iter().map(|w| w.message.clone()));
        for game in &root.games {
            warnings.extend(game.warnings.iter().map(|w| w.message.clone()));
            if let Some(error) = &game.error {
                warnings.push(error.clone());
            }
            if game.question.is_some() {
                warnings.push(caskade_core::tr!(
                    "Storage {root} / {folder} requires a decision",
                    root = root.root.id,
                    folder = game.relative_dir.display()
                ));
            }
        }
    }
    warnings
}
