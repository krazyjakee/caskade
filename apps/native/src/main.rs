//! Caskade's native app (#87): the main window rendered by Blitz, running
//! the same application layer as the Tauri app (`caskade-app`). It ships
//! beside the Tauri app until it reaches parity (BLITZ-33).
//!
//! One winit event loop (Blitz's shell) owns the main thread. Web games and
//! videos open in windows of their own, in a process of their own: this
//! executable started again with `--web-game` or `--video`
//! (`caskade-player`), so no webview shares the loop.

// What screen readers get, as text, for the tests (#83, #93).
#[cfg(test)]
mod a11y_snapshot;
// The anatomy catalogue (#90); the components (#93) add to it.
mod anatomy;
// The data hooks (#89); the views (#96 onwards) use them.
mod data;
mod dialogs;
mod flows;
// Files dragged onto the window: the library installs them (#99).
mod drops;
mod forwarded;
// The frame around the views: sidebar, top bar, status bar (#96).
mod frame;
#[cfg(any(debug_assertions, feature = "gallery"))]
mod gallery;
// Games' menus, quick looks and the dialogs they open (#99).
mod game_menu;
mod i18n;
mod input;
mod launch;
mod library;
// Navigation between the pages (#96), in place of the webview's router.
mod router;
mod settings;
mod shell;
#[cfg(feature = "shots")]
mod shots;
mod smoke;
mod theme;
mod tray;
// The component library (#93).
mod ui;
mod updater;
mod window_state;

use std::sync::{Arc, OnceLock};

use dioxus_native::prelude::*;

/// Most workers the app's async runtime starts, as the Tauri app.
const ASYNC_WORKERS: usize = 4;

/// The main window's state keeper, for the root component and the exit.
static KEEPER: OnceLock<shell::Keeper> = OnceLock::new();

fn main() {
    // `--shots <folder>`: the reference screens' screenshots (#90). A
    // build without them says so instead of starting the app.
    #[cfg(not(feature = "shots"))]
    if std::env::args().any(|arg| arg == "--shots") {
        eprintln!("this build has no screenshots: build with --features shots");
        std::process::exit(2);
    }
    #[cfg(feature = "shots")]
    if let Some((out, themes)) = shots::requested() {
        logging();
        if let Err(error) = shots::run(&out, themes.as_deref()) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
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
    // `--gallery`: the component gallery (#93), not the app.
    #[cfg(any(debug_assertions, feature = "gallery"))]
    if gallery::requested() {
        gallery::run();
        return;
    }
    #[cfg(not(any(debug_assertions, feature = "gallery")))]
    if std::env::args().any(|arg| arg == "--gallery") {
        eprintln!("this build has no gallery: build with --features gallery");
        std::process::exit(2);
    }
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
    // One Caskade per user: a second start hands its arguments (links,
    // `--console`, a torrent file) to the running one and ends.
    let instance = match caskade_app::runtime::block_on(caskade_app::single_instance::claim(
        &caskade_app::single_instance::endpoint(&paths),
        &caskade_app::single_instance::Forwarded::this_process(),
    )) {
        Ok(caskade_app::single_instance::Claim::First(listener)) => Some(listener),
        Ok(caskade_app::single_instance::Claim::Forwarded) => {
            tracing::info!("Caskade already runs: it has this start's arguments");
            std::process::exit(0);
        }
        Err(error) => {
            tracing::warn!(%error, "couldn't tell whether Caskade already runs");
            None
        }
    };
    let artwork = caskade_core::artwork::ArtworkCache::new(paths.cache.join("artwork"))
        .inspect_err(|error| tracing::error!(%error, "artwork cache unavailable"))
        .ok();
    // The library, accounts, settings and launch service; nothing works
    // without them.
    let core = caskade_app::start::open(&paths, artwork, &shell::sink())
        .unwrap_or_else(|error| library_failed(&error, &paths));
    let state_file = window_state::file(&paths);
    let restored = window_state::WindowState::load(&state_file);
    let _ = KEEPER.set(shell::Keeper::new(state_file, restored));
    // The tray icon (Linux: before the window, on its own thread), and
    // whether a login start stays in it (#114).
    tray::handle_menu_events();
    #[cfg(target_os = "linux")]
    tray::start();
    tray::follow(core.settings.clone());
    // Caskade's own updates: checked in the background.
    updater::watch(core.settings.clone());
    smoke::settings(&core.settings);
    let general: caskade_core::settings::GeneralSettings =
        caskade_app::runtime::block_on(core.settings.read_or_default("core.general"));
    let hidden =
        caskade_core::login_item::starts_hidden(&std::env::args().collect::<Vec<_>>(), &general)
            && (cfg!(not(target_os = "linux")) || tray::state().shown());
    if hidden {
        tracing::info!("started at login: Caskade stays in the tray");
    }
    let window = attributes(restored).with_visible(!hidden);
    if let Some(listener) = instance {
        caskade_app::single_instance::serve(listener, forwarded::take);
    }
    // The look of the first frame: the theme, the user's CSS and the
    // Appearance settings (#90).
    let look = theme::first(&core.themes, &core.settings);
    // Everything else starts while the window shows.
    shell::start(core, |services| {
        theme::follow(services);
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
        forwarded::take_own();
        // `caskade://` links: this copy takes them when nothing else does.
        caskade_app::runtime::spawn_blocking(|| {
            if caskade_app::link_handler::wanted(&std::env::args().collect::<Vec<_>>()) {
                match caskade_app::link_handler::register() {
                    Ok(()) => tracing::info!("caskade:// links open this copy"),
                    Err(error) => tracing::warn!(%error, "caskade:// links weren't registered"),
                }
            }
        });
    });
    let restarts = caskade_player::recovery::Restarts::new(paths.cache.join("native-restarts"));
    // Nothing the window held is used after a panic: the process restarts.
    let ui = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        launch::run(app, window, &look, NativeShell);
    }));
    // The window closed, or its UI panicked: save where it was and stop
    // what runs with Caskade.
    if let Some(keeper) = KEEPER.get() {
        keeper.save();
    }
    shell::stop();
    tray::hide_before_exit();
    if let Err(panic) = ui {
        if no_renderer(caskade_app::background::panic_message(&*panic)) {
            tracing::error!("no GPU renderer for the window: Caskade won't restart it");
            eprintln!(
                "Caskade couldn't draw its window: no graphics adapter it can use was found. \
                 Install your GPU's driver, or Mesa's software rendering (lavapipe), and start \
                 Caskade again."
            );
            std::process::exit(1);
        }
        restart_after_panic(&restarts);
    }
    tracing::info!("Caskade (native) quit");
}

/// Whether a panic is the renderer finding no graphics adapter or surface
/// (wgpu's errors, as Blitz's vello_hybrid renderer reports them). A new
/// process wouldn't find one either, so it doesn't restart; choosing a CPU
/// renderer then is BLITZ-31 (#110).
fn no_renderer(message: &str) -> bool {
    [
        "RequestAdapterError",
        "Error creating DeviceHandle",
        "WgpuCreateSurfaceError",
    ]
    .iter()
    .any(|wgpu| message.contains(wgpu))
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

/// What closing the main window does: hide it to the tray when the
/// setting says so and the icon is up, otherwise quit.
struct NativeShell;

impl launch::Shell for NativeShell {
    fn on_close(&mut self) -> launch::OnClose {
        // Console mode isn't in the native app yet (#92), and nothing in its
        // UI holds a close (#58).
        match tray::state().close(false) {
            caskade_app::tray::Close::Hide => launch::OnClose::Hide,
            // Settings' unsaved edits ask before a quit (#103).
            caskade_app::tray::Close::Go | caskade_app::tray::Close::Held(_)
                if settings::holds_close() =>
            {
                launch::OnClose::Hold
            }
            caskade_app::tray::Close::Go | caskade_app::tray::Close::Held(_) => {
                launch::OnClose::Quit
            }
        }
    }
}

/// The main window's attributes, at its restored place and size.
fn attributes(restored: Option<window_state::WindowState>) -> dioxus_native::WindowAttributes {
    use dioxus_native::winit::dpi::{PhysicalPosition, PhysicalSize};
    let window = dioxus_native::WindowAttributes::default()
        .with_title("Caskade")
        // Caskade draws the title bar on Windows and Linux (`frame::window`,
        // #96).
        .with_decorations(!frame::window::CUSTOM_FRAME)
        .with_min_surface_size(dioxus_native::LogicalSize::new(
            f64::from(window_state::MIN_WIDTH),
            f64::from(window_state::MIN_HEIGHT),
        ));
    // macOS keeps its traffic lights, over the top of the sidebar.
    #[cfg(target_os = "macos")]
    let window = {
        use dioxus_native::winit::platform::macos::WindowAttributesMacOS;
        window.with_platform_attributes(Box::new(
            WindowAttributesMacOS::default()
                .with_titlebar_transparent(true)
                .with_title_hidden(true)
                .with_fullsize_content_view(true),
        ))
    };
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
    // The UI's language, before any text (#91).
    i18n::use_language();
    let window = dioxus_native::use_window();
    use_hook({
        let window = window.clone();
        move || window_ready(window)
    });
    dioxus_native::use_window_event(move |event, event_loop| {
        if let Some(keeper) = KEEPER.get() {
            keeper.follow(window.as_ref(), event);
        }
        // Themes with a light and a dark scheme follow the OS's (#90).
        if let dioxus_native::winit::event::WindowEvent::ThemeChanged(os) = event {
            theme::prefers_light(*os == dioxus_native::winit::window::Theme::Light);
        }
        let _ = event_loop;
        if matches!(
            event,
            dioxus_native::winit::event::WindowEvent::RedrawRequested
        ) {
            smoke::panic_in_ui();
        }
    });
    // What the views read (#89), once the services are there.
    rsx! {
        // Keys, pads and focus (#92), around everything: it stays mounted
        // while the data below it starts.
        input::InputRoot {
            data::DataRoot {
                // The library's scroll benchmark, when asked for (#94).
                {library::bench::screen()}
                frame::Frame {}
            }
        }
    }
}

/// The window is up: games may move it once the services are there.
fn window_ready(window: Arc<dyn dioxus_native::winit::window::Window>) {
    shell::set_main_window(window.clone());
    theme::prefers_light(window.theme() == Some(dioxus_native::winit::window::Theme::Light));
    // macOS: the menu bar's menus, on the main thread.
    #[cfg(target_os = "macos")]
    tray::app_menu();
    // Windows and macOS: the tray icon, here on the main thread.
    #[cfg(not(target_os = "linux"))]
    if !tray::start_here() && !window.is_visible().unwrap_or(true) {
        // A login start without an icon to come back from shows the window.
        window.set_visible(true);
    }
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
        // Pads: their actions reach the input root while the window has
        // focus (#92).
        input::start_pads(services, window.clone());
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

/// The library store couldn't open: a dialog says why and offers to restore
/// a backup (#192), or, when a newer Caskade wrote the library, to update;
/// otherwise Caskade quits.
fn library_failed(error: &caskade_core::Error, paths: &caskade_core::AppPaths) -> ! {
    tracing::error!(%error, "library unavailable");
    eprintln!("Caskade couldn't open its library: {error}");
    let report = caskade_core::error::ErrorReport::from(error);
    if report
        .hints
        .contains(&caskade_core::error::RecoveryHint::UpdateCaskade)
    {
        // A backup is older than the library: restoring one would lose what
        // the newer Caskade added.
        updater::update_for_library(report.summary);
    }
    let text = caskade_core::tr!(
        "Caskade couldn't open its library.\n\n{error}\n\nYou can restore it from a backup, or start Caskade with --restore <backup file>.",
        error = error
    );
    let restore = caskade_core::tr!("Restore from backup…");
    if dialogs::ask(
        "Caskade",
        &text,
        dialogs::Kind::Error,
        &[restore, caskade_core::tr!("Quit")],
    ) != Some(0)
    {
        std::process::exit(1);
    }
    let Some(archive) = dialogs::pick_file(
        &caskade_core::tr!("Choose a Caskade backup"),
        Some((&caskade_core::tr!("Caskade backups"), &["zip"])),
    ) else {
        std::process::exit(1);
    };
    match caskade_app::runtime::block_on(caskade_app::recovery::stage(paths, &archive)) {
        Ok(()) => restart(),
        Err(error) => {
            dialogs::ask(
                "Caskade",
                &caskade_core::tr!("This backup can't be restored.\n\n{error}", error = error),
                dialogs::Kind::Error,
                &[caskade_core::tr!("Quit")],
            );
            std::process::exit(1)
        }
    }
}

/// Starts Caskade again with this start's arguments, and ends this one.
fn restart() -> ! {
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

/// Ends the process before the window opens.
fn startup_failed(what: &str, error: &dyn std::fmt::Display) -> ! {
    tracing::error!(%error, "{what}");
    eprintln!("{what}: {error}");
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_wgpu_finding_no_renderer_stops_the_restarts() {
        for message in [
            "Error creating DeviceHandle: RequestAdapterError(NotFound { .. })",
            "Error creating surface: WgpuCreateSurfaceError(CreateSurfaceError { .. })",
        ] {
            assert!(super::no_renderer(message), "{message}");
        }
        assert!(!super::no_renderer("smoke: a panic in the UI"));
        assert!(!super::no_renderer("index out of bounds"));
    }
}
