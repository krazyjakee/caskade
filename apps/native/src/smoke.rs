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
//! recovery). `CASKADE_NATIVE_SMOKE_MOTION=spinner|both` turns the status
//! bar's spinner (#96), to measure what animation costs (#121); the theme's
//! backdrop is the frame's own. `CASKADE_NATIVE_SMOKE_FRAMES=<file>` keeps
//! the window's frame counts in `file` (#149), and
//! `CASKADE_NATIVE_SMOKE_HIDE=<seconds>` hides the window as closing it to
//! the tray does. Release builds ignore them
//! unless built with the `smoke` feature.

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
    if let Some(seconds) = var("CASKADE_NATIVE_SMOKE_HIDE").and_then(|s| s.parse::<f64>().ok()) {
        caskade_app::runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs_f64(seconds)).await;
            tracing::info!("smoke: hiding the window");
            crate::launch::ask(crate::launch::Request::Hide);
        });
    }
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

/// `CASKADE_NATIVE_SMOKE_FRAMES=<file>`: writes the window's frame counts to
/// `file` as JSON when they changed, at most four times a second: frames
/// the document painted (`document`, each after a resolve), frames of the
/// backdrop alone (`backdrop`, #149), whether the GPU draws the backdrop
/// (`gpu`) and whether the window's GPU resources are released while it
/// is hidden (`released`). For the backdrop's test and measurements.
pub fn frames(document: u64, backdrop: u64, gpu: bool, released: bool) {
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    /// Frames painted and backdrop frames, on the GPU, released.
    type Counts = (u64, u64, bool, bool);
    static LAST: Mutex<Option<(Instant, Counts)>> = Mutex::new(None);
    let Some(path) = PATH.get_or_init(|| var("CASKADE_NATIVE_SMOKE_FRAMES")) else {
        return;
    };
    let counts = (document, backdrop, gpu, released);
    let mut last = caskade_core::lock(&LAST);
    if let Some((at, written)) = *last
        && (written == counts || at.elapsed() < Duration::from_millis(250))
    {
        return;
    }
    *last = Some((Instant::now(), counts));
    let json = serde_json::json!({
        "document": document,
        "backdrop": backdrop,
        "gpu": gpu,
        "released": released,
    });
    // Whole or not at all: the test reads it while the app runs.
    let partial = format!("{path}.part");
    if std::fs::write(&partial, json.to_string()).is_ok() {
        let _ = std::fs::rename(&partial, path);
    }
}

/// Whether `CASKADE_NATIVE_SMOKE_MOTION` (`spinner` or `both`) asks the
/// status bar's spinner to turn, as when something runs in the background:
/// to measure what it costs (#96, #121).
pub fn spinning() -> bool {
    matches!(
        var("CASKADE_NATIVE_SMOKE_MOTION").as_deref(),
        Some("spinner" | "both")
    )
}
