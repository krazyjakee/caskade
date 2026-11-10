//! Caskade's native app (#87): the main window rendered by Blitz, running
//! the same application layer as the Tauri app (`caskade-app`). It ships
//! beside the Tauri app until it reaches parity (BLITZ-33).
//!
//! One winit event loop (Blitz's shell) owns the main thread. Web games and
//! videos open in windows of their own, in a process of their own: this
//! executable started again with `--web-game` or `--video`
//! (`caskade-player`), so no webview shares the loop.

// The data hooks (#89); the views (#96 onwards) use them.
#[allow(dead_code)]
mod data;
mod shell;
mod smoke;
mod window_state;

use std::sync::{Arc, OnceLock};

use dioxus_native::prelude::*;

/// Most workers the app's async runtime starts, as the Tauri app.
const ASYNC_WORKERS: usize = 4;

/// The main window's state keeper, for the root component and the exit.
static KEEPER: OnceLock<shell::Keeper> = OnceLock::new();

fn main() {
    // `caskade-native --web-game <url>` / `--video <id>`: a game's or a
    // video's window, not the app. Their WebKitGTK first gets the renderer
    // the Tauri app picks for its own (it may restart the process); Blitz's
    // main window doesn't use WebKit.
    if caskade_player::web_player::Options::from_args(std::env::args_os()).is_some()
        || caskade_player::video_player::Options::from_args(std::env::args_os()).is_some()
    {
        caskade_player::webkit::prefer_gpu();
    }
    if let Some(options) = caskade_player::web_player::Options::from_args(std::env::args_os()) {
        logging();
        exit_window(
            caskade_player::web_player::run(options),
            "the game window couldn't open",
        );
    }
    if let Some(options) = caskade_player::video_player::Options::from_args(std::env::args_os()) {
        logging();
        exit_window(
            caskade_player::video_player::run(options),
            "the video window couldn't open",
        );
    }
    logging();
    caskade_app::background::log_panics();
    if let Err(error) = caskade_app::runtime::init(ASYNC_WORKERS) {
        tracing::warn!(%error, "the async runtime uses its defaults");
    }
    // The OS's language, until the Language setting picks one.
    caskade_core::i18n::set_locale(caskade_core::i18n::SYSTEM);
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "Caskade (native) starting"
    );
    let paths = caskade_core::AppPaths::standard()
        .unwrap_or_else(|error| startup_failed("Caskade couldn't find its data folders", &error));
    if let Err(error) = paths.create() {
        startup_failed("Caskade couldn't create its data folders", &error);
    }
    // `--restore <backup>` (#192): staged now, applied as the library opens.
    if let Some(archive) = caskade_app::recovery::restore_arg(
        &std::env::args().collect::<Vec<_>>(),
        &std::env::current_dir().unwrap_or_default(),
    ) && let Err(error) =
        caskade_app::runtime::block_on(caskade_app::recovery::stage(&paths, &archive))
    {
        startup_failed("Caskade couldn't restore the backup", &error);
    }
    let artwork = caskade_core::artwork::ArtworkCache::new(paths.cache.join("artwork"))
        .inspect_err(|error| tracing::error!(%error, "artwork cache unavailable"))
        .ok();
    // The library, accounts, settings and launch service; nothing works
    // without them. Dialogs come with BLITZ-9 (#88): until then the reason
    // goes to the terminal and the log.
    let core = caskade_app::start::open(&paths, artwork, &shell::sink())
        .unwrap_or_else(|error| startup_failed("Caskade couldn't open its library", &error));
    let state_file = window_state::file(&paths);
    let restored = window_state::WindowState::load(&state_file);
    let _ = KEEPER.set(shell::Keeper::new(state_file, restored));
    let window = attributes(restored);
    // Everything else starts while the window shows.
    shell::start(core, |services| {
        caskade_app::launch_cli::server::listen(
            caskade_app::launch_cli::server::Services {
                accounts: services.accounts.clone(),
                store: services.store.clone(),
                launches: services.launch.clone(),
                saves: Some(services.save_versions.clone()),
            },
            caskade_app::launch_cli::Endpoint::for_paths(&services.paths),
        );
        smoke::after_start(services);
    });
    let restarts = caskade_player::recovery::Restarts::new(paths.cache.join("native-restarts"));
    // Nothing the window held is used after a panic: the process restarts.
    let ui = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dioxus_native::launch_cfg(app, vec![], vec![Box::new(window)]);
    }));
    // The window closed, or its UI panicked: save where it was and stop
    // what runs with Caskade.
    if let Some(keeper) = KEEPER.get() {
        keeper.save();
    }
    shell::stop();
    if ui.is_err() {
        restart_after_panic(&restarts);
    }
    tracing::info!("Caskade (native) quit");
}

/// The UI panicked (`background::log_panics` logged where). One event loop
/// is all a process gets, so the window comes back as a new process, within
/// the restart budget the webviews share (`caskade_player::recovery`);
/// past it Caskade stops rather than loop.
fn restart_after_panic(restarts: &caskade_player::recovery::Restarts) -> ! {
    if !restarts.claim(std::time::SystemTime::now()) {
        tracing::error!("the window keeps failing: Caskade won't restart it again");
        eprintln!(
            "Caskade's window keeps failing, so it stopped. Start Caskade again, or see the log."
        );
        std::process::exit(1);
    }
    tracing::warn!("restarting after the window failed");
    let started = std::env::current_exe().and_then(|program| {
        std::process::Command::new(program)
            .args(std::env::args_os().skip(1))
            .spawn()
    });
    match started {
        Ok(_) => std::process::exit(0),
        Err(error) => {
            tracing::error!(%error, "Caskade couldn't restart");
            std::process::exit(1)
        }
    }
}

/// The main window's attributes, at its restored place and size.
fn attributes(restored: Option<window_state::WindowState>) -> dioxus_native::WindowAttributes {
    use dioxus_native::winit::dpi::{PhysicalPosition, PhysicalSize};
    let window = dioxus_native::WindowAttributes::default()
        .with_title("Caskade")
        .with_min_surface_size(dioxus_native::LogicalSize::new(
            f64::from(window_state::MIN_WIDTH),
            f64::from(window_state::MIN_HEIGHT),
        ));
    match restored {
        Some(state) => window
            .with_surface_size(PhysicalSize::new(state.width, state.height))
            .with_position(PhysicalPosition::new(state.x, state.y))
            .with_maximized(state.maximized),
        None => window.with_surface_size(dioxus_native::LogicalSize::new(1280.0, 800.0)),
    }
}

/// The main window's root: it follows the window (its state, game focus,
/// a quit asked for) and holds the views, which come with the UI work
/// (#96 onwards).
fn app() -> Element {
    let window = dioxus_native::use_window();
    use_hook({
        let window = window.clone();
        move || window_ready(window)
    });
    dioxus_native::use_window_event(move |event, event_loop| {
        if let Some(keeper) = KEEPER.get() {
            keeper.follow(window.as_ref(), event);
        }
        shell::quit_if_asked(event_loop);
        if matches!(
            event,
            dioxus_native::winit::event::WindowEvent::RedrawRequested
        ) {
            smoke::panic_in_ui();
        }
    });
    rsx! {
        div { style: "padding: 24px; font-family: sans-serif;", "Caskade" }
    }
}

/// The window is up: games may move it once the services are there.
fn window_ready(window: Arc<dyn dioxus_native::winit::window::Window>) {
    // A place saved on a monitor that's gone: back to the top left corner
    // of the screen there is.
    if window.current_monitor().is_none() {
        tracing::info!("the saved window place is off every screen");
        window.set_outer_position(dioxus_native::winit::dpi::PhysicalPosition::new(0, 0).into());
    }
    smoke::window_ready(window.clone());
    caskade_app::runtime::spawn(async move {
        let services = loop {
            if let Some(services) = shell::services() {
                break services;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        caskade_app::game_focus::follow(
            shell::NativeWindow(window),
            &services.bus,
            services.settings.clone(),
            services.store.clone(),
            services.launch.clone(),
        );
    });
}

fn logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
}

/// Ends a window process (`--web-game`, `--video`) with its window's code.
fn exit_window(outcome: Result<i32, String>, failed: &str) -> ! {
    match outcome {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            tracing::error!(%error, "{failed}");
            std::process::exit(1)
        }
    }
}

/// Ends the process before the window opens.
fn startup_failed(what: &str, error: &dyn std::fmt::Display) -> ! {
    tracing::error!(%error, "{what}");
    eprintln!("{what}: {error}");
    std::process::exit(1)
}
