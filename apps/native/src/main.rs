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
mod accounts;
// Activity's jobs (#101).
mod activity;
// The anatomy catalogue (#90); the components (#93) add to it.
mod anatomy;
// Collections and a collection's page (#102).
mod collections;
// Console mode, for a TV and a pad (#107).
mod console;
// The data hooks (#89).
mod data;
mod dialogs;
// Files dragged onto the window: the library installs them (#99).
mod drops;
mod flows;
mod forwarded;
// The frame around the views: sidebar, top bar, status bar (#96).
mod frame;
#[cfg(any(debug_assertions, feature = "gallery"))]
mod gallery;
// A game's page, and what Play asks before a game starts (#100).
mod game;
// Games' menus, quick looks and the dialogs they open (#99).
mod game_menu;
mod home;
mod i18n;
mod input;
mod launch;
mod library;
mod onboarding;
mod paste;
// Plugins' drawn UI (#109).
mod plugins;
// Navigation between the pages (#96), in place of the webview's router.
mod router;
mod settings;
mod shell;
#[cfg(feature = "shots")]
mod shots;
mod smoke;
mod state_file;
mod store;
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
    if let Some(window) = caskade_player::Window::from_args(std::env::args_os()) {
        caskade_player::webkit::prefer_gpu();
        logging();
        window.run();
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
    let paths = caskade_app::recovery::prepare_paths();
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
            caskade_app::launch_cli::server::Services::of(services),
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
        // Console mode (#107) closes as the setting says, and nothing in its
        // UI holds a close (#58).
        match tray::state().close(crate::console::active()) {
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
/// a quit asked for) and holds the views.
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
        let services = shell::services_ready().await;
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

/// Logs to stderr and the daily files, as the Tauri app does.
fn logging() {
    caskade_app::logging::init("Caskade (native)", env!("CARGO_PKG_VERSION"));
}

/// The library store couldn't open: the app layer's dialogs say why and
/// offer a way back (`caskade_app::recovery::library_failed`).
fn library_failed(error: &caskade_core::Error, paths: &caskade_core::AppPaths) -> ! {
    caskade_app::recovery::library_failed(error, paths, &StartupDialogs);
    std::process::exit(1)
}

/// The library error dialog's dialogs, which block, before the window opens.
struct StartupDialogs;

impl caskade_app::recovery::StartupDialogs for StartupDialogs {
    fn ask(&self, text: &str, buttons: &[String]) -> Option<usize> {
        dialogs::ask("Caskade", text, buttons)
    }

    fn pick_file(&self, title: &str, filter: (&str, &[&str])) -> Option<std::path::PathBuf> {
        dialogs::pick_file(title, Some(filter))
    }

    fn cant_update(&self) -> Option<caskade_core::i18n::Message> {
        updater::unavailable()
    }

    fn update(&self) -> Result<Option<String>, String> {
        caskade_app::runtime::block_on(updater::install_now())
    }

    fn restart(&self) {
        restart()
    }

    fn quit(&self) {
        std::process::exit(1)
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
