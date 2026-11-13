//! Starting the app (#86): every service the app runs, built and wired the
//! same way whichever shell runs them. [`open`] brings up the library and
//! what the window needs before it shows; [`start`] everything else. The
//! shell then holds the [`Services`] (the Tauri adapter manages each as
//! state) and adds what only it can do: windows, the tray, its updater.

use std::sync::Arc;

use caskade_core::{
    AppPaths, accounts::AccountService, artwork::ArtworkCache, bus::Bus, db::LibraryStore,
    launch::WindowPlacement, settings::SettingsService,
};

use crate::{
    commands::{
        achievement_icons::AchievementIcons,
        addon_packages::{AddonFolders, AddonJobs},
        addons::AddonMedia,
        game_edit::EditArtwork,
        install_requests::InstallRequests,
    },
    events::Events,
};

/// What only the shell can answer while the services start.
pub struct Shell {
    /// Where the app tells its UI what happened.
    pub events: Events,
    /// The main window's place on screen, for windows that open over it
    /// (web games, streams); `None` while it's hidden or minimized.
    pub placement: Arc<dyn Fn() -> Option<WindowPlacement> + Send + Sync>,
    /// The environment that lets a started program take focus from the
    /// window (an X11 startup notification, say).
    pub activation: Arc<dyn Fn() -> Vec<(String, String)> + Send + Sync>,
}

/// What [`open`] brings up: the library, accounts, settings and the launch
/// service, with the UI following the bus.
pub struct Core {
    pub paths: AppPaths,
    /// One cache, shared by every clone: without it no artwork shows;
    /// everything else works.
    pub artwork: Option<ArtworkCache>,
    pub bus: Bus,
    pub store: LibraryStore,
    pub accounts: AccountService,
    pub notifications: caskade_achievements::notifications::Notifications,
    pub settings: SettingsService,
    /// Re-attached to games still running from the last session.
    pub launch: caskade_core::launch::LaunchService,
    /// Built-in and installed themes (#139), before the window opens so its
    /// first frame has the chosen one (#90); plugin-bundled themes join
    /// once the plugin host starts.
    pub themes: caskade_core::themes::ThemeService,
    plugin_runners: caskade_plugin_host::providers::PluginRunners,
}

/// Opens the library (applying a restore staged before the restart, #192),
/// the accounts and settings, and starts the launch service. The library's
/// error when it can't open: nothing works without it. `artwork` is the
/// one artwork cache, shared with the shell (it serves the images).
pub fn open(
    paths: &AppPaths,
    artwork: Option<ArtworkCache>,
    events: &Events,
) -> caskade_core::Result<Core> {
    crate::events::bridge_background(events.clone());
    let bus = Bus::default();
    crate::events::bridge(events.clone(), &bus);
    // The library store, then the launch service, which first re-attaches
    // to games still running from a previous session.
    let store = crate::runtime::block_on(crate::recovery::open_library(paths))?;
    // Local accounts (#202): a lone account without a PIN is signed in, so
    // a library without accounts set up starts as before.
    let accounts = crate::runtime::block_on(AccountService::open(store.clone(), bus.clone()))?;
    // Notifications (#198): game additions, failures and updates, kept in
    // the library.
    let notifications = caskade_achievements::notifications::Notifications::new(store.clone());
    crate::commands::notifications::setup(events.clone(), &notifications, &bus);
    // Plugin runners (#126) attach once the plugin host exists.
    let plugin_runners = caskade_plugin_host::providers::PluginRunners::new();
    let launch = caskade_core::launch::LaunchService::new(store.clone())
        .with_bus(bus.clone())
        .with_plugins(Arc::new(plugin_runners.clone()))
        // Age limits (#209): every launch asks the signed-in account.
        .with_play_gate(Arc::new(accounts.clone()));
    match crate::runtime::block_on(launch.recover()) {
        Ok(recovered) => tracing::info!(recovered = recovered.len(), "launch service ready"),
        Err(error) => tracing::error!(%error, "games from the last session not re-attached"),
    }
    let settings = SettingsService::with_bus(store.clone(), bus.clone());
    if let Err(error) = settings.register_core(paths) {
        tracing::error!(%error, "core settings unavailable");
    }
    // A new device starts as its kind suits (#244): a handheld or a TV in
    // console mode, with emulators fullscreen and a larger interface.
    match crate::runtime::block_on(caskade_core::device_type::settle_new_device(
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
    // Launch on system startup (#114): the login item follows the setting.
    if let Some(item) = caskade_core::login_item::LoginItem::live() {
        crate::runtime::spawn(caskade_core::login_item::follow(settings.clone(), item));
    }
    // Catalogue add-ons (#182): an install the app stopped in the middle of
    // rolls back before plugins and themes load.
    if let Err(error) = caskade_addons::package::recover(&paths.data.join("addons")) {
        tracing::warn!(%error, "an interrupted add-on install wasn't rolled back");
    }
    let themes = caskade_core::themes::ThemeService::new(paths.data.join("themes"));
    // A theme that left the built-in set (Paper, #125): its users move to
    // the default, without a notice.
    if let Err(error) = crate::runtime::block_on(caskade_core::themes::retire(&settings, &themes)) {
        tracing::warn!(%error, "a retired theme choice wasn't moved to the default");
    }
    Ok(Core {
        paths: paths.clone(),
        artwork,
        bus,
        store,
        accounts,
        notifications,
        settings,
        launch,
        themes,
        plugin_runners,
    })
}

/// Every service the app runs. The optional ones are those that may fail
/// to start (the app runs without them) or that need one that did.
pub struct Services {
    pub paths: AppPaths,
    pub bus: Bus,
    pub store: LibraryStore,
    pub accounts: AccountService,
    pub notifications: caskade_achievements::notifications::Notifications,
    pub settings: SettingsService,
    pub launch: caskade_core::launch::LaunchService,
    pub storage: Option<caskade_core::storage::StorageService>,
    pub storage_source: Option<caskade_sources::storage::StorageSource>,
    pub install: Option<caskade_core::install::InstallService>,
    /// With `install`: what's waiting for the user to confirm.
    pub install_requests: Option<InstallRequests>,
    pub discovery: caskade_sources::DiscoveryService,
    pub discovery_controller: crate::discovery::DiscoveryController,
    pub emulation: caskade_core::emulation::EmulationService,
    pub emulator_setup: caskade_core::emulation::EmulatorSetup,
    pub tools: Option<caskade_core::managed::ManagedTools>,
    pub streaming_host: Option<caskade_streaming::StreamingHost>,
    pub stream_hosts: Option<caskade_streaming::hosts::StreamHosts>,
    pub streaming_client: Option<caskade_streaming::StreamingClient>,
    pub edit_artwork: EditArtwork,
    pub web: caskade_core::web::WebGames,
    pub metadata: caskade_core::metadata::Metadata,
    pub hasheous: caskade_core::hasheous::Hasheous,
    pub ludusavi: caskade_core::ludusavi::Ludusavi,
    pub ports: caskade_core::ports::Ports,
    pub igdb: caskade_core::igdb::Igdb,
    pub updates: Option<caskade_core::updates::Updates>,
    pub romm: Option<caskade_core::romm::RommService>,
    pub save_versions: caskade_core::saves::versions::SaveVersions,
    pub addon_folders: AddonFolders,
    pub addon_jobs: AddonJobs,
    pub plugins: Option<caskade_plugin_host::PluginHost>,
    pub plugin_hooks: Option<Arc<caskade_plugin_host::PluginHooks>>,
    pub themes: caskade_core::themes::ThemeService,
    pub catalogue: Option<caskade_addons::CatalogueService>,
    pub addon_media: Option<AddonMedia>,
    pub backups: caskade_backup::Backups,
    pub store_accounts: caskade_core::store_accounts::StoreAccounts,
    pub achievements: caskade_achievements::achievements::Achievements,
    pub achievement_icons: AchievementIcons,
    pub devices: caskade_core::devices::Devices,
    pub linked_pairing: caskade_streaming::linked::LinkedPairing,
    pub transfers: Option<caskade_core::transfer::Transfers>,
    pub shares: Option<caskade_share::Shares>,
    pub remote_play: caskade_core::remote_play::RemotePlay,
    pub sync: caskade_core::sync::SyncService,
    pub presence: caskade_core::sync::presence::Presence,
    pub peer_backups: caskade_backup::peers::PeerBackups,
    /// A `caskade://pair` link opened, waiting for an admin (#226).
    pub opened_pairing_link: crate::commands::sync::OpenedPairingLink,
    /// Plugins and themes loaded in place (`caskade dev`, #131).
    pub dev_plugins: crate::dev_plugins::DevPlugins,
}

/// Starts every other service on `core`, wired to one another, with
/// their background work running.
pub fn start(core: Core, shell: Shell) -> Services {
    let Core {
        paths,
        artwork,
        bus,
        store,
        accounts,
        notifications,
        settings,
        launch,
        themes,
        plugin_runners,
    } = core;
    let events = shell.events.clone();
    let launch = launch.with_runner_resolver(Arc::new(
        caskade_core::install::InstallerRunnerResolver(settings.clone()),
    ));
    let storage = match caskade_core::storage::StorageService::new(
        store.clone(),
        settings.clone(),
        launch.clone(),
    ) {
        Ok(storage) => {
            let storage = storage.with_paths(paths.clone());
            // Disconnected drives keep their catalogue entries. Pending
            // moves remain recoverable and are retried when their roots
            // return.
            if let Err(error) = crate::runtime::block_on(storage.initialize()) {
                tracing::warn!(%error, "storage recovery deferred until its roots are available");
            }
            Some(storage)
        }
        Err(error) => {
            tracing::error!(%error, "storage locations unavailable");
            None
        }
    };
    // A library from before first-run setup existed doesn't go through it;
    // a new one scans once setup is done.
    if let Err(error) =
        crate::runtime::block_on(crate::onboarding::settle_existing(&settings, &store))
    {
        tracing::warn!(%error, "first-run setup state unavailable");
    }
    // Installed-games discovery (#100). The discovery controller (#101)
    // owns the initial scan, the schedule and the watch; this only
    // registers the sources and their settings.
    if let Err(error) = caskade_sources::register_settings(&settings) {
        tracing::error!(%error, "discovery settings unavailable");
    }
    let discovery = caskade_sources::DiscoveryService::new(
        store.clone(),
        bus.clone(),
        caskade_discovery::HostEnv::live(),
    );
    let (storage_source, install, install_requests) = match crate::commands::install::services(
        store.clone(),
        bus.clone(),
        &paths,
        settings.clone(),
        storage.as_ref(),
        crate::commands::install::sidecar(),
    ) {
        Ok((source, install)) => (
            Some(source),
            Some(install),
            Some(InstallRequests::default()),
        ),
        Err(error) => {
            tracing::error!(%error, "installs unavailable");
            (None, None, None)
        }
    };
    // Installs paused before the last exit are listed again, paused (#131).
    if let Some(install) = install.clone() {
        let launch = launch.clone();
        crate::runtime::spawn(async move {
            let restored = install.restore_paused(Some(launch)).await;
            if restored > 0 {
                tracing::info!(restored, "paused installs restored");
            }
        });
    }
    let mut discovery_controller =
        crate::discovery::DiscoveryController::new(discovery.clone(), settings.clone());
    if let Some(source) = &storage_source {
        discovery_controller = discovery_controller.with_storage(source.clone());
    }
    discovery_controller.bridge(events.clone());
    discovery_controller.start();
    // Embedded definitions are validated before constructing the service.
    let emulation = caskade_core::emulation::EmulationService::new(
        store.clone(),
        bus.clone(),
        caskade_sources::emulation::catalog().unwrap_or_else(|error| {
            tracing::error!(%error, "shipped emulation definitions are invalid");
            caskade_core::emulation::Catalog::default()
        }),
    )
    .with_paths(&paths);
    // One-click emulator installs, and their update checks, which follow
    // `core.updates`.
    let emulator_setup = caskade_core::emulation::EmulatorSetup::new(
        store.clone(),
        bus.clone(),
        emulation.clone(),
        &paths,
    );
    emulator_setup.set_settings(settings.clone());
    emulator_setup.set_launch(launch.clone());
    crate::background::supervise("emulator updates", {
        let emulator_setup = emulator_setup.clone();
        move || emulator_setup.watch()
    });
    // Helper tools (the streaming host and client): their update checks
    // follow `core.updates` like the emulators'.
    let (mut tools, mut streaming_host, mut stream_hosts, mut streaming_client) =
        (None, None, None, None);
    match caskade_sources::tools::catalog() {
        Ok(catalog) => {
            let managed = caskade_core::managed::ManagedTools::new(
                store.clone(),
                bus.clone(),
                catalog,
                &paths,
            );
            managed.set_settings(settings.clone());
            crate::background::supervise("tool updates", {
                let tools = managed.clone();
                move || tools.watch()
            });
            // The streaming host (#257): runs with Caskade when hosting is
            // on, and offers the library's games (#259).
            let host = caskade_streaming::StreamingHost::new(
                managed.clone(),
                caskade_streaming::elevation::Elevation::new(&paths),
                settings.clone(),
                &paths,
            );
            crate::commands::streaming::setup(
                host.clone(),
                store.clone(),
                Some(accounts.clone()),
                &bus,
                artwork.clone(),
                &paths,
            );
            streaming_host = Some(host);
            // Devices to stream from (#262), through Caskade's own client.
            let client = caskade_streaming::StreamingClient::new(managed.clone());
            match caskade_streaming::hosts::StreamHosts::new(
                store.clone(),
                bus.clone(),
                settings.clone(),
                client.clone(),
            ) {
                Ok(hosts) => {
                    // Play streams from them (#264).
                    let (placement, activation) =
                        (shell.placement.clone(), shell.activation.clone());
                    launch.set_streamer(Arc::new(
                        caskade_streaming::play::Streams::new(
                            client.clone(),
                            Some(hosts.clone()),
                            settings.clone(),
                        )
                        .with_placement(move || placement())
                        .with_activation(move || activation()),
                    ));
                    crate::commands::streaming::watch_hosts(hosts.clone());
                    stream_hosts = Some(hosts);
                    streaming_client = Some(client);
                }
                Err(error) => tracing::error!(%error, "streaming hosts unavailable"),
            }
            tools = Some(managed);
        }
        Err(error) => tracing::error!(%error, "shipped tool definitions are invalid"),
    }
    // Detection runs after every library scan; results wait for review in
    // Settings → Emulators.
    crate::background::supervise("emulator detection", {
        let emulation = emulation.clone();
        move || emulation.watch_library_scans()
    });
    // ROM scanners take part in every library update (#119).
    crate::background::supervise("ROM scanners", {
        let roms = caskade_sources::RomsSource::new(emulation.scanners());
        let bus = bus.clone();
        move || roms.watch(&bus)
    });
    // Offline Hasheous dumps for the configured emulators' platforms and PC
    // games, kept current in the background.
    let hasheous = caskade_core::hasheous::Hasheous::new(paths.cache.join("hasheous"));
    let ludusavi = caskade_core::ludusavi::Ludusavi::new(paths.cache.join("ludusavi"));
    // Fan ports of games: Decomp Buddy's install plans, fetched daily.
    let ports = caskade_core::ports::Ports::new(paths.cache.join("ports"));
    // IGDB metadata from Caskade's metadata server, cached for good.
    let igdb = caskade_core::igdb::Igdb::new(paths.cache.join("igdb"));
    // Games IGDB has no icon for get their executable's.
    if let Some(artwork) = artwork.clone() {
        let icons = caskade_core::icons::ExeIcons::new(store.clone(), bus.clone(), artwork);
        crate::background::supervise("executable icons", move || icons.watch());
    }
    // Web games play in the game window: this executable, started again.
    let web_player = std::env::current_exe()
        .inspect_err(|error| {
            tracing::error!(%error, "web games can't start: Caskade's executable is unknown");
        })
        .ok()
        .map(|program| {
            let placement = shell.placement.clone();
            caskade_core::launch::WebPlayer::new(program, paths.data.join("web"))
                .with_settings(settings.clone())
                // In desktop mode a web game opens over Caskade's window.
                .with_placement(move || placement())
        });
    // Add web game: the page's details and picture; Play offline.
    let mut web = caskade_core::web::WebGames::new(store.clone()).with_bus(bus.clone());
    if let Some(artwork) = artwork.clone() {
        web = web.with_artwork(artwork);
    }
    if let Some(player) = web_player.clone() {
        web = web.with_player(player);
    }
    // Library games matched to IGDB (RomM, Hasheous names, then store ids)
    // and filled in from it, in the background.
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
    crate::onboarding::supervise_online(settings.clone(), {
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
    crate::commands::game_edit::forward_refresh(events.clone(), &metadata);
    let updates = install.clone().map(|install| {
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
        crate::background::supervise("game updates", {
            let updates = updates.clone();
            move || updates.watch()
        });
        updates
    });
    // Emulated games resolve through the emulation service (#121).
    let launch = launch.with_emulators(
        caskade_core::launch::EmulatorLaunches::new(store.clone(), emulation.clone())
            .with_settings(settings.clone()),
    );
    let launch = match web_player {
        Some(player) => launch.with_web_player(player),
        None => launch,
    };
    // RomM servers (#1): their ROMs download into the cache at launch.
    let (launch, romm) = match crate::commands::romm::setup(
        events.clone(),
        store.clone(),
        bus.clone(),
        &settings,
        emulation.clone(),
        &paths,
        artwork.clone(),
    ) {
        Ok(romm) => {
            // Missing BIOS files come from RomM's firmware.
            emulator_setup.set_bios(Arc::new(romm.clone()));
            (launch.with_preparer(Arc::new(romm.clone())), Some(romm))
        }
        Err(error) => {
            tracing::error!(%error, "RomM unavailable");
            (launch, None)
        }
    };
    // Save versions (#239): a game's saves kept after each session, and
    // carried to paired devices below.
    let save_versions = {
        let saves = caskade_core::saves::Saves::new(store.clone(), ludusavi.clone())
            .with_catalog(emulation.catalog());
        let versions = caskade_core::saves::versions::SaveVersions::new(
            saves,
            paths.data.join("saves").join("blobs"),
        );
        versions.set_settings(settings.clone());
        let active = accounts.clone();
        versions.set_account(move || active.active_id());
        versions.attach(&bus);
        versions
    };
    // An interrupted add-on install was rolled back in `open`.
    let addon_work = paths.data.join("addons");
    let addon_folders = AddonFolders {
        plugins: paths.data.join("plugins"),
        themes: paths.data.join("themes"),
        work: addon_work,
    };
    // Plugins (#123): the folder is read at startup, but no plugin code
    // runs until an enabled plugin is first used. Without a plugin host the
    // app runs without plugins.
    let (plugins, plugin_hooks) = match start_plugins(&paths, &store, &bus, &settings) {
        Ok((plugins, hooks)) => {
            plugin_runners.attach(plugins.clone());
            (Some(plugins), Some(hooks))
        }
        Err(error) => {
            tracing::error!(%error, "plugins unavailable");
            (None, None)
        }
    };
    // Plugin-bundled themes (#139) join the built-in and installed ones.
    if let Some(plugins) = &plugins {
        themes.set_plugin_folders(crate::runtime::block_on(
            crate::commands::themes::plugin_folders(plugins),
        ));
    }
    // The add-on catalogue (#181): read from its cache, fetched on demand.
    let catalogue = crate::commands::addons::service(
        paths.cache.join("addons"),
        crate::self_update::public_key(),
    );
    let addon_media = catalogue.as_ref().map(|catalogue| {
        AddonMedia(artwork.clone().map(|cache| {
            if crate::commands::addons::local_origin(catalogue) {
                cache.allowing_local_metadata()
            } else {
                cache
            }
        }))
    });
    // A plugin's settings changes reach its backend (#128).
    if let Some(host) = plugins.clone() {
        let settings = settings.clone();
        let mut changes = settings.subscribe();
        crate::runtime::spawn(async move {
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
    // Backups (#191–#193): Settings → Backup, automatic backups, and how a
    // restore before this start ended.
    let backups = caskade_backup::Backups::new(store.clone(), paths.clone());
    backups.set_settings(settings.clone());
    backups.set_last_restore(caskade_backup::restore::take_outcome(&paths));
    crate::commands::backup::setup(events.clone(), &backups);
    let (store_accounts, achievements) = store_accounts(&store, &bus, &settings);
    crate::commands::store_accounts::setup(events.clone(), &store_accounts);
    // Paired devices (#224–#226): the device key from the keychain (off
    // this thread: a locked keychain holds only sync), the endpoint and
    // pairing.
    let devices = caskade_core::devices::Devices::new(
        store.clone(),
        accounts.clone(),
        Arc::new(caskade_core::devices::Keychain),
    );
    devices.set_settings(settings.clone());
    // Streaming pairs through sync with no code (#267): this host enters a
    // paired device's code; this client hands its over.
    if let Some(host) = &streaming_host {
        devices.set_stream_handler(Arc::new(caskade_streaming::linked::HostHandover::new(
            Arc::new(host.clone()),
        )));
    }
    let linked_pairing = caskade_streaming::linked::LinkedPairing::new(devices.clone());
    if let Some(hosts) = &stream_hosts {
        hosts.set_devices(devices.clone());
    }
    // Copies of games between devices (#233): served with the endpoint, so
    // set up before it starts.
    let mut peer_downloads = None;
    let mut shares = None;
    let transfers = match crate::runtime::block_on(caskade_core::transfer::Transfers::new(
        store.clone(),
        devices.clone(),
        &paths.cache,
    )) {
        Ok(transfers) => {
            // Downloads come from a device nearby that has them before the
            // internet (#236).
            let downloads = caskade_core::install::nearby::PeerDownloads::new(
                transfers.clone(),
                devices.clone(),
                settings.clone(),
                &paths.cache,
            );
            if let Some(install) = &install {
                install.set_peer_fetcher(Arc::new(transfers.clone()));
                install.set_local_source(Arc::new(downloads.clone()));
            }
            // And emulators, cores, tools, BIOS files, Hasheous dumps and
            // the save manifest (#277).
            emulator_setup.set_nearby(downloads.clone());
            if let Some(tools) = &tools {
                tools.set_nearby(downloads.clone());
            }
            hasheous.set_nearby(downloads.clone());
            ludusavi.set_nearby(downloads.clone());
            peer_downloads = Some(downloads);
            // A game another device sends here (#235): confirmed here
            // first, unless copies are accepted without asking.
            crate::commands::copies::take_sends(
                events.clone(),
                &transfers,
                devices.clone(),
                settings.clone(),
                install.clone(),
                install_requests.clone(),
            );
            // One-off shares (#246): admitted and served by the same
            // endpoint, so set up before it starts too.
            if let Some(install) = &install {
                let one_off = caskade_share::Shares::new(
                    store.clone(),
                    devices.clone(),
                    transfers.clone(),
                    install.clone(),
                    &paths.cache,
                );
                // What else can be shared (#248): save versions, and a
                // ROM's emulator, which emulator setup installs.
                one_off.set_saves(save_versions.clone());
                one_off.set_emulator_installer(Arc::new(emulator_setup.clone()));
                if let Some(requests) = &install_requests {
                    requests.set_shares(one_off.clone());
                }
                // Someone receiving a share is a notification.
                one_off.set_bus(bus.clone());
                crate::commands::shares::setup(events.clone(), &one_off);
                shares = Some(one_off);
            }
            Some(transfers)
        }
        Err(error) => {
            tracing::warn!(%error, "copies between devices unavailable");
            None
        }
    };
    // Games other devices start here, and starting games there (#234).
    let remote_play = caskade_core::remote_play::RemotePlay::new(store.clone(), devices.clone());
    remote_play.set_settings(settings.clone());
    crate::commands::remote_play::setup(events.clone(), &remote_play, devices.clone());
    // Library sync with paired devices (#228–#230), in the background.
    let sync = caskade_core::sync::SyncService::new(store.clone(), devices.clone());
    sync.set_settings(settings.clone());
    // A RomM server removed on another device: its ROMs and games go here
    // too, through RomM's own removal.
    if let Some(romm) = romm.clone() {
        sync.on_romm_removed(move |id| {
            let romm = romm.clone();
            crate::runtime::spawn(async move {
                if let Err(error) = romm.remove_server(&id).await {
                    tracing::warn!(%error, "a RomM server removed elsewhere wasn't cleaned up");
                }
            });
        });
    }
    // Save files travel after the changes (#239), and a launch asks paired
    // devices for newer saves first.
    sync.register_blobs(Arc::new(caskade_core::saves::versions::SaveBlobs(
        save_versions.clone(),
    )));
    // This device's default admin joining one from another device hands the
    // signed-in account over (#273).
    let joined = accounts.clone();
    sync.on_account_joined(move |from, into| {
        let joined = joined.clone();
        crate::runtime::spawn(async move { joined.joined(&from, &into).await });
    });
    // Games from another device are filled in here: metadata and artwork
    // stay on each device (#75). Matching runs only while online lookups
    // are allowed.
    let arrived = metadata.clone();
    sync.on_games_arrived(move || arrived.wake());
    // The user's own artwork travels with its files (#273).
    if let Some(cache) = artwork.clone() {
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
    let active = accounts.clone();
    presence.set_account(move || active.active_id());
    sync.set_presence(presence.clone());
    // Backups kept on a paired device (#243): sent to the chosen one after
    // the changes, and held here for the others.
    let peer_backups = caskade_backup::peers::PeerBackups::new(
        settings.clone(),
        paths.data.join("peer-backups"),
        backups.default_destination(),
    );
    peer_backups.set_devices(devices.clone());
    sync.register_blobs(Arc::new(peer_backups.clone()));
    // What each device keeps of its downloads, told on each exchange.
    if let Some(downloads) = peer_downloads {
        sync.register_blobs(Arc::new(downloads));
    }
    // Emulators' configuration files, while Emulator configuration syncs
    // (#275).
    sync.register_blobs(Arc::new(
        caskade_core::sync::config_files::EmulatorConfig::new(
            store.clone(),
            emulation.catalog(),
            &paths.data,
        ),
    ));
    // An emulator installed from another device's preference becomes the
    // platform's default at once (#275).
    crate::background::supervise("preferred emulators", {
        let (bus, sync) = (bus.clone(), sync.clone());
        move || caskade_core::sync::offers::watch_emulators(&bus, sync.clone())
    });
    crate::background::supervise("presence", {
        let (presence, bus, sync, devices) =
            (presence.clone(), bus.clone(), sync.clone(), devices.clone());
        move || presence.watch(&bus, sync.clone(), devices.clone())
    });
    crate::commands::presence::setup(events.clone(), &presence, devices.clone(), store.clone());
    crate::commands::sync::setup(events.clone(), &devices, &sync);
    crate::commands::saves::forward_changes(events, &save_versions);
    Services {
        edit_artwork: EditArtwork(artwork.clone()),
        achievement_icons: AchievementIcons(artwork),
        paths,
        bus,
        store,
        accounts,
        notifications,
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
        addon_jobs: AddonJobs::default(),
        plugins,
        plugin_hooks,
        themes,
        catalogue,
        addon_media,
        backups,
        store_accounts,
        achievements,
        devices,
        linked_pairing,
        transfers,
        shares,
        remote_play,
        sync,
        presence,
        peer_backups,
        opened_pairing_link: Default::default(),
        dev_plugins: Default::default(),
    }
}

/// Store accounts (#164): what they own, reconciled with what discovery
/// finds installed after every scan, with their sign-ins (#165) in the OS
/// keychain, or for the session without one; and the achievements (#185)
/// of those accounts, read with their sign-ins.
fn store_accounts(
    store: &LibraryStore,
    bus: &Bus,
    settings: &SettingsService,
) -> (
    caskade_core::store_accounts::StoreAccounts,
    caskade_achievements::achievements::Achievements,
) {
    use caskade_achievements::achievements;
    let accounts = caskade_core::store_accounts::StoreAccounts::with_secrets(
        store.clone(),
        bus.clone(),
        Arc::new(caskade_core::secrets::Keychain::new(
            caskade_core::store_accounts::KEYCHAIN_SERVICE,
        )),
    );
    accounts.set_settings(settings.clone());
    // Stores whose libraries Caskade reads (#166–#168).
    accounts.register_provider(Arc::new(
        caskade_core::store_accounts::steam::SteamProvider::new(),
    ));
    accounts.register_provider(Arc::new(
        caskade_core::store_accounts::gog::GogProvider::new(),
    ));
    accounts.register_provider(Arc::new(
        caskade_core::store_accounts::epic::EpicProvider::new(),
    ));
    // RetroAchievements (#189): connected like a store, without a library.
    let retro = Arc::new(achievements::retroachievements::RetroAchievements::new());
    accounts.register_provider(retro.clone());
    let found =
        achievements::Achievements::new(store.clone(), bus.clone(), Arc::new(accounts.clone()));
    found.set_settings(settings.clone());
    found.register_provider(retro.clone());
    found.register_provider(Arc::new(achievements::steam::SteamAchievements::new()));
    found.register_provider(Arc::new(achievements::gog::GogAchievements::new()));
    found.register_provider(Arc::new(achievements::epic::EpicAchievements::new()));
    found.register_linker(retro);
    // Refresh on an achievements-only account fetches its achievements.
    accounts.set_refresher(found.account_refresher());
    crate::background::supervise("achievements", {
        let found = found.clone();
        move || found.watch()
    });
    (accounts, found)
}

/// The plugin host (#123): reads the plugin folders and joins enabled
/// backends to the hook bus (#125).
fn start_plugins(
    paths: &AppPaths,
    store: &LibraryStore,
    bus: &Bus,
    settings: &SettingsService,
) -> Result<
    (
        caskade_plugin_host::PluginHost,
        Arc<caskade_plugin_host::PluginHooks>,
    ),
    Box<dyn std::error::Error>,
> {
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
    if let Err(error) = crate::runtime::block_on(
        plugins.enable_component_cache(paths.cache.join("plugin-components")),
    ) {
        tracing::warn!(%error, "plugin component cache unavailable");
    }
    match crate::runtime::block_on(plugins.discover()) {
        Ok(found) => tracing::info!(plugins = found.len(), "plugins discovered"),
        Err(error) => tracing::warn!(%error, "plugin discovery failed"),
    }
    // Plugin providers use the built-in code paths (#126).
    plugins.register_providers(bus);
    // Enabled backends join the hook bus (#125); none starts until its
    // activation event, and the hooks follow enable/disable.
    let hooks = caskade_plugin_host::PluginHooks::new(plugins.clone(), bus.clone());
    crate::runtime::spawn({
        let hooks = hooks.clone();
        async move { hooks.sync().await }
    });
    Ok((plugins, hooks))
}
