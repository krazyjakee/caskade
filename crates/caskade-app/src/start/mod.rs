//! Starting the app (#86): every service the app runs, built and wired the
//! same way whichever shell runs them. [`open`] brings up the library and
//! what the window needs before it shows; [`start`] everything else. The
//! shell then holds the [`Services`] and adds what only it can do: windows,
//! the tray, its updater.

mod local;
mod network;
mod peers;

use std::sync::Arc;

use caskade_core::{
    AppPaths, accounts::AccountService, artwork::ArtworkCache, bus::Bus, db::LibraryStore,
    launch::WindowPlacement, settings::SettingsService,
};

use self::{local::start_local, network::Network, peers::start_peers};
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
    pub companion_control: caskade_core::companion_control::CompanionControl,
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
    // What the services may reach (#167), decided once for all of them.
    let network = Network::of_this_run();
    if network == Network::Loopback {
        tracing::info!("loopback only: online lookups, update checks and mDNS stay off");
    }
    let local = start_local(core, &shell, network);
    let peers = start_peers(&local, shell.events, network);
    Services {
        edit_artwork: EditArtwork(local.artwork.clone()),
        achievement_icons: AchievementIcons(local.artwork),
        paths: local.paths,
        bus: local.bus,
        store: local.store,
        accounts: local.accounts,
        notifications: local.notifications,
        settings: local.settings,
        launch: local.launch,
        storage: local.storage,
        storage_source: local.storage_source,
        install: local.install,
        install_requests: local.install_requests,
        discovery: local.discovery,
        discovery_controller: local.discovery_controller,
        emulation: local.emulation,
        emulator_setup: local.emulator_setup,
        tools: local.tools,
        streaming_host: local.streaming_host,
        stream_hosts: local.stream_hosts,
        streaming_client: local.streaming_client,
        web: local.web,
        metadata: local.metadata,
        hasheous: local.hasheous,
        ludusavi: local.ludusavi,
        ports: local.ports,
        igdb: local.igdb,
        updates: local.updates,
        romm: local.romm,
        save_versions: local.save_versions,
        addon_folders: local.addon_folders,
        addon_jobs: AddonJobs::default(),
        plugins: local.plugins,
        plugin_hooks: local.plugin_hooks,
        themes: local.themes,
        catalogue: local.catalogue,
        addon_media: local.addon_media,
        backups: local.backups,
        store_accounts: local.store_accounts,
        achievements: local.achievements,
        devices: peers.devices,
        linked_pairing: peers.linked_pairing,
        transfers: peers.transfers,
        shares: peers.shares,
        remote_play: peers.remote_play,
        companion_control: peers.companion_control,
        sync: peers.sync,
        presence: peers.presence,
        peer_backups: peers.peer_backups,
        opened_pairing_link: Default::default(),
        dev_plugins: Default::default(),
    }
}
