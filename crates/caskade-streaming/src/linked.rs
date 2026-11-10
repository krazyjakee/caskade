//! Pairing streaming through sync (#267): devices
//! already paired through sync (#226) stream to each other with no code.
//!
//! The client starts the usual pairing (§6.2) with its own device name and
//! a fresh code, then hands the code to its paired devices over
//! `caskade/sync/1` (purpose `stream`, which only paired devices are
//! admitted to). The device whose host is the one being paired with waits
//! for the request with that name and enters the code itself; every other
//! device answers that it isn't the one. A device not paired through sync
//! can't open the session, so it still needs the code entered by hand.
//!
//! One message each way, one JSON line: the client's [`Message::Pair`],
//! then the host's [`Message::Accepted`], [`Message::NotHere`] or
//! [`Message::Refused`].

use super::{
    api::PairRequest,
    host::{HostState, StreamingHost},
};
use async_trait::async_trait;
use caskade_core::{
    Result,
    devices::{ConnectionPurpose, DeviceSession, Devices},
};
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// What the handover needs of this device's host ([`StreamingHost`]; a
/// fake in tests).
#[async_trait]
pub trait PairingHost: Send + Sync {
    /// It lets other devices play now.
    async fn ready(&self) -> bool;
    /// Its streaming id ([`StreamingHost::host_id`]).
    async fn host_id(&self) -> Option<String>;
    async fn pair_requests(&self) -> Result<Vec<PairRequest>>;
    async fn accept_pairing(&self, id: &str, code: &str, name: &str) -> Result<()>;
}

#[async_trait]
impl PairingHost for StreamingHost {
    async fn ready(&self) -> bool {
        self.status().state == HostState::Ready
    }

    async fn host_id(&self) -> Option<String> {
        StreamingHost::host_id(self).await
    }

    async fn pair_requests(&self) -> Result<Vec<PairRequest>> {
        StreamingHost::pair_requests(self).await
    }

    async fn accept_pairing(&self, id: &str, code: &str, name: &str) -> Result<()> {
        StreamingHost::accept_pairing(self, id, code, name).await
    }
}

/// A line's bound.
const MAX_LINE: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Message {
    /// Client to host: enter `code` for the pairing request named
    /// `request`, to host `host` (its streaming id), and call the device
    /// `name`.
    Pair {
        host: String,
        request: String,
        code: String,
        name: String,
    },
    /// The code was entered: pairing goes on as usual.
    Accepted,
    /// This device isn't that host, or doesn't let others play now.
    NotHere,
    /// It is that host, and entering the code failed.
    Refused { reason: String },
}

/// How long the host waits for the request to arrive, and the client for
/// an answer.
#[derive(Debug, Clone, Copy)]
pub struct LinkTiming {
    pub request: Duration,
    pub answer: Duration,
}

impl Default for LinkTiming {
    fn default() -> Self {
        Self {
            request: Duration::from_secs(10),
            answer: Duration::from_secs(20),
        }
    }
}

/// The host's side: `stream` sessions from paired devices.
pub struct HostHandover {
    host: Arc<dyn PairingHost>,
    timing: LinkTiming,
}

impl HostHandover {
    pub fn new(host: Arc<dyn PairingHost>) -> Self {
        Self::with(host, LinkTiming::default())
    }

    pub fn with(host: Arc<dyn PairingHost>, timing: LinkTiming) -> Self {
        Self { host, timing }
    }

    /// The answer to a paired device's [`Message::Pair`].
    pub async fn answer(&self, message: Message) -> Message {
        let Message::Pair {
            host,
            request,
            code,
            name,
        } = message
        else {
            return Message::NotHere;
        };
        if !self.host.ready().await {
            return Message::NotHere;
        }
        match self.host.host_id().await {
            Some(id) if id.eq_ignore_ascii_case(&host) => {}
            _ => return Message::NotHere,
        }
        // The client asks the host a moment after it asked us.
        let deadline = Instant::now() + self.timing.request;
        let id = loop {
            match self.host.pair_requests().await {
                Ok(requests) => {
                    if let Some(found) = requests.into_iter().rev().find(|r| r.name == request) {
                        break found.id;
                    }
                }
                Err(error) => {
                    return Message::Refused {
                        reason: error.to_string(),
                    };
                }
            }
            if Instant::now() >= deadline {
                return Message::Refused {
                    reason: caskade_core::tr!("the pairing request didn't arrive"),
                };
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        };
        match self.host.accept_pairing(&id, &code, &name).await {
            Ok(()) => {
                tracing::info!(device = name, "paired for streaming through sync");
                Message::Accepted
            }
            Err(error) => Message::Refused {
                reason: error.to_string(),
            },
        }
    }
}

#[async_trait]
impl caskade_sync::Sessions for HostHandover {
    async fn opened(&self, mut session: DeviceSession) {
        let answer = match tokio::time::timeout(
            self.timing.answer,
            session.reader().read::<Message>(MAX_LINE),
        )
        .await
        {
            Ok(Ok(message)) => self.answer(message).await,
            _ => return session.end().await,
        };
        let _ = session.writer().write(&answer).await;
        session.end().await;
    }
}

/// The client's side: hands a pairing's code to its paired devices.
#[derive(Clone)]
pub struct LinkedPairing {
    devices: Devices,
    timing: LinkTiming,
}

impl LinkedPairing {
    pub fn new(devices: Devices) -> Self {
        Self::with(devices, LinkTiming::default())
    }

    pub fn with(devices: Devices, timing: LinkTiming) -> Self {
        Self { devices, timing }
    }

    /// This device's name, as pairing requests carry it.
    pub async fn device_name(&self) -> String {
        super::pairing::device_name(&self.devices.status().await.this_device.name)
    }

    /// Offers `code` for the request named `request` to every paired
    /// device at once; `true` when the one hosting `host` entered it.
    pub async fn hand_over(&self, host: &str, request: &str, code: &str, name: &str) -> bool {
        let message = Message::Pair {
            host: host.to_owned(),
            request: request.to_owned(),
            code: code.to_owned(),
            name: name.to_owned(),
        };
        let devices = self.devices.status().await.devices;
        let mut asking = tokio::task::JoinSet::new();
        for device in devices {
            let (devices, message, wait) =
                (self.devices.clone(), message.clone(), self.timing.answer);
            asking.spawn(async move {
                tokio::time::timeout(wait, ask(&devices, &device.id, &message))
                    .await
                    .unwrap_or(None)
            });
        }
        while let Some(answer) = asking.join_next().await {
            match answer {
                Ok(Some(Message::Accepted)) => return true,
                Ok(Some(Message::Refused { reason })) => {
                    tracing::warn!(%reason, "the host didn't take the code");
                }
                _ => {}
            }
        }
        false
    }
}

async fn ask(devices: &Devices, device: &str, message: &Message) -> Option<Message> {
    let mut session = devices
        .connect(device, ConnectionPurpose::Stream)
        .await
        .ok()?;
    session.writer().write(message).await.ok()?;
    let answer = session.reader().read::<Message>(MAX_LINE).await.ok();
    session.end().await;
    answer
}

#[cfg(test)]
mod tests;
