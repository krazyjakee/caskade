//! Pairing: by matching codes on the local network, or with a pairing link
//! anywhere.
//!
//! Both run on a [`Session`] opened for [`Purpose::Pair`]; the core decides
//! who may open one (see [`crate::Admission`]) and stores the result.
//!
//! ## By code
//!
//! Both devices show the same 6-digit code, derived from both endpoint ids
//! and a fresh 32-byte nonce from each side, and the pairing completes only
//! when both users confirm. The dialler commits to its nonce before it sees
//! the other's, so nobody in the middle can pick nonces until the two codes
//! match: a device relaying between two others shows each a different code,
//! except by a one-in-a-million chance per attempt.
//!
//! ```text
//! dialler                                   acceptor
//!   Commit { sha256(nonce_d), device } ──▶
//!                                       ◀── Nonce { nonce_a, device }
//!   Reveal { nonce_d }                 ──▶   (checks the commitment)
//!   both show code(id_d, id_a, nonce_d, nonce_a)
//!   Confirm { grant } or Cancel        ◀─▶   Confirm { grant } or Cancel
//! ```
//!
//! A cancel on either side, a mismatch, a timeout or a dropped connection
//! ends it with nothing to store.
//!
//! ## By link
//!
//! The link's single-use grant was already checked and spent in the opening,
//! so the two devices only introduce themselves:
//!
//! ```text
//!   Hello { device, grant }   ──▶
//!                             ◀── Welcome { device, grant }
//! ```
//!
//! ## Grants
//!
//! Each side generates the grant the *other* will present when it dials this
//! one later ([`Paired::grant_in`]), and receives the one it will present
//! ([`Paired::grant_out`]).

use std::time::Duration;

use iroh::{EndpointAddr, EndpointId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::device::DeviceInfo;
use crate::endpoint::{Session, SyncEndpoint};
use crate::error::{Error, Result};
use crate::grant::{Grant, hex};
use crate::link_ticket::PairingLink;
use crate::opening::{Purpose, within};

/// The longest pairing message this build reads.
const MAX_MESSAGE_BYTES: usize = 1024;

/// How long a step of the exchange may take, apart from the users' decision.
const STEP_TIMEOUT: Duration = Duration::from_secs(15);

/// How long the users get to compare codes and confirm.
pub const DECISION_TIMEOUT: Duration = Duration::from_secs(120);

/// A paired device, as one side of a completed pairing sees it.
#[derive(Debug, Clone)]
pub struct Paired {
    pub peer: EndpointId,
    pub info: DeviceInfo,
    /// Where it was reached.
    pub addr: EndpointAddr,
    /// The grant the peer presents when it dials this device.
    pub grant_in: Grant,
    /// The grant this device presents when it dials the peer.
    pub grant_out: Grant,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message {
    Commit {
        commitment: String,
        device: DeviceInfo,
    },
    Nonce {
        nonce: String,
        device: DeviceInfo,
    },
    Reveal {
        nonce: String,
    },
    Confirm {
        grant: Grant,
    },
    Cancel,
    Hello {
        device: DeviceInfo,
        grant: Grant,
    },
    Welcome {
        device: DeviceInfo,
        grant: Grant,
    },
}

/// A code pairing whose codes are on screen, waiting for both users.
#[derive(Debug)]
pub struct CodePairing {
    session: Session,
    peer_info: DeviceInfo,
    code: String,
}

impl CodePairing {
    /// Runs the commit/nonce/reveal exchange on a session opened for
    /// pairing without a grant. Either side calls it; the session knows which
    /// one it is.
    pub async fn begin(mut session: Session, me: &DeviceInfo) -> Result<Self> {
        let own_nonce = random_nonce();
        let dialler = session.is_dialler();
        let (peer_info, peer_nonce) = if dialler {
            within(STEP_TIMEOUT, async {
                session
                    .writer()
                    .write(&Message::Commit {
                        commitment: hex(&commitment(&own_nonce)),
                        device: me.clone(),
                    })
                    .await?;
                let Message::Nonce { nonce, device } = read(&mut session).await? else {
                    return Err(Error::Malformed);
                };
                session
                    .writer()
                    .write(&Message::Reveal {
                        nonce: hex(&own_nonce),
                    })
                    .await?;
                Ok((device, parse_nonce(&nonce)?))
            })
            .await?
        } else {
            within(STEP_TIMEOUT, async {
                let Message::Commit {
                    commitment: commit,
                    device,
                } = read(&mut session).await?
                else {
                    return Err(Error::Malformed);
                };
                session
                    .writer()
                    .write(&Message::Nonce {
                        nonce: hex(&own_nonce),
                        device: me.clone(),
                    })
                    .await?;
                let Message::Reveal { nonce } = read(&mut session).await? else {
                    return Err(Error::Malformed);
                };
                let nonce = parse_nonce(&nonce)?;
                if hex(&commitment(&nonce)) != commit {
                    return Err(Error::Malformed);
                }
                Ok((device, nonce))
            })
            .await?
        };
        let peer = session.peer();
        let own_id = session.local_id();
        let code = if dialler {
            code(own_id, peer, &own_nonce, &peer_nonce)
        } else {
            code(peer, own_id, &peer_nonce, &own_nonce)
        };
        Ok(Self {
            session,
            peer_info: clean(peer_info),
            code,
        })
    }

    /// The 6-digit code both devices show.
    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn peer(&self) -> EndpointId {
        self.session.peer()
    }

    pub fn peer_info(&self) -> &DeviceInfo {
        &self.peer_info
    }

    /// Waits for this user's decision and the other's. `decision` resolves to
    /// whether this user confirmed. Completes only when both confirmed;
    /// [`Error::Cancelled`] when either cancelled, and
    /// [`Error::TimedOut`] when nobody decided within
    /// [`DECISION_TIMEOUT`].
    pub async fn finish(
        self,
        decision: impl Future<Output = bool>,
        grant_in: Grant,
    ) -> Result<Paired> {
        let Self {
            mut session,
            peer_info,
            ..
        } = self;
        let result = within(DECISION_TIMEOUT, async {
            let mut decision = std::pin::pin!(decision);
            let mut peer_grant: Option<Grant> = None;
            let mut decided = false;
            loop {
                if decided && peer_grant.is_some() {
                    break;
                }
                let (writer, reader) = session.split();
                tokio::select! {
                    confirmed = &mut decision, if !decided => {
                        decided = true;
                        if !confirmed {
                            let _ = writer.write(&Message::Cancel).await;
                            return Err(Error::Cancelled);
                        }
                        writer.write(&Message::Confirm { grant: grant_in }).await?;
                    }
                    message = reader.read::<Message>(MAX_MESSAGE_BYTES), if peer_grant.is_none() => {
                        match message? {
                            Message::Confirm { grant } => peer_grant = Some(grant),
                            Message::Cancel => return Err(Error::Cancelled),
                            _ => return Err(Error::Malformed),
                        }
                    }
                }
            }
            Ok(peer_grant.expect("the loop ends once the peer confirmed"))
        })
        .await;
        match result {
            Ok(grant_out) => {
                let paired = Paired {
                    peer: session.peer(),
                    info: peer_info,
                    addr: session.addr(),
                    grant_in,
                    grant_out,
                };
                session.end().await;
                Ok(paired)
            }
            Err(error) => {
                session.end().await;
                Err(error)
            }
        }
    }

    /// Ends the pairing on this side without waiting for anyone.
    pub async fn cancel(mut self) {
        let _ = within(STEP_TIMEOUT, self.session.writer().write(&Message::Cancel)).await;
        self.session.end().await;
    }
}

/// Dials the device a pairing link names and pairs with it.
///
/// [`Error::LinkExpired`] without dialling when the link says it expired;
/// [`Error::LinkRefused`] when the device refused it (used, expired, or no
/// longer offered: the refusal doesn't say which).
pub async fn pair_with_link(
    endpoint: &SyncEndpoint,
    link: &PairingLink,
    me: &DeviceInfo,
    grant_in: Grant,
) -> Result<Paired> {
    if link.expired(std::time::SystemTime::now()) {
        return Err(Error::LinkExpired);
    }
    let mut session = match endpoint
        .connect(link.addr().clone(), Purpose::Pair, Some(link.grant()))
        .await
    {
        Ok(session) => session,
        Err(Error::Refused) => return Err(Error::LinkRefused),
        Err(error) => return Err(error),
    };
    let result = within(STEP_TIMEOUT, async {
        session
            .writer()
            .write(&Message::Hello {
                device: me.clone(),
                grant: grant_in,
            })
            .await?;
        match read(&mut session).await? {
            Message::Welcome { device, grant } => Ok((device, grant)),
            Message::Cancel => Err(Error::Cancelled),
            _ => Err(Error::Malformed),
        }
    })
    .await;
    finish_link(session, result, grant_in).await
}

/// The accepting side of a link pairing, on a session whose pairing grant
/// the core's admission already spent.
pub async fn accept_link(mut session: Session, me: &DeviceInfo, grant_in: Grant) -> Result<Paired> {
    let result = within(STEP_TIMEOUT, async {
        let Message::Hello { device, grant } = read(&mut session).await? else {
            return Err(Error::Malformed);
        };
        session
            .writer()
            .write(&Message::Welcome {
                device: me.clone(),
                grant: grant_in,
            })
            .await?;
        Ok((device, grant))
    })
    .await;
    finish_link(session, result, grant_in).await
}

async fn finish_link(
    session: Session,
    result: Result<(DeviceInfo, Grant)>,
    grant_in: Grant,
) -> Result<Paired> {
    let result = result.map(|(info, grant_out)| Paired {
        peer: session.peer(),
        info: clean(info),
        addr: session.addr(),
        grant_in,
        grant_out,
    });
    // The last message reaches the peer before the connection closes.
    session.end().await;
    result
}

async fn read(session: &mut Session) -> Result<Message> {
    session.reader().read(MAX_MESSAGE_BYTES).await
}

fn clean(info: DeviceInfo) -> DeviceInfo {
    DeviceInfo {
        name: crate::device::clean_name(&info.name),
        ..info
    }
}

fn random_nonce() -> [u8; 32] {
    *Grant::generate().as_bytes()
}

fn parse_nonce(text: &str) -> Result<[u8; 32]> {
    let grant: Grant = text.parse()?;
    Ok(*grant.as_bytes())
}

fn commitment(nonce: &[u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"caskade-pair-commit/1");
    hash.update(nonce);
    hash.finalize().into()
}

/// The 6-digit code for a pairing between `dialler` and `acceptor`.
pub fn code(
    dialler: EndpointId,
    acceptor: EndpointId,
    dialler_nonce: &[u8; 32],
    acceptor_nonce: &[u8; 32],
) -> String {
    let mut hash = Sha256::new();
    hash.update(b"caskade-pair-code/1");
    hash.update(dialler.as_bytes());
    hash.update(acceptor.as_bytes());
    hash.update(dialler_nonce);
    hash.update(acceptor_nonce);
    let digest: [u8; 32] = hash.finalize().into();
    let mut head = [0u8; 8];
    head.copy_from_slice(&digest[..8]);
    format!("{:06}", u64::from_be_bytes(head) % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_depends_on_both_ids_and_both_nonces() {
        let a = iroh::SecretKey::generate().public();
        let b = iroh::SecretKey::generate().public();
        let c = iroh::SecretKey::generate().public();
        let (na, nb) = ([1u8; 32], [2u8; 32]);
        let code_ab = code(a, b, &na, &nb);
        assert_eq!(code_ab.len(), 6);
        assert!(code_ab.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(code(a, b, &na, &nb), code_ab, "deterministic");
        let others = [
            code(b, a, &na, &nb),
            code(a, c, &na, &nb),
            code(c, b, &na, &nb),
            code(a, b, &[3u8; 32], &nb),
            code(a, b, &na, &[3u8; 32]),
        ];
        // Six digits can collide by chance; these fixed inputs don't.
        for other in others {
            assert_ne!(other, code_ab);
        }
    }

    #[test]
    fn codes_spread_over_six_digits() {
        let a = iroh::SecretKey::generate().public();
        let b = iroh::SecretKey::generate().public();
        let codes: std::collections::HashSet<String> = (0..2000)
            .map(|_| code(a, b, &random_nonce(), &random_nonce()))
            .collect();
        assert!(codes.len() > 1990, "{}", codes.len());
    }

    #[test]
    fn a_commitment_binds_the_nonce() {
        assert_ne!(commitment(&[1u8; 32]), commitment(&[2u8; 32]));
        assert_eq!(commitment(&[1u8; 32]), commitment(&[1u8; 32]));
    }
}
