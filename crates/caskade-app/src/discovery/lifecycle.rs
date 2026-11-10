use super::*;
use caskade_discovery::watch::WatchOptions;
use tokio::sync::broadcast::error::RecvError;

impl DiscoveryController {
    /// Idempotent startup: one settings listener, schedule and incremental
    /// watcher. Source absence never deletes library/history records.
    pub fn start(&self) {
        if self.0.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let controller = self.clone();
        crate::runtime::spawn(async move {
            controller.lifecycle().await;
        });
    }
    /// Stop scan/watch work and wait for the lifecycle to finish. The caller
    /// owns the controller lifetime; this is also used by deterministic tests.
    pub async fn stop(&self) {
        self.0.stop.send_replace(true);
        self.cancel().await;
        let mut updates = self.subscribe();
        while self.0.started.load(Ordering::Acquire) {
            if updates.changed().await.is_err() {
                break;
            }
        }
    }
    /// Sends every change of the controller's state to `events`.
    pub fn bridge(&self, events: crate::events::Events) {
        let mut updates = self.subscribe();
        crate::runtime::spawn(async move {
            while updates.changed().await.is_ok() {
                let snapshot = updates.borrow_and_update().clone();
                events.send(crate::events::Event::DiscoveryChanged(DiscoveryChanged(
                    snapshot,
                )));
            }
        });
    }
    async fn lifecycle(self) {
        let mut changes = self.0.settings.subscribe();
        let mut first = true;
        let mut stop = self.0.stop.subscribe();
        loop {
            if *stop.borrow_and_update() {
                break;
            }
            let preferences = config::preferences(&self.0.settings).await;
            let (preferences, launchers) = match preferences {
                Ok(value) => value,
                Err(error) => {
                    self.lifecycle_error(error.summary).await;
                    if changes.recv().await.is_err() {
                        return;
                    }
                    continue;
                }
            };
            // The first scan waits for first-run setup, which may have run
            // one itself by the time it is done.
            let ready = crate::onboarding::completed(&self.0.settings).await;
            if first
                && ready
                && preferences.enabled
                && preferences.scan_on_startup
                && self.idle().await
            {
                self.background_scan().await;
            }
            first &= !ready;
            let mut watcher = None;
            if preferences.enabled && preferences.watch_folders {
                match async {
                    let config =
                        config::scan_config(&preferences, &launchers, self.0.service.env().os())?;
                    let options = caskade_sources::import_options(&self.0.settings)
                        .await
                        .map_err(failure)?;
                    self.0
                        .service
                        .watch(config, options, WatchOptions::default())
                        .await
                        .map_err(failure)
                }
                .await
                {
                    Ok(value) => watcher = Some(value),
                    Err(error) => self.lifecycle_error(error.summary).await,
                }
            }
            {
                let mut state = self.0.state.lock().await;
                state.snapshot.watching = watcher.is_some();
                self.publish(&mut state);
            }
            let timer = tokio::time::sleep(config::refresh_interval(&preferences));
            tokio::pin!(timer);
            loop {
                tokio::select! {
                    _ = stop.changed() => break,
                    changed = changes.recv() => {
                        match changed {
                            Ok(change) if matches!(change.namespace.as_str(), "core.discovery" | "core.launchers" | "discovery.import" | "core.onboarding") => break,
                            Err(RecvError::Lagged(_)) => break,
                            Err(RecvError::Closed) => return,
                            _ => {},
                        }
                    }
                    () = &mut timer, if preferences.enabled && preferences.background_refresh => {
                        self.background_scan().await;
                        timer.as_mut().reset(tokio::time::Instant::now() + config::refresh_interval(&preferences));
                    }
                    update = async {
                        match watcher.as_mut() {
                            Some(watcher) => { watcher.status.changed().await.map_err(|_| ())?; Ok(watcher.status.borrow_and_update().clone()) },
                            None => std::future::pending::<Result<Option<Result<ImportOutcome, String>>, ()>>().await,
                        }
                    } => {
                        match update {
                            Ok(Some(Ok(outcome))) => {
                                let _operation = self.0.operation.lock().await;
                                match self.authorize_reviews(outcome, &Cancel::new()).await {
                                    Ok(outcome) => { let mut state = self.0.state.lock().await; self.apply_import(&mut state, outcome); self.publish(&mut state); },
                                    Err(error) => self.lifecycle_error(error.summary).await,
                                }
                            },
                            Ok(Some(Err(error))) => self.lifecycle_error(error).await,
                            Err(()) => { self.lifecycle_error(caskade_core::tr!("Discovery watcher stopped.")).await; break; },
                            _ => {},
                        }
                    }
                }
            }
            // Cancel a scan of an old configuration before publishing any
            // later scan.
            self.cancel_if_outdated().await;
            if let Some(watcher) = watcher {
                watcher.stop().await;
            }
            let _operation = self.0.operation.lock().await;
            let mut state = self.0.state.lock().await;
            state.snapshot.watching = false;
            self.publish(&mut state);
        }
        self.0.started.store(false, Ordering::Release);
        let mut state = self.0.state.lock().await;
        self.publish(&mut state);
    }
    /// A scan nobody asked for: a failure before it starts (its settings,
    /// import options) shows in Settings → Library like one during it,
    /// instead of vanishing (#160). One already running is no failure.
    async fn background_scan(&self) {
        if let Err(error) = self.scan().await
            && self.idle().await
        {
            tracing::warn!(%error, "background discovery scan didn't start");
            self.lifecycle_error(error.summary).await;
        }
    }

    async fn lifecycle_error(&self, error: String) {
        let mut state = self.0.state.lock().await;
        state.snapshot.error = Some(error);
        self.publish(&mut state);
    }
}
