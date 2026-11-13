//! Pairing (#226): by a code both devices show, or by a link one device
//! mints and another uses once.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use caskade_sync::pairing::pair_with_link;
use caskade_sync::{
    CodePairing, DeviceInfo, EndpointAddr, EndpointId, Grant, LINK_LIFETIME, Paired, PairingLink,
    Purpose, TransportAddr,
};
use tokio::sync::oneshot;

use super::endpoint::addr_text;
use super::{
    DeviceKind, DeviceOs, Devices, Inner, PairedDevice, Pairing, PairingLinkInfo,
    PairingLinkTarget, PairingState, name_of, parse_id,
};
use crate::db::PairedDeviceRecord;
use crate::{Error, Result, now_ms};

/// How long a pairing link's address waits for a relay before it's minted
/// with local addresses only.
const LINK_ADDR_WAIT: Duration = Duration::from_secs(2);

impl Devices {
    /// Starts pairing with a nearby device: both show the code.
    pub async fn pair_nearby(&self, device_id: &str) -> Result<Pairing> {
        self.admin()?;
        let peer = parse_id(device_id)?;
        let endpoint = self.inner.endpoint()?;
        if !self.inner.gate.reserve_pairing() {
            return Err(Error::Invalid(crate::tr!("another pairing is in progress")));
        }
        let addrs = endpoint
            .nearby()
            .into_iter()
            .find(|nearby| nearby.id == peer)
            .map(|nearby| nearby.addrs)
            .unwrap_or_default();
        let addr = EndpointAddr::from_parts(peer, addrs.into_iter().map(TransportAddr::Ip));
        let me = self.inner.me().await;
        let begun = async {
            let session = endpoint.connect(addr, Purpose::Pair, None).await?;
            CodePairing::begin(session, &me).await
        }
        .await;
        let pairing = match begun {
            Ok(pairing) => pairing,
            Err(error) => {
                self.inner.gate.release_pairing();
                return Err(pairing_error(error));
            }
        };
        let decision = self.inner.show_pairing(&pairing, false);
        let inner = self.inner.clone();
        tokio::spawn(async move { inner.finish_pairing(pairing, decision).await });
        self.inner
            .pairing()
            .ok_or_else(|| Error::Invalid(crate::tr!("pairing was cancelled")))
    }

    /// This user confirms the codes match.
    pub async fn confirm_pairing(&self, device_id: &str) -> Result<()> {
        self.admin()?;
        self.inner.decide(device_id, true)
    }

    /// This user cancels the pairing, or dismisses a finished one.
    pub async fn cancel_pairing(&self, device_id: &str) -> Result<()> {
        self.admin()?;
        self.inner.decide(device_id, false)
    }

    /// A link that pairs another device with this one from anywhere. It works
    /// once, for ten minutes.
    pub async fn pairing_link(&self) -> Result<PairingLinkInfo> {
        self.admin()?;
        let endpoint = self.inner.endpoint()?;
        let grant = Grant::generate();
        let expires = SystemTime::now() + LINK_LIFETIME;
        self.inner.gate.offer_link(grant, expires);
        let addr = endpoint.shareable_addr(LINK_ADDR_WAIT).await;
        let link = PairingLink::new(addr, grant, expires);
        Ok(PairingLinkInfo {
            url: link.to_url(),
            expires_at_ms: millis(expires),
        })
    }

    /// Pairs with the device a pasted pairing link names.
    pub async fn accept_pairing_link(&self, text: &str) -> Result<PairedDevice> {
        self.admin()?;
        let link = PairingLink::parse(text).map_err(pairing_error)?;
        let endpoint = self.inner.endpoint()?;
        if link.peer() == endpoint.id() {
            return Err(Error::Invalid(crate::tr!(
                "that pairing link is this device's own"
            )));
        }
        let me = self.inner.me().await;
        let paired = pair_with_link(&endpoint, &link, &me, Grant::generate())
            .await
            .map_err(pairing_error)?;
        self.inner.store_paired(&paired).await?;
        self.inner
            .paired_device(&paired.peer.to_string())
            .await
            .ok_or_else(|| Error::Invalid(crate::tr!("pairing was cancelled")))
    }
}

impl Inner {
    pub(super) fn pairing(&self) -> Option<Pairing> {
        self.lock().pairing.as_ref().map(ActivePairing::view)
    }

    /// Puts a pairing's codes on screen; the receiver gets this user's
    /// decision.
    pub(super) fn show_pairing(
        &self,
        pairing: &CodePairing,
        incoming: bool,
    ) -> oneshot::Receiver<bool> {
        let (decide, decision) = oneshot::channel();
        self.lock().pairing = Some(ActivePairing {
            peer: pairing.peer(),
            info: pairing.peer_info().clone(),
            code: pairing.code().to_owned(),
            incoming,
            confirmed: false,
            state: PairingState::Waiting,
            decide: Some(decide),
        });
        self.changed();
        decision
    }

    pub(super) async fn finish_pairing(
        &self,
        pairing: CodePairing,
        decision: oneshot::Receiver<bool>,
    ) {
        let peer = pairing.peer();
        let result = pairing
            .finish(async { decision.await.unwrap_or(false) }, Grant::generate())
            .await;
        let outcome = match result {
            Ok(paired) => match self.store_paired(&paired).await {
                Ok(()) => PairingState::Paired,
                Err(error) => {
                    tracing::warn!(%error, "pairing not stored");
                    PairingState::Failed
                }
            },
            Err(caskade_sync::Error::Cancelled) => PairingState::Cancelled,
            Err(error) => {
                tracing::info!(%error, "pairing ended");
                PairingState::Failed
            }
        };
        self.gate.release_pairing();
        if let Some(active) = self.lock().pairing.as_mut().filter(|p| p.peer == peer) {
            active.state = outcome;
            active.decide = None;
        }
        self.changed();
    }

    pub(super) fn decide(&self, device_id: &str, confirmed: bool) -> Result<()> {
        let mut state = self.lock();
        let Some(active) = state
            .pairing
            .as_mut()
            .filter(|p| p.peer.to_string() == device_id)
        else {
            return Err(Error::Invalid(crate::tr!("no pairing with that device")));
        };
        match active.decide.take() {
            Some(decide) => {
                let _ = decide.send(confirmed);
                active.confirmed = confirmed;
                if !confirmed {
                    active.state = PairingState::Cancelled;
                }
            }
            // Finished: cancelling dismisses it.
            None if !confirmed => state.pairing = None,
            None => {}
        }
        drop(state);
        self.changed();
        Ok(())
    }

    pub(super) async fn store_paired(&self, paired: &Paired) -> Result<()> {
        let record = PairedDeviceRecord {
            endpoint_id: paired.peer.to_string(),
            name: paired.info.name.clone(),
            kind: name_of(&DeviceKind::from(paired.info.kind)),
            os: name_of(&DeviceOs::from(paired.info.os)),
            addresses: paired.addr.addrs.iter().map(addr_text).collect(),
            wol_macs: Vec::new(),
            last_sync_ms: None,
            grant_in: paired.grant_in.to_hex(),
            grant_out: paired.grant_out.to_hex(),
            paired_at_ms: now_ms(),
            forgotten_at_ms: None,
        };
        self.store.save_paired_device(&record).await?;
        self.gate.pair(paired.peer, paired.grant_in);
        self.changed();
        Ok(())
    }
}

/// Reads a pairing link without using it.
pub fn inspect_link(text: &str) -> Result<PairingLinkTarget> {
    let link = PairingLink::parse(text).map_err(pairing_error)?;
    Ok(PairingLinkTarget {
        device_id: link.peer().to_string(),
        expires_at_ms: millis(link.expires_at()),
        expired: link.expired(SystemTime::now()),
    })
}

pub(super) struct ActivePairing {
    peer: EndpointId,
    info: DeviceInfo,
    code: String,
    incoming: bool,
    confirmed: bool,
    state: PairingState,
    decide: Option<oneshot::Sender<bool>>,
}

impl ActivePairing {
    pub(super) fn view(&self) -> Pairing {
        Pairing {
            device_id: self.peer.to_string(),
            name: self.info.name.clone(),
            kind: self.info.kind.into(),
            os: self.info.os.into(),
            code: self.code.clone(),
            incoming: self.incoming,
            confirmed: self.confirmed,
            state: self.state,
        }
    }
}

/// The user-facing error for a pairing failure.
fn pairing_error(error: caskade_sync::Error) -> Error {
    use caskade_sync::Error as E;
    Error::Invalid(match error {
        E::NotALink => crate::tr!("that isn't a Caskade pairing link"),
        E::LinkExpired => {
            crate::tr!("this pairing link has expired; make a new one on the other device")
        }
        E::LinkRefused => crate::tr!(
            "this pairing link was already used or has expired; make a new one on the other device"
        ),
        E::Refused => crate::tr!("the other device isn't accepting a pairing right now"),
        E::Cancelled => crate::tr!("pairing was cancelled"),
        _ => crate::tr!("couldn't reach the other device"),
    })
}

fn millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
    })
}
