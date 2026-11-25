//! Local and online services started before peer networking.

use std::sync::Arc;

use caskade_core::{
    AppPaths, accounts::AccountService, artwork::ArtworkCache, bus::Bus, db::LibraryStore,
    settings::SettingsService,
};

use super::{Core, Shell, network::Network};
use crate::{
    commands::{
        addon_packages::AddonFolders, addons::AddonMedia, install_requests::InstallRequests,
    },
    events::Events,
};

/// Every local and online service brought up from [`Core`].
pub(super) struct LocalServices {
    pub(super) paths: AppPaths,
    pub(super) artwork: Option<ArtworkCache>,
    pub(super) bus: Bus,
    pub(super) store: LibraryStore,
    pub(super) accounts: AccountService,
    pub(super) notifications: caskade_achievements::notifications::Notifications,
    pub(super) settings: SettingsService,
    pub(super) launch: caskade_core::launch::LaunchService,
    pub(super) storage: Option<caskade_core::storage::StorageService>,
    pub(super) storage_source: Option<caskade_sources::storage::StorageSource>,
    pub(super) install: Option<caskade_core::install::InstallService>,
    pub(super) install_requests: Option<InstallRequests>,
    pub(super) discovery: caskade_sources::DiscoveryService,
    pub(super) discovery_controller: crate::discovery::DiscoveryController,
    pub(super) emulation: caskade_core::emulation::EmulationService,
    pub(super) emulator_setup: caskade_core::emulation::EmulatorSetup,
    pub(super) tools: Option<caskade_core::managed::ManagedTools>,
    pub(super) streaming_host: Option<caskade_streaming::StreamingHost>,
    pub(super) stream_hosts: Option<caskade_streaming::hosts::StreamHosts>,
    pub(super) streaming_client: Option<caskade_streaming::StreamingClient>,
    pub(super) web: caskade_core::web::WebGames,
    pub(super) metadata: caskade_core::metadata::Metadata,
    pub(super) hasheous: caskade_core::hasheous::Hasheous,
    pub(super) ludusavi: caskade_core::ludusavi::Ludusavi,
    pub(super) ports: caskade_core::ports::Ports,
    pub(super) igdb: caskade_core::igdb::Igdb,
    pub(super) updates: Option<caskade_core::updates::Updates>,
    pub(super) romm: Option<caskade_core::romm::RommService>,
    pub(super) save_versions: caskade_core::saves::versions::SaveVersions,
    pub(super) addon_folders: AddonFolders,
    pub(super) plugins: Option<caskade_plugin_host::PluginHost>,
    pub(super) plugin_hooks: Option<Arc<caskade_plugin_host::PluginHooks>>,
    pub(super) themes: caskade_core::themes::ThemeService,
    pub(super) catalogue: Option<caskade_addons::CatalogueService>,
    pub(super) addon_media: Option<AddonMedia>,
    pub(super) backups: caskade_backup::Backups,
    pub(super) store_accounts: caskade_core::store_accounts::StoreAccounts,
    pub(super) achievements: caskade_achievements::achievements::Achievements,
}

struct StorageDiscovery {
    storage: Option<caskade_core::storage::StorageService>,
    storage_source: Option<caskade_sources::storage::StorageSource>,
    install: Option<caskade_core::install::InstallService>,
    install_requests: Option<InstallRequests>,
    discovery: caskade_sources::DiscoveryService,
    discovery_controller: crate::discovery::DiscoveryController,
}

struct EmulationStreaming {
    emulation: caskade_core::emulation::EmulationService,
    emulator_setup: caskade_core::emulation::EmulatorSetup,
    tools: Option<caskade_core::managed::ManagedTools>,
    streaming_host: Option<caskade_streaming::StreamingHost>,
    stream_hosts: Option<caskade_streaming::hosts::StreamHosts>,
    streaming_client: Option<caskade_streaming::StreamingClient>,
}

struct MetadataUpdates {
    web_player: Option<caskade_core::launch::WebPlayer>,
    web: caskade_core::web::WebGames,
    metadata: caskade_core::metadata::Metadata,
    hasheous: caskade_core::hasheous::Hasheous,
    ludusavi: caskade_core::ludusavi::Ludusavi,
    ports: caskade_core::ports::Ports,
    igdb: caskade_core::igdb::Igdb,
    updates: Option<caskade_core::updates::Updates>,
}

struct PluginsAddons {
    addon_folders: AddonFolders,
    plugins: Option<caskade_plugin_host::PluginHost>,
    plugin_hooks: Option<Arc<caskade_plugin_host::PluginHooks>>,
    catalogue: Option<caskade_addons::CatalogueService>,
    addon_media: Option<AddonMedia>,
}

/// Starts every local and online service on `core`.
pub(super) fn start_local(core: Core, shell: &Shell, network: Network) -> LocalServices {
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
    let sd = start_storage_and_discovery(&paths, &store, &bus, &settings, &launch, &events);
    let es = start_emulation_and_streaming(
        &paths, &store, &bus, &settings, &accounts, &launch, &artwork, shell, network,
    );
    let mu = start_metadata_and_updates(
        &paths,
        &store,
        &bus,
        &settings,
        &artwork,
        &es.emulation,
        sd.install.as_ref(),
        shell,
        network,
    );
    // Emulated games resolve through the emulation service (#121).
    let launch = launch.with_emulators(
        caskade_core::launch::EmulatorLaunches::new(store.clone(), es.emulation.clone())
            .with_settings(settings.clone()),
    );
    let launch = match mu.web_player {
        Some(player) => launch.with_web_player(player),
        None => launch,
    };
    let (launch, romm, save_versions) = start_romm_and_saves(
        &paths,
        &store,
        &bus,
        &settings,
        &accounts,
        &artwork,
        &es.emulation,
        &es.emulator_setup,
        &mu.ludusavi,
        launch,
        &events,
    );
    let pa = start_plugins_and_addons(
        &paths,
        &store,
        &bus,
        &settings,
        &artwork,
        &themes,
        &plugin_runners,
        network,
    );
    // Backups (#191–#193): Settings → Backup, automatic backups, and how a
    // restore before this start ended.
    let backups = caskade_backup::Backups::new(store.clone(), paths.clone());
    backups.set_settings(settings.clone());
    backups.set_last_restore(caskade_backup::restore::take_outcome(&paths));
    crate::commands::backup::setup(events.clone(), &backups);
    let (store_accounts, achievements) = store_accounts(&store, &bus, &settings);
    crate::commands::store_accounts::setup(events, &store_accounts);
    LocalServices {
        paths,
        artwork,
        bus,
        store,
        accounts,
        notifications,
        settings,
        launch,
        storage: sd.storage,
        storage_source: sd.storage_source,
        install: sd.install,
        install_requests: sd.install_requests,
        discovery: sd.discovery,
        discovery_controller: sd.discovery_controller,
        emulation: es.emulation,
        emulator_setup: es.emulator_setup,
        tools: es.tools,
        streaming_host: es.streaming_host,
        stream_hosts: es.stream_hosts,
        streaming_client: es.streaming_client,
        web: mu.web,
        metadata: mu.metadata,
        hasheous: mu.hasheous,
        ludusavi: mu.ludusavi,
        ports: mu.ports,
        igdb: mu.igdb,
        updates: mu.updates,
        romm,
        save_versions,
        addon_folders: pa.addon_folders,
        plugins: pa.plugins,
        plugin_hooks: pa.plugin_hooks,
        themes,
        catalogue: pa.catalogue,
        addon_media: pa.addon_media,
        backups,
        store_accounts,
        achievements,
    }
}

fn start_storage_and_discovery(
    paths: &AppPaths,
    store: &LibraryStore,
    bus: &Bus,
    settings: &SettingsService,
    launch: &caskade_core::launch::LaunchService,
    events: &Events,
) -> StorageDiscovery {
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
        crate::runtime::block_on(crate::onboarding::settle_existing(settings, store))
    {
        tracing::warn!(%error, "first-run setup state unavailable");
    }
    // Installed-games discovery (#100). The discovery controller (#101)
    // owns the initial scan, the schedule and the watch; this only
    // registers the sources and their settings.
    if let Err(error) = caskade_sources::register_settings(settings) {
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
        paths,
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
    StorageDiscovery {
        storage,
        storage_source,
        install,
        install_requests,
        discovery,
        discovery_controller,
    }
}

#[allow(clippy::too_many_arguments)]
fn start_emulation_and_streaming(
    paths: &AppPaths,
    store: &LibraryStore,
    bus: &Bus,
    settings: &SettingsService,
    accounts: &AccountService,
    launch: &caskade_core::launch::LaunchService,
    artwork: &Option<ArtworkCache>,
    shell: &Shell,
    network: Network,
) -> EmulationStreaming {
    // Embedded definitions are validated before constructing the service.
    let emulation = caskade_core::emulation::EmulationService::new(
        store.clone(),
        bus.clone(),
        caskade_sources::emulation::catalog().unwrap_or_else(|error| {
            tracing::error!(%error, "shipped emulation definitions are invalid");
            caskade_core::emulation::Catalog::default()
        }),
    )
    .with_paths(paths);
    // One-click emulator installs, and their update checks, which follow
    // `core.updates`.
    let emulator_setup = caskade_core::emulation::EmulatorSetup::new(
        store.clone(),
        bus.clone(),
        emulation.clone(),
        paths,
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
                paths,
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
                caskade_streaming::elevation::Elevation::new(paths),
                settings.clone(),
                paths,
            );
            crate::commands::streaming::setup(
                host.clone(),
                store.clone(),
                Some(accounts.clone()),
                bus,
                artwork.clone(),
                paths,
            );
            streaming_host = Some(host);
            // Devices to stream from (#262), through Caskade's own client.
            let client = caskade_streaming::StreamingClient::new(managed.clone());
            match caskade_streaming::hosts::StreamHosts::with(
                store.clone(),
                bus.clone(),
                settings.clone(),
                client.clone(),
                network.mdns(),
                caskade_streaming::hosts::HostTiming::default(),
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
    EmulationStreaming {
        emulation,
        emulator_setup,
        tools,
        streaming_host,
        stream_hosts,
        streaming_client,
    }
}

#[allow(clippy::too_many_arguments)]
fn start_metadata_and_updates(
    paths: &AppPaths,
    store: &LibraryStore,
    bus: &Bus,
    settings: &SettingsService,
    artwork: &Option<ArtworkCache>,
    emulation: &caskade_core::emulation::EmulationService,
    install: Option<&caskade_core::install::InstallService>,
    shell: &Shell,
    network: Network,
) -> MetadataUpdates {
    // Offline Hasheous dumps for the configured emulators' platforms and PC
    // games, kept current in the background.
    let hasheous = caskade_core::hasheous::Hasheous::new(paths.cache.join("hasheous"));
    let ludusavi = caskade_core::ludusavi::Ludusavi::new(paths.cache.join("ludusavi"));
    // Fan ports of games: Decomp Buddy's install plans, fetched daily.
    let ports = caskade_core::ports::Ports::new(paths.cache.join("ports"));
    // IGDB metadata from Caskade's metadata server, cached for good.
    let igdb = caskade_core::igdb::Igdb::with_base_url(
        paths.cache.join("igdb"),
        network.server(caskade_core::igdb::BASE_URL),
    );
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
    // `core.metadata.online` allows them, and never on loopback only.
    if network.online_lookups() {
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
    }
    // Refreshing chosen games' details (#176): its status goes to Activity.
    crate::commands::game_edit::forward_refresh(shell.events.clone(), &metadata);
    let updates = install.cloned().map(|install| {
        // Installed ROMs map to the catalogue's platforms (#120).
        install.set_emulation(emulation.clone());
        // Games installed from GitHub are followed for new releases,
        // checked in the background and before they're played.
        let tracker = match network {
            Network::Open => caskade_core::updates::Tracker::builtin(store.clone()),
            Network::Loopback => caskade_core::updates::Tracker::new(
                store.clone(),
                vec![Arc::new(caskade_core::updates::GitHub::new(network.web()))],
            ),
        };
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
    MetadataUpdates {
        web_player,
        web,
        metadata,
        hasheous,
        ludusavi,
        ports,
        igdb,
        updates,
    }
}

#[allow(clippy::too_many_arguments)]
fn start_romm_and_saves(
    paths: &AppPaths,
    store: &LibraryStore,
    bus: &Bus,
    settings: &SettingsService,
    accounts: &AccountService,
    artwork: &Option<ArtworkCache>,
    emulation: &caskade_core::emulation::EmulationService,
    emulator_setup: &caskade_core::emulation::EmulatorSetup,
    ludusavi: &caskade_core::ludusavi::Ludusavi,
    launch: caskade_core::launch::LaunchService,
    events: &Events,
) -> (
    caskade_core::launch::LaunchService,
    Option<caskade_core::romm::RommService>,
    caskade_core::saves::versions::SaveVersions,
) {
    // RomM servers (#1): their ROMs download into the cache at launch.
    let (launch, romm) = match crate::commands::romm::setup(
        events.clone(),
        store.clone(),
        bus.clone(),
        settings,
        emulation.clone(),
        paths,
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
        versions.attach(bus);
        versions
    };
    (launch, romm, save_versions)
}

#[allow(clippy::too_many_arguments)]
fn start_plugins_and_addons(
    paths: &AppPaths,
    store: &LibraryStore,
    bus: &Bus,
    settings: &SettingsService,
    artwork: &Option<ArtworkCache>,
    themes: &caskade_core::themes::ThemeService,
    plugin_runners: &caskade_plugin_host::providers::PluginRunners,
    network: Network,
) -> PluginsAddons {
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
    let (plugins, plugin_hooks) = match start_plugins(paths, store, bus, settings) {
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
        network.server(caskade_addons::DEFAULT_ORIGIN),
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
    PluginsAddons {
        addon_folders,
        plugins,
        plugin_hooks,
        catalogue,
        addon_media,
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
    found.register_provider(retro);
    found.register_provider(Arc::new(achievements::steam::SteamAchievements::new()));
    found.register_provider(Arc::new(achievements::gog::GogAchievements::new()));
    found.register_provider(Arc::new(achievements::epic::EpicAchievements::new()));
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
