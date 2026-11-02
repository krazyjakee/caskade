//! The opening: what a connection is about, and whether it's welcome.
//!
//! The dialler opens one bidirectional stream and writes one line: the
//! [`Purpose`] and the grant it holds, if any. The accepting device asks its
//! [`Admission`](crate::Admission) policy once, gets a bool, and writes one
//! line back: [`ACCEPTED`] or [`REFUSAL`]. Nothing else.
//!
//! There's no key exchange, signature or challenge here: TLS 1.3 on the
//! endpoint keys already encrypted the connection and proved who both sides
//! are (`Connection::remote_id`).
//!
//! ## One refusal
//!
//! Every refusal is [`REFUSAL`], byte for byte, followed by the same QUIC
//! close code and reason: an unknown device, a wrong or missing grant, a used
//! or expired link, a forgotten device, a purpose this device doesn't take, a
//! malformed opening and a dialler too slow to write one all look the same.
//! A peer learns that it was refused and nothing about what this device holds.
//!
//! After an accepted opening the stream stays open for the purpose's own
//! messages (pairing uses it); every message on it is a line read with a
//! bound, like the opening.

use std::future::Future;
use std::time::Duration;

use iroh::endpoint::Connection;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::frame::{FrameReader, FrameWriter};
use crate::grant::Grant;

/// The longest opening line this build reads. An offer is a purpose and a
/// 64-character grant, well under this.
pub(crate) const MAX_OPENING_BYTES: usize = 256;

/// The answer to an admitted opening, as written on the wire.
pub const ACCEPTED: &[u8] = b"{\"answer\":\"accepted\"}\n";

/// The answer to every refused opening, whatever the reason.
pub const REFUSAL: &[u8] = b"{\"answer\":\"refused\"}\n";

/// What a connection is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// Library sync between paired devices.
    Sync,
    /// Copying a game between paired devices.
    Copy,
    /// A one-off share with someone else.
    Share,
    /// Pairing: by matching codes (no grant) or by a pairing link (its grant).
    Pair,
    /// A paired device saying it forgot this one.
    Forget,
}

#[derive(Debug, Serialize, Deserialize)]
struct Offer {
    purpose: Purpose,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grant: Option<Grant>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Answer {
    Accepted,
    Refused,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnswerLine {
    answer: Answer,
}

/// The dialler's half. `Ok` with the stream's two halves when admitted.
pub(crate) async fn offer(
    connection: &Connection,
    purpose: Purpose,
    grant: Option<&Grant>,
) -> Result<(FrameWriter, FrameReader)> {
    let (send, recv) = connection.open_bi().await.map_err(Error::interrupted)?;
    let (mut writer, mut reader) = (FrameWriter::new(send), FrameReader::new(recv));
    writer
        .write(&Offer {
            purpose,
            grant: grant.copied(),
        })
        .await?;
    let line: AnswerLine = reader.read(MAX_OPENING_BYTES).await?;
    match line.answer {
        Answer::Accepted => Ok((writer, reader)),
        Answer::Refused => Err(Error::Refused),
    }
}

/// What the accepting side read and admitted.
pub(crate) struct Received {
    pub purpose: Purpose,
    pub grant: Option<Grant>,
    pub writer: FrameWriter,
    pub reader: FrameReader,
}

/// The accepting half: read the offer within `limit`, ask `admits` once, and
/// answer. `None` when refused, for whatever reason; by then [`REFUSAL`] has
/// been written on any stream the dialler opened.
pub(crate) async fn answer(
    connection: &Connection,
    limit: Duration,
    admits: impl FnOnce(Purpose, Option<&Grant>) -> bool,
) -> Option<Received> {
    let (send, recv) = within(limit, async {
        connection.accept_bi().await.map_err(Error::interrupted)
    })
    .await
    .ok()?;
    let (mut writer, mut reader) = (FrameWriter::new(send), FrameReader::new(recv));
    let offer = within(limit, reader.read::<Offer>(MAX_OPENING_BYTES)).await;
    // Asked once; everything after this line sees only the bool, so every
    // reason to refuse takes the same path.
    let admitted = match &offer {
        Ok(offer) => admits(offer.purpose, offer.grant.as_ref()),
        Err(_) => false,
    };
    match offer {
        Ok(offer) if admitted => {
            within(limit, writer.write_raw(ACCEPTED)).await.ok()?;
            Some(Received {
                purpose: offer.purpose,
                grant: offer.grant,
                writer,
                reader,
            })
        }
        _ => {
            refuse(writer).await;
            None
        }
    }
}

/// Writes [`REFUSAL`] and waits (briefly) for the peer to have read it, so the
/// close that follows can't swallow it. Failures are ignored: the connection is
/// closed either way.
pub(crate) async fn refuse(mut writer: FrameWriter) {
    let _ = tokio::time::timeout(REFUSAL_FLUSH, async {
        writer.write_raw(REFUSAL).await?;
        writer.finish().await
    })
    .await;
}

/// How long a refusal waits to be read before the connection closes anyway.
const REFUSAL_FLUSH: Duration = Duration::from_secs(2);

/// Bounds one half of the exchange: a peer that connects and says nothing
/// mustn't hold a task open, and one that accepts and stalls mustn't hang the
/// dialler.
pub(crate) async fn within<T>(
    limit: Duration,
    exchange: impl Future<Output = Result<T>>,
) -> Result<T> {
    match tokio::time::timeout(limit, exchange).await {
        Ok(result) => result,
        Err(_) => Err(Error::TimedOut { within: limit }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answers_are_the_serialized_forms() {
        let mut accepted = serde_json::to_vec(&AnswerLine {
            answer: Answer::Accepted,
        })
        .unwrap();
        accepted.push(b'\n');
        assert_eq!(accepted, ACCEPTED);
        let mut refused = serde_json::to_vec(&AnswerLine {
            answer: Answer::Refused,
        })
        .unwrap();
        refused.push(b'\n');
        assert_eq!(refused, REFUSAL);
    }

    #[test]
    fn an_offer_fits_the_bound_with_a_grant() {
        let offer = serde_json::to_vec(&Offer {
            purpose: Purpose::Forget,
            grant: Some(Grant::generate()),
        })
        .unwrap();
        assert!(offer.len() < MAX_OPENING_BYTES, "{}", offer.len());
    }

    #[test]
    fn an_offer_without_a_grant_leaves_the_field_out() {
        let offer = serde_json::to_string(&Offer {
            purpose: Purpose::Pair,
            grant: None,
        })
        .unwrap();
        assert_eq!(offer, r#"{"purpose":"pair"}"#);
    }
}
