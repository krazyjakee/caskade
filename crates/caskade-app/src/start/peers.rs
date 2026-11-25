//! Peer-to-peer services: paired devices, copies, shares, remote play, sync,
//! presence and peer backups.

use std::sync::Arc;

use super::{local::LocalServices, network::Network};
use crate::events::Events;

/// Every peer-connected service brought up on top of [`LocalServices`].
pub(super) struct PeerServices {
    pub(super) devices: caskade_core::devices::Devices,
    pub(super) linked_pairing: caskade_streaming::linked::LinkedPairing,
    pub(super) transfers: Option<caskade_core::transfer::Transfers>,
    pub(super) shares: Option<caskade_share::Shares>,
    pub(super) remote_play: caskade_core::remote_play::RemotePlay,
    pub(super) sync: caskade_core::sync::SyncService,
    pub(super) presence: caskade_core::sync::presence::Presence,
    pub(super) peer_backups: caskade_backup::peers::PeerBackups,
}

/// Starts paired devices, transfers, shares, remote play, sync, presence and
/// peer backups, wiring them to the local services and UI event bridges.
pub(super) fn start_peers(local: &LocalServices, events: Events, network: Network) -> PeerServices {
    // Paired devices (#224–#226): the device key from the keychain (off
    // this thread: a locked keychain holds only sync), the endpoint and
    // pairing.
    let keys = Arc::new(caskade_core::devices::Keychain);
    let devices = match network.sync_reach() {
        Some(reach) => caskade_core::devices::Devices::with_reach(
            local.store.clone(),
            local.accounts.clone(),
            keys,
            reach,
        ),
        None => {
            caskade_core::devices::Devices::new(local.store.clone(), local.accounts.clone(), keys)
        }
    };
    devices.set_settings(local.settings.clone());
    // Streaming pairs through sync with no code (#267): this host enters a
    // paired device's code; this client hands its over.
    if let Some(host) = &local.streaming_host {
        devices.set_stream_handler(Arc::new(caskade_streaming::linked::HostHandover::new(
            Arc::new(host.clone()),
        )));
    }
    let linked_pairing = caskade_streaming::linked::LinkedPairing::new(devices.clone());
    if let Some(hosts) = &local.stream_hosts {
        hosts.set_devices(devices.clone());
    }
    // Copies of games between devices (#233): served with the endpoint, so
    // set up before it starts.
    let mut peer_downloads = None;
    let mut shares = None;
    let transfers = match crate::runtime::block_on(caskade_core::transfer::Transfers::new(
        local.store.clone(),
        devices.clone(),
        &local.paths.cache,
    )) {
        Ok(transfers) => {
            // Downloads come from a device nearby that has them before the
            // internet (#236).
            let downloads = caskade_core::install::nearby::PeerDownloads::new(
                transfers.clone(),
                devices.clone(),
                local.settings.clone(),
                &local.paths.cache,
            );
            if let Some(install) = &local.install {
                install.set_peer_fetcher(Arc::new(transfers.clone()));
                install.set_local_source(Arc::new(downloads.clone()));
            }
            // And emulators, cores, tools, BIOS files, Hasheous dumps and
            // the save manifest (#277).
            local.emulator_setup.set_nearby(downloads.clone());
            if let Some(tools) = &local.tools {
                tools.set_nearby(downloads.clone());
            }
            local.hasheous.set_nearby(downloads.clone());
            local.ludusavi.set_nearby(downloads.clone());
            peer_downloads = Some(downloads);
            // A game another device sends here (#235): confirmed here
            // first, unless copies are accepted without asking.
            crate::commands::copies::take_sends(
                events.clone(),
                &transfers,
                devices.clone(),
                local.settings.clone(),
                local.install.clone(),
                local.install_requests.clone(),
            );
            // One-off shares (#246): admitted and served by the same
            // endpoint, so set up before it starts too.
            if let Some(install) = &local.install {
                let one_off = caskade_share::Shares::new(
                    local.store.clone(),
                    devices.clone(),
                    transfers.clone(),
                    install.clone(),
                    &local.paths.cache,
                );
                // What else can be shared (#248): save versions, and a
                // ROM's emulator, which emulator setup installs.
                one_off.set_saves(local.save_versions.clone());
                one_off.set_emulator_installer(Arc::new(local.emulator_setup.clone()));
                if let Some(requests) = &local.install_requests {
                    requests.set_shares(one_off.clone());
                }
                // Someone receiving a share is a notification.
                one_off.set_bus(local.bus.clone());
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
    let remote_play =
        caskade_core::remote_play::RemotePlay::new(local.store.clone(), devices.clone());
    remote_play.set_settings(local.settings.clone());
    crate::commands::remote_play::setup(events.clone(), &remote_play, devices.clone());
    // Library sync with paired devices (#228–#230), in the background.
    let sync = caskade_core::sync::SyncService::new(local.store.clone(), devices.clone());
    sync.set_settings(local.settings.clone());
    // A RomM server removed on another device: its ROMs and games go here
    // too, through RomM's own removal.
    if let Some(romm) = local.romm.clone() {
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
        local.save_versions.clone(),
    )));
    // This device's default admin joining one from another device hands the
    // signed-in account over (#273).
    let joined = local.accounts.clone();
    sync.on_account_joined(move |from, into| {
        let joined = joined.clone();
        crate::runtime::spawn(async move { joined.joined(&from, &into).await });
    });
    // Games from another device are filled in here: metadata and artwork
    // stay on each device (#75). Matching runs only while online lookups
    // are allowed.
    let arrived = local.metadata.clone();
    sync.on_games_arrived(move || arrived.wake());
    // The user's own artwork travels with its files (#273).
    if let Some(cache) = local.artwork.clone() {
        sync.set_artwork_cache(cache);
    }
    local
        .save_versions
        .set_remote(caskade_core::saves::versions::SyncRefresh::new(
            sync.clone(),
            devices.clone(),
        ));
    let engine = sync.clone();
    local
        .save_versions
        .set_device(move || engine.engine().map(|e| e.device().to_owned()));
    // What's running on which device (#240): told at the end of each
    // exchange, and a game starting or ending here syncs at once.
    let presence = caskade_core::sync::presence::Presence::new(local.store.clone());
    let active = local.accounts.clone();
    presence.set_account(move || active.active_id());
    sync.set_presence(presence.clone());
    // Backups kept on a paired device (#243): sent to the chosen one after
    // the changes, and held here for the others.
    let peer_backups = caskade_backup::peers::PeerBackups::new(
        local.settings.clone(),
        local.paths.data.join("peer-backups"),
        local.backups.default_destination(),
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
            local.store.clone(),
            local.emulation.catalog(),
            &local.paths.data,
        ),
    ));
    // An emulator installed from another device's preference becomes the
    // platform's default at once (#275).
    crate::background::supervise("preferred emulators", {
        let (bus, sync) = (local.bus.clone(), sync.clone());
        move || caskade_core::sync::offers::watch_emulators(&bus, sync.clone())
    });
    crate::background::supervise("presence", {
        let (presence, bus, sync, devices) = (
            presence.clone(),
            local.bus.clone(),
            sync.clone(),
            devices.clone(),
        );
        move || presence.watch(&bus, sync.clone(), devices.clone())
    });
    crate::commands::presence::setup(
        events.clone(),
        &presence,
        devices.clone(),
        local.store.clone(),
    );
    crate::commands::sync::setup(events.clone(), &devices, &sync);
    crate::commands::saves::forward_changes(events, &local.save_versions);
    PeerServices {
        devices,
        linked_pairing,
        transfers,
        shares,
        remote_play,
        sync,
        presence,
        peer_backups,
    }
}
