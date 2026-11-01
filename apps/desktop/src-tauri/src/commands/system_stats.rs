//! Settings → Statistics (#156): the core's system sampler. The dashboard
//! watches while it is open and visible, renews its watch every few
//! seconds, and gets each sample as a `SystemStatsSampled` event. Read-only:
//! nothing here changes the machine or the library.

use caskade_core::system_stats::{SystemSample, SystemStats, SystemStatsSnapshot};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_specta::Event;

/// Each sample, while anyone watches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Event)]
pub struct SystemStatsSampled(pub SystemSample);

/// Manages the sampler and forwards its samples to the webview.
pub fn setup<R: Runtime>(app: &AppHandle<R>) {
    let stats = SystemStats::new();
    let mut samples = stats.subscribe();
    app.manage(stats);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match samples.recv().await {
                Ok(sample) => {
                    if let Err(error) = SystemStatsSampled(sample).emit(&handle) {
                        tracing::debug!(%error, "system statistics sample not delivered");
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
}

/// Starts or renews `watcher`'s watch (it lasts 30 s unless renewed) and
/// answers how sampling runs, with the history so far when `history`.
#[tauri::command]
#[specta::specta]
pub async fn system_stats_watch(
    stats: State<'_, SystemStats>,
    watcher: String,
    history: bool,
) -> Result<SystemStatsSnapshot, String> {
    Ok(stats.watch(&watcher, history))
}

/// Ends `watcher`'s watch; sampling stops when no watch is left.
#[tauri::command]
#[specta::specta]
pub async fn system_stats_unwatch(
    stats: State<'_, SystemStats>,
    watcher: String,
) -> Result<(), String> {
    stats.unwatch(&watcher);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_ipc::invoke;
    use caskade_core::system_stats::{Probe, Readings, SamplerConfig};
    use serde_json::json;
    use std::time::Duration;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    struct Steady;
    impl Probe for Steady {
        fn read(&mut self) -> Readings {
            Readings {
                cpu: Some(12.5),
                cores: 4,
                memory: Some((8 << 30, 2 << 30)),
                ..Readings::default()
            }
        }
    }

    #[test]
    fn the_dashboard_watches_and_unwatches_over_ipc() {
        let stats = SystemStats::with_probe(
            SamplerConfig {
                interval: Duration::from_millis(10),
                history: 60,
                lease: Duration::from_secs(30),
            },
            || Box::new(Steady) as Box<dyn Probe>,
        );
        let app = mock_builder()
            .manage(stats.clone())
            .invoke_handler(tauri::generate_handler![
                system_stats_watch,
                system_stats_unwatch
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let snapshot = invoke(
            &window,
            "system_stats_watch",
            json!({"watcher": "settings", "history": true}),
        )
        .unwrap();
        assert_eq!(snapshot["intervalMs"], 10);
        assert_eq!(snapshot["history"], 60);
        assert!(stats.is_sampling());
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while stats.history().is_empty() {
            assert!(std::time::Instant::now() < deadline, "no sample");
            std::thread::sleep(Duration::from_millis(5));
        }
        let sample = serde_json::to_value(&stats.history()[0]).unwrap();
        assert_eq!(sample["cpu"]["usage"], 12.5);
        assert_eq!(sample["memory"]["used"], 6u64 << 30);
        invoke(
            &window,
            "system_stats_unwatch",
            json!({"watcher": "settings"}),
        )
        .unwrap();
        while stats.is_sampling() {
            assert!(std::time::Instant::now() < deadline, "still sampling");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
