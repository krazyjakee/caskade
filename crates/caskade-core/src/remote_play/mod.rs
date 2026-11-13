//! Play a game on another device (#234), waking it first.
//!
//! The requesting device opens a `play` session to a paired device (its own
//! grant; nobody else gets one) and names the game in its own ids. The
//! target maps it to its game, refuses when `core.sync.remotePlay` is off or
//! the game isn't in its library, and hands it to the app, which starts it as
//! a Play here would: the active account, its age limit (the admin PIN
//! asked on the target), console mode. The outcome goes back on the same
//! session; the game running there then shows on the requester through
//! presence (#240).
//!
//! A device out of reach is woken first when its Wake-on-LAN addresses are
//! known: each device sends its wired and wireless MAC addresses at the start
//! of every sync (`sync::exchange`), and Wake sends a magic packet to every
//! local broadcast address and waits for it to come into reach.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use caskade_sync::{Session, Sessions};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::db::LibraryStore;
use crate::devices::{ConnectionPurpose, DeviceLink, Devices};
use crate::settings::SettingsService;
use crate::sync::policy::SyncSettings;
use crate::{Error, Result};

const MAX_LINE: usize = 16 * 1024;
/// How long the target's user gets to start it (a PIN to type, say).
pub const START_WAIT: Duration = Duration::from_secs(180);
/// How long a woken device gets to come into reach.
pub const WAKE_WAIT: Duration = Duration::from_secs(120);

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message {
    /// Requester: start this game (its id there).
    Play { game: String },
    /// Target: taken; starting it.
    Accepted,
    /// Target: it's running.
    Started,
    /// Target: not this one, and why (the user's words).
    Refused { reason: String },
}

/// A play request another device made here, for the app to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayRequest {
    pub id: String,
    pub device_id: String,
    /// The game here.
    pub game_id: String,
}

/// How starting a requested game went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayOutcome {
    Started,
    /// It didn't start, and why (declined, a PIN not given, a failure).
    NotStarted(String),
}

/// How a request to another device is going, for its UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayProgress {
    Waking,
    Starting,
}

type Handler = Arc<dyn Fn(PlayRequest) + Send + Sync>;

/// Play requests both ways. Cheap to clone.
#[derive(Clone)]
pub struct RemotePlay {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    devices: Devices,
    settings: std::sync::OnceLock<SettingsService>,
    handler: std::sync::OnceLock<Handler>,
    waiting: Mutex<HashMap<String, oneshot::Sender<PlayOutcome>>>,
}

impl RemotePlay {
    /// Play requests over `devices`' endpoint; takes the `play` sessions
    /// paired devices open.
    pub fn new(store: LibraryStore, devices: Devices) -> Self {
        let play = Self {
            inner: Arc::new(Inner {
                store,
                devices: devices.clone(),
                settings: std::sync::OnceLock::new(),
                handler: std::sync::OnceLock::new(),
                waiting: Mutex::default(),
            }),
        };
        devices.set_play_handler(Arc::new(Serve(Arc::downgrade(&play.inner))));
        play
    }

    /// Reads `core.sync.remotePlay` from `settings`.
    pub fn set_settings(&self, settings: SettingsService) {
        let _ = self.inner.settings.set(settings);
    }

    /// Where requests from other devices go: the app starts the game and
    /// says how it went with [`RemotePlay::finished`].
    pub fn on_request(&self, handler: impl Fn(PlayRequest) + Send + Sync + 'static) {
        let _ = self.inner.handler.set(Arc::new(handler));
    }

    /// How the app's start of request `id` went.
    pub fn finished(&self, id: &str, outcome: PlayOutcome) {
        let sender = crate::lock(&self.inner.waiting).remove(id);
        if let Some(sender) = sender {
            let _ = sender.send(outcome);
        }
    }

    /// Starts `game_id` (this device's id) on `device_id`, waking it first
    /// when it's out of reach and its Wake-on-LAN addresses are known.
    pub async fn play_on(
        &self,
        device_id: &str,
        game_id: &str,
        progress: impl Fn(PlayProgress) + Send,
    ) -> Result<()> {
        let status = self.inner.devices.status().await;
        let device = status
            .devices
            .iter()
            .find(|device| device.id == device_id)
            .ok_or_else(|| Error::Invalid(crate::tr!("that device isn't paired")))?;
        let name = device.name.clone();
        if device.link == DeviceLink::Offline {
            let macs = self.inner.store.paired_device_macs(device_id).await?;
            if macs.is_empty() {
                return Err(Error::Invalid(crate::tr!(
                    "{device} is out of reach, and it can't be woken from here",
                    device = name.as_str()
                )));
            }
            progress(PlayProgress::Waking);
            let targets = wake_targets(&[]);
            for mac in macs.iter().filter_map(|mac| parse_mac(mac)) {
                if let Err(error) = wake(mac, &targets).await {
                    tracing::warn!(%error, "Wake-on-LAN");
                }
            }
            self.wait_for(device_id).await?;
        }
        progress(PlayProgress::Starting);
        let mut session = self
            .inner
            .devices
            .connect(device_id, ConnectionPurpose::Play)
            .await?;
        let lost = || {
            Error::Invalid(crate::tr!(
                "the connection to {device} was lost",
                device = name.as_str()
            ))
        };
        session
            .writer()
            .write(&Message::Play {
                game: game_id.to_owned(),
            })
            .await
            .map_err(|_| lost())?;
        loop {
            let answer = tokio::time::timeout(
                START_WAIT + Duration::from_secs(10),
                session.reader().read(MAX_LINE),
            )
            .await;
            match answer {
                Ok(Ok(Message::Accepted)) => continue,
                Ok(Ok(Message::Started)) => {
                    session.end().await;
                    return Ok(());
                }
                Ok(Ok(Message::Refused { reason })) => {
                    session.end().await;
                    return Err(Error::Invalid(reason));
                }
                _ => {
                    session.end().await;
                    return Err(lost());
                }
            }
        }
    }

    /// Waits for `device_id` to come into reach after a wake.
    async fn wait_for(&self, device_id: &str) -> Result<()> {
        let deadline = tokio::time::Instant::now() + WAKE_WAIT;
        loop {
            let status = self.inner.devices.status().await;
            if status
                .devices
                .iter()
                .any(|device| device.id == device_id && device.link != DeviceLink::Offline)
            {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Invalid(crate::tr!(
                    "the device didn't wake up; check that Wake-on-LAN is on in its settings"
                )));
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn allowed(&self) -> bool {
        SyncSettings::read(self.inner.settings.get())
            .await
            .remote_play
    }

    async fn serve(&self, mut session: Session) {
        let peer = session.peer().to_string();
        let Ok(Message::Play { game }) = session.reader().read(MAX_LINE).await else {
            return session.end().await;
        };
        let refuse = |reason: String| Message::Refused { reason };
        let answer = if !self.allowed().await {
            refuse(crate::tr!("this device doesn't take play requests"))
        } else {
            match self.inner.store.sync_mapped_game(&peer, &game).await {
                Ok(Some(game_id)) => match self.inner.handler.get() {
                    Some(handler) => {
                        let id = crate::model::new_id();
                        let (sender, receiver) = oneshot::channel();
                        crate::lock(&self.inner.waiting).insert(id.clone(), sender);
                        let _ = session.writer().write(&Message::Accepted).await;
                        handler(PlayRequest {
                            id: id.clone(),
                            device_id: peer,
                            game_id,
                        });
                        let outcome = tokio::time::timeout(START_WAIT, receiver).await;
                        crate::lock(&self.inner.waiting).remove(&id);
                        match outcome {
                            Ok(Ok(PlayOutcome::Started)) => Message::Started,
                            Ok(Ok(PlayOutcome::NotStarted(reason))) => refuse(reason),
                            _ => refuse(crate::tr!("nobody started it on that device in time")),
                        }
                    }
                    None => refuse(crate::tr!("this device doesn't take play requests")),
                },
                _ => refuse(crate::tr!("that game isn't in this device's library")),
            }
        };
        let _ = session.writer().write(&answer).await;
        session.end().await;
    }
}

struct Serve(Weak<Inner>);

#[async_trait]
impl Sessions for Serve {
    async fn opened(&self, session: Session) {
        let Some(inner) = self.0.upgrade() else {
            return session.end().await;
        };
        RemotePlay { inner }.serve(session).await;
    }
}

/// This device's wired and wireless MAC addresses, `aa:bb:cc:dd:ee:ff`: what
/// another device wakes it with.
pub fn local_macs() -> Vec<String> {
    let mut macs: Vec<String> = netdev::get_interfaces()
        .into_iter()
        .filter(|interface| !interface.is_loopback())
        .filter(|interface| {
            matches!(
                interface.if_type,
                netdev::interface::types::InterfaceType::Ethernet
                    | netdev::interface::types::InterfaceType::Wireless80211
            )
        })
        .filter_map(|interface| interface.mac_addr)
        .map(|mac| mac.to_string().to_lowercase())
        .filter(|mac| mac != "00:00:00:00:00:00")
        .collect();
    macs.sort();
    macs.dedup();
    macs
}

/// A MAC address from `aa:bb:cc:dd:ee:ff` (or with dashes).
fn parse_mac(text: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = text.split([':', '-']).collect();
    let [a, b, c, d, e, f] = parts[..] else {
        return None;
    };
    let byte = |part: &str| u8::from_str_radix(part, 16).ok();
    Some([byte(a)?, byte(b)?, byte(c)?, byte(d)?, byte(e)?, byte(f)?])
}

/// A Wake-on-LAN magic packet for `mac`: six 0xff bytes, then the address
/// sixteen times.
pub fn magic_packet(mac: [u8; 6]) -> [u8; 102] {
    let mut packet = [0xffu8; 102];
    for chunk in packet[6..].chunks_mut(6) {
        chunk.copy_from_slice(&mac);
    }
    packet
}

/// The ports Moonlight sends magic packets to: the usual ones and the
/// streaming ports, which a router may forward.
pub const WAKE_PORTS: [u16; 6] = [7, 9, 47998, 47999, 48000, 48010];

/// Where the magic packet for a host at `addresses` goes: every network's
/// broadcast address and each address, on each of [`WAKE_PORTS`]. The
/// all-networks address alone leaves by the default route, which on a machine
/// with a second card or a VPN isn't the network the host sleeps on.
pub fn wake_targets(addresses: &[IpAddr]) -> Vec<SocketAddr> {
    targets_through(&network_broadcasts(), addresses)
}

fn targets_through(broadcasts: &[Ipv4Addr], addresses: &[IpAddr]) -> Vec<SocketAddr> {
    std::iter::once(Ipv4Addr::BROADCAST)
        .chain(broadcasts.iter().copied())
        .map(IpAddr::V4)
        .chain(addresses.iter().copied().filter(IpAddr::is_ipv4))
        .flat_map(|address| WAKE_PORTS.map(|port| SocketAddr::new(address, port)))
        .collect()
}

/// The broadcast address of each network this machine is on.
fn network_broadcasts() -> Vec<Ipv4Addr> {
    let mut broadcasts: Vec<Ipv4Addr> = netdev::get_interfaces()
        .iter()
        .filter(|interface| !interface.is_loopback())
        .flat_map(|interface| interface.ipv4.iter().map(|net| net.broadcast()))
        .collect();
    broadcasts.sort();
    broadcasts.dedup();
    broadcasts
}

/// Sends the magic packet for `mac` to `targets`.
pub async fn wake(mac: [u8; 6], targets: &[SocketAddr]) -> Result<()> {
    let socket =
        tokio::net::UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)).await?;
    socket.set_broadcast(true)?;
    let packet = magic_packet(mac);
    let mut sent = false;
    for target in targets {
        sent |= socket.send_to(&packet, target).await.is_ok();
    }
    if sent {
        Ok(())
    } else {
        Err(Error::Invalid(crate::tr!(
            "the wake-up message couldn't be sent"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_magic_packet_is_six_ffs_and_sixteen_addresses() {
        let mac = parse_mac("a0:B1:c2:d3:e4:f5").unwrap();
        assert_eq!(mac, [0xa0, 0xb1, 0xc2, 0xd3, 0xe4, 0xf5]);
        let packet = magic_packet(mac);
        assert_eq!(&packet[..6], &[0xff; 6]);
        for i in 0..16 {
            assert_eq!(&packet[6 + i * 6..12 + i * 6], &mac);
        }
        assert_eq!(parse_mac("a0-b1-c2-d3-e4-f5"), Some(mac));
        assert_eq!(parse_mac("nope"), None);
        assert_eq!(parse_mac("a0:b1:c2:d3:e4"), None);
    }

    #[test]
    fn a_wake_up_leaves_by_every_network_not_just_the_default_route() {
        let targets = targets_through(
            &[Ipv4Addr::new(192, 168, 77, 255)],
            &[IpAddr::from([192, 168, 77, 12])],
        );
        for address in [
            [255, 255, 255, 255],
            [192, 168, 77, 255],
            [192, 168, 77, 12],
        ] {
            let target = SocketAddr::new(IpAddr::from(address), 9);
            assert!(targets.contains(&target), "{target} in {targets:?}");
        }
    }
}
