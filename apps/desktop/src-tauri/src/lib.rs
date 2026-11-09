//! The Caskade desktop app: a thin Tauri 2 adapter over `caskade-core`.
//! Commands, events, windows and URI schemes live here;
//! everything else is in the core. The webview gets no `fs` or `shell`
//! plugin.

pub mod addon_tool;
pub mod asset;
pub mod background;
pub mod bindings;
pub mod commands;
pub mod deep_link;
pub mod dev_plugins;
pub mod discovery;
#[cfg(feature = "e2e")]
pub mod e2e;
pub mod error;
pub mod events;
pub mod game_focus;
pub mod launch_cli;
pub mod logging;
pub mod main_window;
pub mod navigation;
pub mod onboarding;
pub mod permissions;
pub mod plugin_scheme;
pub mod recovery;
pub mod theme_scheme;
pub mod torrent_links;
pub mod tray;
pub mod video_player;
pub mod web_player;
pub mod webkit;
pub mod webview_recovery;
pub mod window_state;

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
        if let Some(input) =
            torrent_links::input_from_args(&args, std::path::Path::new(&cwd))
        {
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
        background::bridge(app.handle().clone());
        let bus = caskade_core::bus::Bus::default();
        events::bridge(app.handle().clone(), &bus);
        app.manage(bus.clone());
        // Gamepads for console mode (#146).
        commands::input::setup(app.handle(), bus.clone());
        // Settings → Statistics (#156): samples only while it is open.
        commands::system_stats::setup(app.handle());
        // The library store, then the launch service, which first
        // re-attaches to games still running from a previous session.
        // A restore staged before the restart is applied first (#192).
        let store = match tauri::async_runtime::block_on(recovery::open_library(&setup_paths)) {
            Ok(store) => store,
            Err(error) => {
                // Nothing works without the library: say so, then quit.
                library_failed(app.handle(), &error, &setup_paths);
                return Ok(());
            }
        };
        // Local accounts (#202): a lone account without a PIN is signed
        // in, so a library without accounts set up starts as before.
        match tauri::async_runtime::block_on(caskade_core::accounts::AccountService::open(
            store.clone(),
            bus.clone(),
        )) {
            Ok(accounts) => {
                app.manage(accounts);
            }
            Err(error) => {
                library_failed(app.handle(), &error, &setup_paths);
                return Ok(());
            }
        }
        // Notifications (#198): game additions, failures and updates, kept
        // in the library.
        commands::notifications::setup(
            app.handle(),
            caskade_core::notifications::Notifications::new(store.clone()),
            &bus,
        );
        // Plugin runners (#126) attach once the plugin host exists.
        let plugin_runners = caskade_plugin_host::providers::PluginRunners::new();
        let launch = caskade_core::launch::LaunchService::new(store.clone())
            .with_bus(bus.clone())
            .with_plugins(std::sync::Arc::new(plugin_runners.clone()))
            // Age limits (#209): every launch asks the signed-in account.
            .with_play_gate(std::sync::Arc::new(
                app.state::<caskade_core::accounts::AccountService>()
                    .inner()
                    .clone(),
            ));
        match tauri::async_runtime::block_on(launch.recover()) {
            Ok(recovered) => tracing::info!(recovered = recovered.len(), "launch service ready"),
            Err(error) => tracing::error!(%error, "games from the last session not re-attached"),
        }
        let settings =
            caskade_core::settings::SettingsService::with_bus(store.clone(), bus.clone());
        if let Err(error) = settings.register_core(&setup_paths) {
            tracing::error!(%error, "core settings unavailable");
        }
        // A new device starts as its kind suits (#244): a handheld or a TV
        // in console mode, with emulators fullscreen and a larger interface.
        match tauri::async_runtime::block_on(caskade_core::device_type::settle_new_device(
            &settings,
            &store,
            caskade_core::device_type::detected(),
        )) {
            Ok(written) if !written.is_empty() => {
                tracing::info!(?written, "first settings for this kind of device")
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "first settings by device kind unavailable"),
        }
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
        window_state::show(app.handle());
        let launch = launch.with_runner_resolver(std::sync::Arc::new(
            caskade_core::install::InstallerRunnerResolver(settings.clone()),
        ));
        match caskade_core::storage::StorageService::new(
            store.clone(),
            settings.clone(),
            launch.clone(),
        ) {
            Ok(storage) => {
                let storage = storage.with_paths(setup_paths.clone());
                // Disconnected drives keep their catalogue entries. Pending
                // moves remain recoverable and are retried when their roots
                // return.
                if let Err(error) = tauri::async_runtime::block_on(storage.initialize()) {
                    tracing::warn!(%error, "storage recovery deferred until its roots are available");
                }
                app.manage(storage);
            }
            Err(error) => tracing::error!(%error, "storage locations unavailable"),
        }
        // A library from before first-run setup existed doesn't go
        // through it; a new one scans once setup is done.
        if let Err(error) =
            tauri::async_runtime::block_on(onboarding::settle_existing(&settings, &store))
        {
            tracing::warn!(%error, "first-run setup state unavailable");
        }
        // Installed-games discovery (#100). The discovery controller (#101)
        // owns the initial scan, the schedule and the watch; setup only
        // registers the sources and their settings.
        if let Err(error) = caskade_sources::register_settings(&settings) {
            tracing::error!(%error, "discovery settings unavailable");
        }
        let discovery = caskade_sources::register(
            &bus,
            store.clone(),
            caskade_discovery::HostEnv::live(),
            &caskade_discovery::Config::default(),
        );
        if let Err(error) = commands::install::setup(
            app.handle(),
            store.clone(),
            bus.clone(),
            &setup_paths,
            settings.clone(),
        ) {
            tracing::error!(%error, "installs unavailable");
        }
        deep_link::listen(app.handle());
        let mut controller =
            discovery::DiscoveryController::new(discovery.clone(), settings.clone());
        if let Some(source) = app.try_state::<caskade_sources::storage::StorageSource>() {
            controller = controller.with_storage(source.inner().clone());
        }
        controller.bridge(app.handle().clone());
        controller.start();
        app.manage(controller);
        app.manage(discovery);
        // Embedded definitions are validated before constructing the service.
        let emulation = caskade_core::emulation::EmulationService::new(
            store.clone(),
            bus.clone(),
            caskade_sources::emulation::catalog().unwrap_or_else(|error| {
                tracing::error!(%error, "shipped emulation definitions are invalid");
                caskade_core::emulation::Catalog::default()
            }),
        )
        .with_paths(&setup_paths);
        // One-click emulator installs, and
        // their update checks, which follow `core.updates`.
        let emulator_setup = caskade_core::emulation::EmulatorSetup::new(
            store.clone(),
            bus.clone(),
            emulation.clone(),
            &setup_paths,
        );
        emulator_setup.set_settings(settings.clone());
        emulator_setup.set_launch(launch.clone());
        background::supervise("emulator updates", {
            let emulator_setup = emulator_setup.clone();
            move || emulator_setup.watch()
        });
        app.manage(emulator_setup);
        // Helper tools (the streaming host and client):
        // nothing installs them yet; their update checks follow
        // `core.updates` like the emulators'.
        match caskade_sources::tools::catalog() {
            Ok(catalog) => {
                let tools = caskade_core::managed::ManagedTools::new(
                    store.clone(),
                    bus.clone(),
                    catalog,
                    &setup_paths,
                );
                tools.set_settings(settings.clone());
                background::supervise("tool updates", {
                    let tools = tools.clone();
                    move || tools.watch()
                });
                // The streaming host (#257): runs with Caskade when hosting
                // is on, and offers the library's games (#259).
                commands::streaming::setup(
                    app.handle(),
                    caskade_streaming::StreamingHost::new(
                        tools.clone(),
                        caskade_streaming::elevation::Elevation::new(&setup_paths),
                        settings.clone(),
                        &setup_paths,
                    ),
                    store.clone(),
                    &bus,
                    artwork.clone(),
                    &setup_paths,
                );
                // Devices to stream from (#262), through Caskade's own client.
                let client = caskade_streaming::StreamingClient::new(tools.clone());
                match caskade_streaming::hosts::StreamHosts::new(
                    store.clone(),
                    bus.clone(),
                    settings.clone(),
                    client.clone(),
                ) {
                    Ok(hosts) => {
                        // Play streams from them (#264).
                        let handle = app.handle().clone();
                        let focus = app.handle().clone();
                        launch.set_streamer(std::sync::Arc::new(
                            caskade_streaming::play::Streams::new(
                                client.clone(),
                                Some(hosts.clone()),
                                settings.clone(),
                            )
                            .with_placement(move || main_window::placement(&handle))
                            .with_activation(move || main_window::activation(&focus)),
                        ));
                        commands::streaming::watch_hosts(app.handle(), hosts, client)
                    }
                    Err(error) => tracing::error!(%error, "streaming hosts unavailable"),
                }
                app.manage(tools);
            }
            Err(error) => tracing::error!(%error, "shipped tool definitions are invalid"),
        }
        // Detection runs after every library scan (§5); results wait
        // for review in Settings → Emulators.
        background::supervise("emulator detection", {
            let emulation = emulation.clone();
            move || emulation.watch_library_scans()
        });
        // ROM scanners take part in every library update (#119).
        background::supervise("ROM scanners", {
            let roms = caskade_sources::RomsSource::new(emulation.scanners());
            let bus = bus.clone();
            move || roms.watch(&bus)
        });
        // Offline Hasheous dumps for the configured emulators' platforms
        // and PC games, kept current in the background.
        let hasheous = caskade_core::hasheous::Hasheous::new(setup_paths.cache.join("hasheous"));
        let ludusavi = caskade_core::ludusavi::Ludusavi::new(setup_paths.cache.join("ludusavi"));
        // Fan ports of games: Decomp Buddy's install plans, fetched daily.
        let ports = caskade_core::ports::Ports::new(setup_paths.cache.join("ports"));
        // IGDB metadata from Caskade's metadata server, cached for good.
        let igdb = caskade_core::igdb::Igdb::new(setup_paths.cache.join("igdb"));
        // Artwork the user chooses or imports in the game editor (#175).
        app.manage(commands::game_edit::EditArtwork(artwork.clone()));
        // Games IGDB has no icon for get their executable's.
        if let Some(artwork) = artwork.clone() {
            let icons = caskade_core::icons::ExeIcons::new(store.clone(), bus.clone(), artwork);
            background::supervise("executable icons", move || icons.watch());
        }
        // Web games play in the game window: this executable, started again.
        let web_player = std::env::current_exe()
            .inspect_err(|error| {
                tracing::error!(%error, "web games can't start: Caskade's executable is unknown");
            })
            .ok()
            .map(|program| {
                let handle = app.handle().clone();
                caskade_core::launch::WebPlayer::new(program, setup_paths.data.join("web"))
                    .with_settings(settings.clone())
                    // In desktop mode a web game opens over Caskade's window.
                    .with_placement(move || main_window::placement(&handle))
            });
        // Add web game: the page's details and picture; Play offline.
        let mut web = caskade_core::web::WebGames::new(store.clone()).with_bus(bus.clone());
        if let Some(artwork) = artwork.clone() {
            web = web.with_artwork(artwork);
        }
        if let Some(player) = web_player.clone() {
            web = web.with_player(player);
        }
        app.manage(web);
        // Library games matched to IGDB (RomM, Hasheous names, then
        // store ids) and filled in from it, in the background.
        let metadata = caskade_core::metadata::Metadata::new(
            store.clone(),
            bus.clone(),
            igdb.clone(),
            hasheous.clone(),
            emulation.catalog(),
            artwork.clone(),
        );
        // These go online: they run once first-run setup is done, while
        // `core.metadata.online` allows them.
        onboarding::supervise_online(settings.clone(), {
            let (hasheous, metadata) = (hasheous.clone(), metadata.clone());
            let (ludusavi, settings) = (ludusavi.clone(), settings.clone());
            let ports = ports.clone();
            let (bus, emulation, store) = (bus.clone(), emulation.clone(), store.clone());
            move || {
                vec![
                    hasheous.watch(&bus, emulation.clone(), store.clone()),
                    ludusavi.watch_settings(settings.clone()),
                    ports.watch_settings(settings.clone()),
                    metadata.watch(),
                ]
            }
        });
        // Refreshing chosen games' details (#176): its status goes to Activity.
        commands::game_edit::forward_refresh(app.handle(), &metadata);
        app.manage(metadata);
        app.manage(hasheous);
        app.manage(ludusavi);
        app.manage(ports);
        app.manage(igdb);
        if let Some(install) = app
            .try_state::<caskade_core::install::InstallService>()
            .map(|install| install.inner().clone())
        {
            // Installed ROMs map to the catalogue's platforms (#120).
            install.set_emulation(emulation.clone());
            // Games installed from GitHub are followed for new releases,
            // checked in the background and before they're played.
            let tracker = caskade_core::updates::Tracker::builtin(store.clone());
            install.set_tracker(tracker.clone());
            let updates = caskade_core::updates::Updates::new(
                store.clone(),
                bus.clone(),
                tracker,
                install,
                Some(settings.clone()),
            );
            background::supervise("game updates", {
                let updates = updates.clone();
                move || updates.watch()
            });
            app.manage(updates);
        }
        // Emulated games resolve through the emulation service (#121).
        let launch = launch.with_emulators(
            caskade_core::launch::EmulatorLaunches::new(store.clone(), emulation.clone())
                .with_settings(settings.clone()),
        );
        let launch = match web_player {
            Some(player) => launch.with_web_player(player),
            None => launch,
        };
        game_focus::follow(app.handle().clone(), &bus, settings.clone());
        // RomM servers (#1): their ROMs download into the cache at launch.
        let launch = match commands::romm::setup(
            app.handle(),
            store.clone(),
            bus.clone(),
            &settings,
            emulation.clone(),
            &setup_paths,
            artwork.clone(),
        ) {
            Ok(romm) => {
                // Missing BIOS files come from RomM's firmware.
                app.state::<caskade_core::emulation::EmulatorSetup>()
                    .set_bios(std::sync::Arc::new(romm.clone()));
                launch.with_preparer(std::sync::Arc::new(romm))
            }
            Err(error) => {
                tracing::error!(%error, "RomM unavailable");
                launch
            }
        };
        app.manage(emulation);
        // Save versions (#239): a game's saves kept after each session, and
        // carried to paired devices below.
        let save_versions = {
            let saves = caskade_core::saves::Saves::new(
                store.clone(),
                app.state::<caskade_core::ludusavi::Ludusavi>()
                    .inner()
                    .clone(),
            )
            .with_catalog(
                app.state::<caskade_core::emulation::EmulationService>()
                    .catalog(),
            );
            let versions = caskade_core::saves::versions::SaveVersions::new(
                saves,
                setup_paths.data.join("saves").join("blobs"),
            );
            versions.set_settings(settings.clone());
            if let Some(accounts) = app.try_state::<caskade_core::accounts::AccountService>() {
                let accounts = accounts.inner().clone();
                versions.set_account(move || accounts.active_id());
            }
            versions.attach(&bus);
            versions
        };
        // Plugins (#123): the folder is read at startup, but no plugin
        // code runs until an enabled plugin is first used. Without a
        // plugin host the app runs without plugins.
        // Catalogue add-ons (#182): an install the app stopped in the middle
        // of rolls back before plugins and themes load.
        let addon_work = setup_paths.data.join("addons");
        if let Err(error) = caskade_core::addons::package::recover(&addon_work) {
            tracing::warn!(%error, "an interrupted add-on install wasn't rolled back");
        }
        app.manage(commands::addon_packages::AddonFolders {
            plugins: setup_paths.data.join("plugins"),
            themes: setup_paths.data.join("themes"),
            work: addon_work,
        });
        app.manage(commands::addon_packages::AddonJobs::default());
        app.manage(commands::video::VideoWindow::default());
        let plugins = match start_plugins(app, &setup_paths, &store, &bus, &settings) {
            Ok(plugins) => {
                plugin_runners.attach(plugins.clone());
                Some(plugins)
            }
            Err(error) => {
                tracing::error!(%error, "plugins unavailable");
                None
            }
        };
        // Built-in, installed and plugin-bundled themes (#139), served by
        // theme_scheme.
        let themes = caskade_core::themes::ThemeService::new(setup_paths.data.join("themes"));
        if let Some(plugins) = &plugins {
            themes.set_plugin_folders(tauri::async_runtime::block_on(
                commands::themes::plugin_folders(plugins),
            ));
        }
        app.manage(themes);
        // The add-on catalogue (#181): read from its cache, fetched on demand.
        if let Some(catalogue) =
            commands::addons::service(app.handle(), setup_paths.cache.join("addons"))
        {
            let media = artwork.clone().map(|cache| {
                if commands::addons::local_origin(&catalogue) {
                    cache.allowing_local_metadata()
                } else {
                    cache
                }
            });
            app.manage(commands::addons::AddonMedia(media));
            app.manage(catalogue);
        }
        app.manage(dev_plugins::DevPlugins::default());
        // `caskade dev <folder>`: load and watch a plugin in place (#131).
        let args: Vec<String> = std::env::args().collect();
        if let Some(folder) =
            dev_plugins::folder_from_args(&args, &std::env::current_dir().unwrap_or_default())
        {
            dev_plugins::load_in_background(app.handle(), folder);
        }
        // A plugin's settings changes reach its backend (#128).
        if let Some(host) = plugins {
            let settings = settings.clone();
            let mut changes = settings.subscribe();
            tauri::async_runtime::spawn(async move {
                use tokio::sync::broadcast::error::RecvError;
                loop {
                    let changed = match changes.recv().await {
                        Ok(changed) => changed,
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    };
                    if let Err(error) =
                        caskade_plugin_host::settings::deliver(&host, &settings, &changed).await
                    {
                        tracing::warn!(%error, "plugin settings.changed not delivered");
                    }
                }
            });
        }
        // Backups (#191–#193): Settings → Backup, automatic backups, and
        // how a restore before this start ended.
        let backups = caskade_backup::Backups::new(store.clone(), setup_paths.clone());
        backups.set_settings(settings.clone());
        backups.set_last_restore(caskade_backup::restore::take_outcome(&setup_paths));
        commands::backup::setup(app.handle(), backups);
        // Store accounts (#164): what they own, reconciled with what
        // discovery finds installed after every scan.
        // Sign-ins (#165) live in the OS keychain, or for the session
        // without one.
        let store_accounts = caskade_core::store_accounts::StoreAccounts::with_secrets(
            store.clone(),
            bus.clone(),
            std::sync::Arc::new(caskade_core::store_accounts::StoreKeychain),
        );
        store_accounts.set_settings(settings.clone());
        // Stores whose libraries Caskade reads (#166–#168).
        store_accounts.register_provider(std::sync::Arc::new(
            caskade_core::store_accounts::steam::SteamProvider::new(),
        ));
        store_accounts.register_provider(std::sync::Arc::new(
            caskade_core::store_accounts::gog::GogProvider::new(),
        ));
        store_accounts.register_provider(std::sync::Arc::new(
            caskade_core::store_accounts::epic::EpicProvider::new(),
        ));
        // RetroAchievements (#189): connected like a store, without a library.
        let retro = std::sync::Arc::new(
            caskade_core::achievements::retroachievements::RetroAchievements::new(),
        );
        store_accounts.register_provider(retro.clone());
        // Achievements (#185) of those accounts, read with their sign-ins.
        let achievements = caskade_core::achievements::Achievements::new(
            store.clone(),
            bus.clone(),
            std::sync::Arc::new(store_accounts.clone()),
        );
        achievements.set_settings(settings.clone());
        achievements.register_provider(retro.clone());
        achievements.register_provider(std::sync::Arc::new(
            caskade_core::achievements::steam::SteamAchievements::new(),
        ));
        achievements.register_provider(std::sync::Arc::new(
            caskade_core::achievements::gog::GogAchievements::new(),
        ));
        achievements.register_provider(std::sync::Arc::new(
            caskade_core::achievements::epic::EpicAchievements::new(),
        ));
        achievements.register_linker(retro);
        // Refresh on an achievements-only account fetches its achievements.
        store_accounts.set_refresher(achievements.account_refresher());
        commands::achievements::setup(app.handle(), achievements);
        // Their icons (#190), through the artwork cache.
        app.manage(commands::achievement_icons::AchievementIcons(artwork.clone()));
        commands::store_accounts::setup(app.handle(), store_accounts);
        // Paired devices (#224–#226): the device key from the keychain (off
        // this thread: a locked keychain holds only sync), the endpoint and
        // pairing.
        if let Some(accounts) = app.try_state::<caskade_core::accounts::AccountService>() {
            let devices = caskade_core::devices::Devices::new(
                store.clone(),
                accounts.inner().clone(),
                std::sync::Arc::new(caskade_core::devices::Keychain),
            );
            devices.set_settings(settings.clone());
            // Streaming pairs through sync with no code (#267): this host
            // enters a paired device's code; this client hands its over.
            if let Some(host) = app.try_state::<caskade_streaming::StreamingHost>() {
                devices.set_stream_handler(std::sync::Arc::new(
                    caskade_streaming::linked::HostHandover::new(std::sync::Arc::new(
                        host.inner().clone(),
                    )),
                ));
            }
            app.manage(caskade_streaming::linked::LinkedPairing::new(
                devices.clone(),
            ));
            if let Some(hosts) = app.try_state::<caskade_streaming::hosts::StreamHosts>() {
                hosts.set_devices(devices.clone());
            }
            // Copies of games between devices (#233): served with the
            // endpoint, so set up before it starts.
            let mut peer_downloads = None;
            match tauri::async_runtime::block_on(caskade_core::transfer::Transfers::new(
                store.clone(),
                devices.clone(),
                &setup_paths.cache,
            )) {
                Ok(transfers) => {
                    // Downloads come from a device nearby that has them
                    // before the internet (#236).
                    let downloads = caskade_core::install::nearby::PeerDownloads::new(
                        transfers.clone(),
                        devices.clone(),
                        settings.clone(),
                        &setup_paths.cache,
                    );
                    if let Some(install) = app.try_state::<caskade_core::install::InstallService>() {
                        install.set_peer_fetcher(std::sync::Arc::new(transfers.clone()));
                        install.set_local_source(std::sync::Arc::new(downloads.clone()));
                    }
                    // And emulators, cores, tools, BIOS files, Hasheous dumps
                    // and the save manifest (#277).
                    if let Some(setup) = app.try_state::<caskade_core::emulation::EmulatorSetup>() {
                        setup.set_nearby(downloads.clone());
                    }
                    if let Some(tools) = app.try_state::<caskade_core::managed::ManagedTools>() {
                        tools.set_nearby(downloads.clone());
                    }
                    if let Some(hasheous) = app.try_state::<caskade_core::hasheous::Hasheous>() {
                        hasheous.set_nearby(downloads.clone());
                    }
                    if let Some(ludusavi) = app.try_state::<caskade_core::ludusavi::Ludusavi>() {
                        ludusavi.set_nearby(downloads.clone());
                    }
                    peer_downloads = Some(downloads);
                    // A game another device sends here (#235): confirmed
                    // here first, unless copies are accepted without asking.
                    commands::copies::take_sends(app.handle(), &transfers, devices.clone());
                    // One-off shares (#246): admitted and served by the same
                    // endpoint, so set up before it starts too.
                    if let Some(install) = app.try_state::<caskade_core::install::InstallService>() {
                        let shares = caskade_core::share::Shares::new(
                            store.clone(),
                            devices.clone(),
                            transfers.clone(),
                            install.inner().clone(),
                            &setup_paths.cache,
                        );
                        // What else can be shared (#248): save versions, and
                        // a ROM's emulator, which emulator setup installs.
                        shares.set_saves(save_versions.clone());
                        if let Some(setup) =
                            app.try_state::<caskade_core::emulation::EmulatorSetup>()
                        {
                            shares.set_emulator_installer(std::sync::Arc::new(
                                setup.inner().clone(),
                            ));
                        }
                        if let Some(requests) =
                            app.try_state::<commands::install_requests::InstallRequests>()
                        {
                            requests.set_shares(shares.clone());
                        }
                        // Someone receiving a share is a notification.
                        shares.set_bus(bus.clone());
                        commands::shares::setup(app.handle(), &shares);
                        app.manage(shares);
                    }
                    app.manage(transfers);
                }
                Err(error) => tracing::warn!(%error, "copies between devices unavailable"),
            }
            // Games other devices start here, and starting games there (#234).
            let remote_play =
                caskade_core::remote_play::RemotePlay::new(store.clone(), devices.clone());
            remote_play.set_settings(settings.clone());
            commands::remote_play::setup(app.handle(), &remote_play, devices.clone());
            app.manage(remote_play);
            // Library sync with paired devices (#228–#230), in the background.
            let sync = caskade_core::sync::SyncService::new(store.clone(), devices.clone());
            sync.set_settings(settings.clone());
            // A RomM server removed on another device: its ROMs and games go
            // here too, through RomM's own removal.
            if let Some(romm) = app.try_state::<caskade_core::romm::RommService>() {
                let romm = romm.inner().clone();
                sync.on_romm_removed(move |id| {
                    let romm = romm.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = romm.remove_server(&id).await {
                            tracing::warn!(%error, "a RomM server removed elsewhere wasn't cleaned up");
                        }
                    });
                });
            }
            // Save files travel after the changes (#239), and a launch asks
            // paired devices for newer saves first.
            sync.register_blobs(std::sync::Arc::new(
                caskade_core::saves::versions::SaveBlobs(save_versions.clone()),
            ));
            // This device's default admin joining one from another device
            // hands the signed-in account over (#273).
            let joined = accounts.inner().clone();
            sync.on_account_joined(move |from, into| {
                let joined = joined.clone();
                tauri::async_runtime::spawn(async move { joined.joined(&from, &into).await });
            });
            // Games from another device are filled in here: metadata and
            // artwork stay on each device (#75). Matching runs only while
            // online lookups are allowed.
            if let Some(metadata) = app.try_state::<caskade_core::metadata::Metadata>() {
                let metadata = metadata.inner().clone();
                sync.on_games_arrived(move || metadata.wake());
            }
            // The user's own artwork travels with its files (#273).
            if let Some(cache) = artwork {
                sync.set_artwork_cache(cache);
            }
            save_versions.set_remote(caskade_core::saves::versions::SyncRefresh::new(
                sync.clone(),
                devices.clone(),
            ));
            let engine = sync.clone();
            save_versions.set_device(move || engine.engine().map(|e| e.device().to_owned()));
            // What's running on which device (#240): told at the end of each
            // exchange, and a game starting or ending here syncs at once.
            let presence = caskade_core::sync::presence::Presence::new(store.clone());
            if let Some(accounts) = app.try_state::<caskade_core::accounts::AccountService>() {
                let accounts = accounts.inner().clone();
                presence.set_account(move || accounts.active_id());
            }
            sync.set_presence(presence.clone());
            // Backups kept on a paired device (#243): sent to the chosen
            // one after the changes, and held here for the others.
            let peer_backups = caskade_backup::peers::PeerBackups::new(
                settings.clone(),
                setup_paths.data.join("peer-backups"),
                app.state::<caskade_backup::Backups>()
                    .default_destination(),
            );
            peer_backups.set_devices(devices.clone());
            sync.register_blobs(std::sync::Arc::new(peer_backups.clone()));
            // What each device keeps of its downloads, told on each exchange.
            if let Some(downloads) = peer_downloads {
                sync.register_blobs(std::sync::Arc::new(downloads));
            }
            // Emulators' configuration files, while Emulator configuration
            // syncs (#275).
            if let Some(emulation) = app.try_state::<caskade_core::emulation::EmulationService>() {
                sync.register_blobs(std::sync::Arc::new(
                    caskade_core::sync::config_files::EmulatorConfig::new(
                        store.clone(),
                        emulation.catalog(),
                        &setup_paths.data,
                    ),
                ));
            }
            app.manage(peer_backups);
            // An emulator installed from another device's preference
            // becomes the platform's default at once (#275).
            background::supervise("preferred emulators", {
                let (bus, sync) = (bus.clone(), sync.clone());
                move || caskade_core::sync::offers::watch_emulators(&bus, sync.clone())
            });
            background::supervise("presence", {
                let (presence, bus, sync, devices) =
                    (presence.clone(), bus.clone(), sync.clone(), devices.clone());
                move || presence.watch(&bus, sync.clone(), devices.clone())
            });
            commands::presence::setup(app.handle(), presence, devices.clone());
            commands::sync::setup(app.handle(), devices, sync);
        }
        commands::saves::forward_changes(app.handle(), &save_versions);
        app.manage(save_versions);
        app.manage(settings);
        app.manage(store);
        app.manage(launch);
        // `caskade --launch` from other programs (#256), once the library
        // and the launch service are there to take them.
        launch_cli::server::listen(
            app.handle(),
            launch_cli::Endpoint::for_paths(&setup_paths),
        );
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

/// Gives Tauri a runtime with at most [`ASYNC_WORKERS`] workers.
fn async_runtime() {
    let workers = std::thread::available_parallelism()
        .map_or(ASYNC_WORKERS, |cores| cores.get().min(ASYNC_WORKERS));
    match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
    {
        // It runs as long as the process: Tauri keeps only a handle.
        Ok(runtime) => tauri::async_runtime::set(Box::leak(Box::new(runtime)).handle().clone()),
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

/// The plugin host (#123): reads the plugin folders and joins enabled
/// backends to the hook bus (#125).
fn start_plugins<R: tauri::Runtime>(
    app: &tauri::App<R>,
    paths: &caskade_core::AppPaths,
    store: &caskade_core::db::LibraryStore,
    bus: &caskade_core::bus::Bus,
    settings: &caskade_core::settings::SettingsService,
) -> Result<caskade_plugin_host::PluginHost, Box<dyn std::error::Error>> {
    use tauri::Manager;
    let folder = paths.data.join("plugins");
    std::fs::create_dir_all(&folder)?;
    let plugins = caskade_plugin_host::PluginHost::new(
        caskade_plugin_host::PluginServices {
            store: store.clone(),
            bus: Some(bus.clone()),
            settings: Some(settings.clone()),
        },
        caskade_plugin_host::HostOptions {
            folders: vec![folder],
            data: paths.data.join("plugin-data"),
            limits: caskade_plugin_host::Limits::default(),
        },
    )?;
    // Compiled backends are kept across starts, signed with a key in the
    // database (#136); without it every start compiles.
    if let Err(error) = tauri::async_runtime::block_on(
        plugins.enable_component_cache(paths.cache.join("plugin-components")),
    ) {
        tracing::warn!(%error, "plugin component cache unavailable");
    }
    match tauri::async_runtime::block_on(plugins.discover()) {
        Ok(found) => tracing::info!(plugins = found.len(), "plugins discovered"),
        Err(error) => tracing::warn!(%error, "plugin discovery failed"),
    }
    // Plugin providers use the built-in code paths (#126).
    plugins.register_providers(bus);
    // Enabled backends join the hook bus (#125); none starts until its
    // activation event, and the hooks follow enable/disable.
    let hooks = caskade_plugin_host::PluginHooks::new(plugins.clone(), bus.clone());
    tauri::async_runtime::spawn({
        let hooks = hooks.clone();
        async move { hooks.sync().await }
    });
    app.manage(hooks);
    app.manage(plugins.clone());
    Ok(plugins)
}
