//! What the smoke test (`tests/smoke.rs`) asks a debug build to do, through
//! the environment, until the views can (#96 onwards):
//! `CASKADE_NATIVE_SMOKE_LAUNCH=<entry id>` plays a game once the services
//! are up, `CASKADE_NATIVE_SMOKE_WEB_URL=<url>` adds a web game at `url` and
//! plays it (for a manual run on another OS), `CASKADE_NATIVE_SMOKE_VIDEO=<YouTube
//! id>` opens the video window,
//! `CASKADE_NATIVE_SMOKE_QUIT=<seconds>` quits as closing the window does,
//! and `CASKADE_NATIVE_SMOKE_PANIC=1` panics in the window's UI (crash
//! recovery). Release builds ignore them unless built with the `smoke` feature.

use std::sync::Arc;

use caskade_app::start::Services;
use dioxus_native::winit::window::Window;

fn var(name: &str) -> Option<String> {
    if !cfg!(any(debug_assertions, feature = "smoke")) {
        return None;
    }
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// Once the services are up.
pub fn after_start(services: &'static Services) {
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
        crate::shell::quit(window.as_ref());
    });
}
