//! What the smoke test (`tests/smoke.rs`) asks a debug build to do, through
//! the environment, until the views can (#96 onwards):
//! `CASKADE_NATIVE_SMOKE_LAUNCH=<entry id>` plays a game once the services
//! are up, `CASKADE_NATIVE_SMOKE_WEB_URL=<url>` adds a web game at `url` and
//! plays it (for a manual run on another OS), `CASKADE_NATIVE_SMOKE_VIDEO=<YouTube
//! id>` opens the video window,
//! `CASKADE_NATIVE_SMOKE_GENERAL=<json>` merges into `core.general` and
//! `CASKADE_NATIVE_SMOKE_APPEARANCE=<json>` into `core.appearance` before
//! the window opens (Start minimized, launch at login; a theme, accent or
//! custom CSS), `CASKADE_NATIVE_SMOKE_RESTYLE=<json>` into `core.appearance`
//! three seconds after the services start (a live change), and
//! `CASKADE_NATIVE_SMOKE_QUIT=<seconds>` quits as closing the window does,
//! and `CASKADE_NATIVE_SMOKE_PANIC=1` panics in the window's UI (crash
//! recovery). `CASKADE_NATIVE_SMOKE_MOTION=spinner|ambient|both` shows the
//! theme's backdrop and a status-bar spinner, until the shell's views have
//! them (#96), to measure what animation costs (#121). Release builds ignore
//! them unless built with the `smoke` feature.

use std::sync::Arc;

use caskade_app::start::Services;
use dioxus_native::winit::window::Window;

fn var(name: &str) -> Option<String> {
    if !cfg!(any(debug_assertions, feature = "smoke")) {
        return None;
    }
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// Before the window opens: `core.general` and `core.appearance` changed
/// as asked.
pub fn settings(settings: &caskade_core::settings::SettingsService) {
    for (name, namespace) in [
        ("CASKADE_NATIVE_SMOKE_GENERAL", "core.general"),
        ("CASKADE_NATIVE_SMOKE_APPEARANCE", "core.appearance"),
    ] {
        if let Some(change) = var(name) {
            caskade_app::runtime::block_on(merge(settings, namespace, &change));
        }
    }
}

/// Merges the JSON object `change` into `namespace`'s settings.
async fn merge(settings: &caskade_core::settings::SettingsService, namespace: &str, change: &str) {
    let changed = async {
        let current = settings
            .get(namespace)
            .await
            .map_err(|error| error.to_string())?;
        let mut value = current.value;
        let change: serde_json::Value =
            serde_json::from_str(change).map_err(|error| error.to_string())?;
        if let (Some(value), Some(change)) = (value.as_object_mut(), change.as_object()) {
            value.extend(change.clone());
        }
        settings
            .set(namespace, value, None)
            .await
            .map_err(|error| error.to_string())
    };
    match changed.await {
        Ok(_) => tracing::info!(namespace, "smoke: settings changed"),
        Err(error) => tracing::error!(%error, namespace, "smoke: settings unchanged"),
    }
}

/// Once the services are up.
pub fn after_start(services: &'static Services) {
    if let Some(change) = var("CASKADE_NATIVE_SMOKE_RESTYLE") {
        caskade_app::runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            merge(&services.settings, "core.appearance", &change).await;
        });
    }
    if let Some(entry) = var("CASKADE_NATIVE_SMOKE_LAUNCH") {
        caskade_app::runtime::spawn(async move {
            match caskade_app::commands::launch::launch_game(&services.launch, &entry).await {
                Ok(ticket) => tracing::info!(?ticket, "smoke: game started"),
                Err(error) => tracing::error!(error = %error.summary, "smoke: game didn't start"),
            }
        });
    }
    if let Some(url) = var("CASKADE_NATIVE_SMOKE_WEB_URL") {
        caskade_app::runtime::spawn(async move {
            match web_game(services, &url).await {
                Ok(ticket) => tracing::info!(?ticket, "smoke: web game started"),
                Err(error) => tracing::error!(%error, "smoke: web game didn't start"),
            }
        });
    }
    if let Some(video) = var("CASKADE_NATIVE_SMOKE_VIDEO") {
        match caskade_app::commands::video::video_play(
            crate::shell::video_window(),
            &video,
            "Smoke test",
            caskade_app::commands::video::VideoPlacement::Anywhere,
        ) {
            Ok(()) => tracing::info!("smoke: video window started"),
            Err(error) => tracing::error!(%error, "smoke: video window didn't start"),
        }
    }
}

/// Adds a web game at `url` and plays it.
async fn web_game(
    services: &Services,
    url: &str,
) -> Result<caskade_core::launch::LaunchTicket, String> {
    use caskade_core::model::{Game, GameEntry, InstallState, LaunchSpec};
    let game = services
        .store
        .upsert_game(&Game::new("Smoke Game"))
        .await
        .map_err(|error| error.to_string())?;
    let mut entry = GameEntry::new(&game.id, caskade_core::web::WEB_SOURCE, url);
    entry.state = InstallState::Installed;
    entry.launch = Some(LaunchSpec::Web { url: url.into() });
    services
        .store
        .upsert_entry(&entry)
        .await
        .map_err(|error| error.to_string())?;
    caskade_app::commands::launch::launch_game(&services.launch, &entry.id)
        .await
        .map_err(|error| error.summary)
}

/// Panics in the UI, as a bug in a view would.
pub fn panic_in_ui() {
    if var("CASKADE_NATIVE_SMOKE_PANIC").is_some() {
        panic!("smoke: a panic in the UI");
    }
}

/// Once the main window is up.
pub fn window_ready(window: Arc<dyn Window>) {
    let Some(seconds) = var("CASKADE_NATIVE_SMOKE_QUIT").and_then(|s| s.parse::<f64>().ok()) else {
        return;
    };
    caskade_app::runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs_f64(seconds)).await;
        tracing::info!("smoke: quitting");
        let _ = window;
        crate::tray::state().quit();
        crate::launch::ask(crate::launch::Request::Quit);
    });
}

/// The theme's backdrop and a status-bar spinner (`animate-worker`, as the
/// webview's status bar has it), as `CASKADE_NATIVE_SMOKE_MOTION` asks.
pub fn motion() -> dioxus_native::prelude::Element {
    use dioxus_native::prelude::*;
    let what = var("CASKADE_NATIVE_SMOKE_MOTION").unwrap_or_default();
    let ambient = matches!(what.as_str(), "ambient" | "both");
    let spinner = matches!(what.as_str(), "spinner" | "both");
    rsx! {
        if ambient {
            div { "data-cs": "backdrop", "aria-hidden": "true" }
        }
        if spinner {
            footer {
                "data-cs": "status-bar",
                "aria-hidden": "true",
                class: "fixed inset-x-0 bottom-0 flex min-h-9 items-center gap-2 border-t border-border bg-surface-raised px-4 py-2",
                svg {
                    "data-part": "spinner",
                    class: "size-3.5 shrink-0 animate-worker text-accent-alt",
                    view_box: "0 0 24 24",
                    fill: "none",
                    stroke: "currentColor",
                    stroke_width: "2",
                    stroke_linecap: "round",
                    path { d: "M21 12a9 9 0 1 1-6.219-8.56" }
                }
            }
        }
    }
}
