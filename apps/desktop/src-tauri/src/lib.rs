//! The Caskade desktop app: a thin Tauri 2 adapter over `caskade-app` and
//! `caskade-core`. Commands, events, windows and URI schemes live here;
//! the app layer and the core hold everything else. The webview gets no `fs` or `shell`
//! plugin.

pub mod addon_tool;
pub mod asset;
pub mod background;
pub mod bindings;
pub mod commands;
pub mod deep_link;
pub mod dev_plugins;
#[cfg(feature = "e2e")]
pub mod e2e;
pub mod events;
pub mod game_focus;
pub mod launch_cli;
pub mod logging;
pub mod main_window;
pub mod navigation;
pub mod permissions;
pub mod plugin_scheme;
pub mod recovery;
pub mod theme_scheme;
pub mod tray;
pub mod video_player;
pub mod web_player;
pub mod webkit;
pub mod webview_recovery;
pub mod window_state;

// The app layer's, under the paths the adapter has always used.
pub use caskade_app::{discovery, error, onboarding, torrent_links};

/// Builds and runs the app.
pub fn run() {
    // `caskade --elevated …` (#255): the administrator steps, in the
    // process UAC started; nothing else.
    #[cfg(windows)]
    {
        let args: Vec<String> = std::env::args().collect();
        if args
            .iter()
            .any(|arg| arg == caskade_streaming::elevation::windows::ELEVATED_ARG)
        {
            std::process::exit(caskade_streaming::elevation::windows::perform(&args));
        }
    }
    // `caskade --launch <game>` (#256): hand the launch to the running
    // Caskade and wait for the game, not the app. Before logging: the
    // caller isn't Caskade starting, and the app logs the launch itself.
    if let Some(request) = launch_cli::from_args(std::env::args()) {
        match request {
            Ok(request) => launch_cli::main(request),
            Err(usage) => {
                eprintln!("{usage}");
                std::process::exit(launch_cli::code::USAGE);
            }
        }
    }
    logging::init();
    // Panics reach the log too, not only stderr.
    background::log_panics();
    // Before anything runs on it.
    async_runtime();
    // First, as it may restart the process.
    webkit::prefer_gpu();
    // The OS's language, until the Language setting (#213) picks one.
    caskade_core::i18n::set_locale(caskade_core::i18n::SYSTEM);
    // `caskade --web-game <url>`: a web game's window, not the app.
    if let Some(options) = web_player::Options::from_args(std::env::args_os()) {
        exit_window(web_player::run(options), "the game window couldn't open");
    }
    // `caskade --video <id>`: a game's video, not the app.
    if let Some(options) = video_player::Options::from_args(std::env::args_os()) {
        exit_window(video_player::run(options), "the video window couldn't open");
    }
    let specta = bindings::builder();
    #[cfg(debug_assertions)]
    if let Err(error) = bindings::export(&bindings::default_path()) {
        tracing::warn!(%error, "could not refresh the TypeScript bindings");
    }
    let paths = caskade_core::AppPaths::standard()
        .unwrap_or_else(|error| startup_failed("Caskade couldn't find its data folders", &error));
    if let Err(error) = paths.create() {
        startup_failed("Caskade couldn't create its data folders", &error);
    }
    // `caskade --restore <backup>` (#192): staged now, applied as the
    // library opens.
    if let Some(archive) = recovery::restore_arg(
        &std::env::args().collect::<Vec<_>>(),
        &std::env::current_dir().unwrap_or_default(),
    ) && let Err(error) = tauri::async_runtime::block_on(recovery::stage(&paths, &archive))
    {
        startup_failed("Caskade couldn't restore the backup", &error);
    }
    let setup_paths = paths.clone();
    // One cache, shared by every clone: without it no artwork shows;
    // everything else works.
    let artwork = caskade_core::artwork::ArtworkCache::new(paths.cache.join("artwork"))
        .inspect_err(|error| tracing::error!(%error, "artwork cache unavailable"))
        .ok();
    theme_scheme::register(plugin_scheme::register(asset::register(
        tauri::Builder::default(),
        artwork.clone(),
    )))
    .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
        // `caskade dev <folder>` while Caskade runs loads it here (#131).
        if let Some(folder) = dev_plugins::folder_from_args(&args, std::path::Path::new(&cwd)) {
            dev_plugins::load_in_background(app, folder);
        }
        // A magnet link or torrent file the OS opened with Caskade (#159).
        if let Some(input) = torrent_links::input_from_args(&args, std::path::Path::new(&cwd)) {
            deep_link::queue_in_background(app, input);
        }
        main_window::raise(app);
        // `caskade --console` while Caskade runs switches it (#148).
        if commands::console::wants_console(&args)
            && let Err(error) = commands::console::requested(app)
        {
            tracing::warn!(%error, "couldn't switch to console mode");
        }
    }))
    .plugin(tauri_plugin_deep_link::init())
    .plugin(window_state::plugin())
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .on_page_load(|webview, payload| {
        if payload.event() == tauri::webview::PageLoadEvent::Finished {
            webkit::mark_cpu_paint(webview);
        }
    })
    // Every command checks the signed-in account's permissions (#205).
    .invoke_handler(permissions::gate(specta.invoke_handler()))
    .setup(move |app| {
        use tauri::Manager;
        specta.mount_events(app);
        // The main window, with its navigation guard (CSK-14), brought back
        // when its webview loses its processes (#3).
        let main = navigation::create_main_window(app.handle())?;
        #[cfg(windows)]
        main_window::watch_focus(&main);
        webview_recovery::watch(&main, setup_paths.cache.join("webview-restarts"));
        let events = events::sink(app.handle().clone());
        // The library, accounts, settings and launch service (the app
        // layer's). A restore staged before the restart is applied first
        // (#192).
        let core = match caskade_app::start::open(&setup_paths, artwork.clone(), &events) {
            Ok(core) => core,
            Err(error) => {
                // Nothing works without the library: say so, then quit.
                library_failed(app.handle(), &error, &setup_paths);
                return Ok(());
            }
        };
        app.manage(core.bus.clone());
        app.manage(core.accounts.clone());
        // Gamepads for console mode (#146).
        commands::input::setup(app.handle(), core.bus.clone());
        // Settings → Statistics (#156): samples only while it is open.
        commands::system_stats::setup(app.handle());
        // Notifications (#198), which also note Caskade's own updates.
        commands::notifications::setup(app.handle(), core.notifications.clone());
        let settings = core.settings.clone();
        // Console mode (#148): `--console`, gamescope or the setting.
        commands::console::setup(
            app.handle(),
            commands::console::Startup::read(&std::env::args().collect::<Vec<_>>()),
            &settings,
        );
        // The language (#213): the setting's, before the window loads.
        commands::i18n::setup(app.handle(), &settings);
        // The tray icon, and the close button hiding to it.
        tray::setup(app.handle(), &settings);
        // Caskade's own updates: checked in the background, installed
        // when the user asks.
        app.manage(commands::app_update::AppUpdater::<
            tauri_plugin_updater::Update,
        >::new(
            app.package_info().version.to_string(),
            commands::app_update::unavailable_reason(tauri::utils::platform::bundle_type()),
            commands::app_update::ENDPOINT,
        ));
        background::supervise("app updates", {
            let (app, settings) = (app.handle().clone(), settings.clone());
            move || commands::app_update::watch(app.clone(), settings.clone())
        });
        window_state::save_on_change(app.handle());
        // Start minimized (#114): a login start stays in the tray.
        let general: caskade_core::settings::GeneralSettings =
            caskade_app::runtime::block_on(settings.read_or_default("core.general"));
        if !window_state::starts_in_tray(
            app.handle(),
            &std::env::args().collect::<Vec<_>>(),
            &general,
        ) {
            window_state::show(app.handle());
        }
        // Everything else the app runs, wired by the app layer; windows
        // that open over Caskade's (web games, streams) ask the main
        // window where it is.
        let (placement, activation) = (app.handle().clone(), app.handle().clone());
        let services = caskade_app::start::start(
            core,
            caskade_app::start::Shell {
                events,
                placement: std::sync::Arc::new(move || main_window::placement(&placement)),
                activation: std::sync::Arc::new(move || main_window::activation(&activation)),
                release_key: commands::addons::release_key(app.handle()),
            },
        );
        // Following the running game with the window (#149).
        game_focus::follow(
            app.handle().clone(),
            &services.bus,
            services.settings.clone(),
            services.store.clone(),
            services.launch.clone(),
        );
        let paths = services.paths.clone();
        manage(app, services);
        deep_link::listen(app.handle());
        app.manage(commands::video::VideoWindow::default());
        app.manage(dev_plugins::DevPlugins::default());
        // `caskade dev <folder>`: load and watch a plugin in place (#131).
        let args: Vec<String> = std::env::args().collect();
        if let Some(folder) =
            dev_plugins::folder_from_args(&args, &std::env::current_dir().unwrap_or_default())
        {
            dev_plugins::load_in_background(app.handle(), folder);
        }
        // `caskade --launch` from other programs (#256), once the library
        // and the launch service are there to take them.
        launch_cli::listen(app.handle(), launch_cli::Endpoint::for_paths(&paths));
        Ok(())
    })
    .build(tauri::generate_context!())
    .expect("error while building Caskade")
    .run(run_event);
}

/// Most workers the app's async runtime starts. Tauri's own starts one per
/// core (24 on a desktop we measured), each a thread with its own stack and
/// malloc cache; the app's async work waits on I/O, and its heavy work runs
/// on blocking threads, which come and go as needed.
const ASYNC_WORKERS: usize = 4;

/// Gives Tauri the app layer's runtime, with at most [`ASYNC_WORKERS`]
/// workers, so both spawn onto the same one.
fn async_runtime() {
    match caskade_app::runtime::init(ASYNC_WORKERS) {
        Ok(handle) => tauri::async_runtime::set(handle),
        Err(error) => tracing::warn!(%error, "Tauri's own async runtime is used"),
    }
}

/// The app's run events: the streaming host stops when Caskade exits
/// (#257), then the tray's own handling.
pub fn run_event<R: tauri::Runtime>(app: &tauri::AppHandle<R>, event: tauri::RunEvent) {
    if let tauri::RunEvent::Exit = event {
        commands::streaming::on_exit(app);
    }
    tray::on_run_event(app, event);
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

/// Ends the process before Tauri starts, when there is nowhere to keep the
/// library.
fn startup_failed(what: &str, error: &dyn std::fmt::Display) -> ! {
    tracing::error!(%error, "{what}");
    eprintln!("{what}: {error}");
    std::process::exit(1)
}

/// The library store couldn't open: a dialog says why and offers to restore
/// a backup (#192), or to update when a newer Caskade wrote the library;
/// otherwise closing it quits.
fn library_failed<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    error: &caskade_core::Error,
    paths: &caskade_core::AppPaths,
) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    tracing::error!(%error, "library unavailable");
    eprintln!("Caskade couldn't open its library: {error}");
    let report = caskade_core::error::ErrorReport::from(error);
    if report
        .hints
        .contains(&caskade_core::error::RecoveryHint::UpdateCaskade)
    {
        // A backup is older than the library: restoring one would lose
        // what the newer Caskade added.
        return library_newer(app, report.summary);
    }
    let (handle, paths) = (app.clone(), paths.clone());
    app.dialog()
        .message(caskade_core::tr!(
            "Caskade couldn't open its library.\n\n{error}\n\nYou can restore it from a backup, or start Caskade with --restore <backup file>.",
            error = error
        ))
        .title("Caskade")
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::OkCancelCustom(
            caskade_core::tr!("Restore from backup…"),
            caskade_core::tr!("Quit"),
        ))
        .show(move |restore| {
            if restore {
                recovery::restore_from_dialog(&handle, paths);
            } else {
                handle.exit(1);
            }
        });
}

/// A newer Caskade changed the library in a way this one can't read: a
/// copy that updates itself offers to, then restarts into the update.
fn library_newer<R: tauri::Runtime>(app: &tauri::AppHandle<R>, summary: String) {
    use tauri_plugin_dialog::MessageDialogButtons;
    let handle = app.clone();
    match commands::app_update::unavailable_reason(tauri::utils::platform::bundle_type()) {
        Some(reason) => startup_dialog(app, format!("{summary}\n\n{}", reason.render()))
            .buttons(MessageDialogButtons::OkCustom(caskade_core::tr!("Quit")))
            .show(move |_| handle.exit(1)),
        None => startup_dialog(
            app,
            caskade_core::tr!("{summary}\n\nUpdate Caskade to open it.", summary = summary),
        )
        .buttons(MessageDialogButtons::OkCancelCustom(
            caskade_core::tr!("Update Caskade"),
            caskade_core::tr!("Quit"),
        ))
        .show(move |update| {
            if update {
                update_for_library(&handle);
            } else {
                handle.exit(1);
            }
        }),
    }
}

/// An error dialog before the main window shows.
fn startup_dialog<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    message: String,
) -> tauri_plugin_dialog::MessageDialogBuilder<R> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    app.dialog()
        .message(message)
        .title("Caskade")
        .kind(MessageDialogKind::Error)
}

/// Installs the newest release from the startup dialog, then restarts into
/// it; a failed update says why and offers to try again.
fn update_for_library<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogKind};
    // Nothing else shows while it downloads; closing this changes nothing.
    startup_dialog(
        app,
        caskade_core::tr!(
            "Updating Caskade…\n\nCaskade restarts by itself once the update is installed."
        ),
    )
    .kind(MessageDialogKind::Info)
    .show(|_| {});
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let quit = handle.clone();
        match commands::app_update::install_now(&handle).await {
            Ok(Some(_)) => tray::restart(&handle),
            Ok(None) => startup_dialog(
                &handle,
                caskade_core::tr!(
                    "No released version of Caskade can open this library yet: a development build changed it."
                ),
            )
            .buttons(MessageDialogButtons::OkCustom(caskade_core::tr!("Quit")))
            .show(move |_| quit.exit(1)),
            Err(error) => {
                tracing::warn!(%error, "update from the startup dialog failed");
                startup_dialog(&handle, error)
                    .buttons(MessageDialogButtons::OkCancelCustom(
                        caskade_core::tr!("Try again"),
                        caskade_core::tr!("Quit"),
                    ))
                    .show(move |again| {
                        if again {
                            update_for_library(&quit);
                        } else {
                            quit.exit(1);
                        }
                    })
            }
        }
    });
}

/// Puts every service the app layer started into Tauri's state, where
/// the commands find them.
fn manage<R: tauri::Runtime>(app: &tauri::App<R>, services: caskade_app::start::Services) {
    use tauri::Manager;
    fn some<R: tauri::Runtime, T: Send + Sync + 'static>(app: &tauri::App<R>, value: Option<T>) {
        if let Some(value) = value {
            app.manage(value);
        }
    }
    let caskade_app::start::Services {
        paths: _,
        bus: _,
        accounts: _,
        notifications: _,
        store,
        settings,
        launch,
        storage,
        storage_source,
        install,
        install_requests,
        discovery,
        discovery_controller,
        emulation,
        emulator_setup,
        tools,
        streaming_host,
        stream_hosts,
        streaming_client,
        edit_artwork,
        web,
        metadata,
        hasheous,
        ludusavi,
        ports,
        igdb,
        updates,
        romm,
        save_versions,
        addon_folders,
        addon_jobs,
        plugins,
        plugin_hooks,
        themes,
        catalogue,
        addon_media,
        backups,
        store_accounts,
        achievements,
        achievement_icons,
        devices,
        linked_pairing,
        transfers,
        shares,
        remote_play,
        sync,
        presence,
        peer_backups,
    } = services;
    app.manage(store);
    app.manage(settings);
    app.manage(launch);
    some(app, storage);
    some(app, storage_source);
    if let Some(install) = install {
        app.manage(commands::install_picker::native(app.handle().clone()));
        app.manage(install);
    }
    some(app, install_requests);
    app.manage(discovery);
    app.manage(discovery_controller);
    app.manage(emulation);
    app.manage(emulator_setup);
    some(app, tools);
    some(app, streaming_host);
    some(app, stream_hosts);
    some(app, streaming_client);
    app.manage(edit_artwork);
    app.manage(web);
    app.manage(metadata);
    app.manage(hasheous);
    app.manage(ludusavi);
    app.manage(ports);
    app.manage(igdb);
    some(app, updates);
    some(app, romm);
    app.manage(save_versions);
    app.manage(addon_folders);
    app.manage(addon_jobs);
    some(app, plugins);
    some(app, plugin_hooks);
    app.manage(themes);
    some(app, catalogue);
    some(app, addon_media);
    app.manage(backups);
    app.manage(store_accounts);
    app.manage(achievements);
    app.manage(achievement_icons);
    app.manage(devices);
    app.manage(linked_pairing);
    some(app, transfers);
    some(app, shares);
    app.manage(remote_play);
    app.manage(sync);
    app.manage(presence);
    app.manage(peer_backups);
    app.manage(commands::sync::OpenedPairingLink::default());
}
