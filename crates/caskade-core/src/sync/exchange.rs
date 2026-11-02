//! The exchange on `caskade/sync/1` (#230): each side sends its version
//! vector, then the changes the other hasn't seen, in bounded pages, in both
//! directions at once.
//!
//! ```text
//!   Hello { seen }                 ◀─▶   Hello { seen }
//!   Page { changes } …             ◀─▶   Page { changes } …
//!   Done                           ◀─▶   Done
//! ```
//!
//! Each received page is imported and applied before the next is read, one
//! transaction per change, so a connection that drops mid-exchange leaves
//! both libraries consistent: every change is applied entirely or not at all,
//! and the version vector says exactly which arrived. The next exchange starts
//! from there.
//!
//! A category that is This device only here isn't sent; one turned back on is
//! sent in full once (the device takes what it lacks and ignores the rest).
//!
//! After both `Ack`s, the files the changes name follow (`sync::blobs`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::change::{Change, Entity};
use super::engine::SyncEngine;
use super::policy::Category;
use crate::db::Seen;
use crate::{Error, Result};

/// Changes per page.
pub const PAGE: usize = 200;

/// The longest line this build reads: a page of [`PAGE`] changes with room
/// to spare. Longer is a malformed peer.
const MAX_LINE: usize = 8 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message {
    Hello {
        seen: Seen,
    },
    Page {
        changes: Vec<Change>,
    },
    Done,
    /// Everything received has applied: what this side has now.
    Ack {
        seen: Seen,
    },
}

/// How an exchange went.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exchanged {
    pub sent: usize,
    pub received: usize,
}

/// Options for tests.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Stop sending after this many pages, as a connection that drops does.
    pub fail_after_pages: Option<usize>,
}

fn interrupted(error: caskade_sync::Error) -> Error {
    tracing::debug!(%error, "sync exchange interrupted");
    Error::Invalid(crate::tr!("the connection to the other device failed"))
}

/// Runs one exchange with `peer` over `session` (either side).
pub async fn exchange(
    engine: &SyncEngine,
    peer: &str,
    session: &mut caskade_sync::Session,
    options: &Options,
) -> Result<Exchanged> {
    engine.capture().await?;
    let seen = engine.seen().await?;
    let (writer, reader) = session.split();
    writer
        .write(&Message::Hello { seen })
        .await
        .map_err(interrupted)?;
    let Message::Hello { seen: theirs } = reader.read(MAX_LINE).await.map_err(interrupted)? else {
        return Err(Error::Invalid(
            "the other device didn't say what it has".into(),
        ));
    };
    engine.store().set_sync_peer_seen(peer, &theirs).await?;

    let send = async {
        let mut sent = 0;
        let mut pages = 0;
        // Categories turned back on here go again in full.
        let replay = engine.store().sync_peer_replay(peer).await?;
        let held_back = engine.held_back().await;
        let replay: Vec<Entity> = Entity::ALL
            .into_iter()
            .filter(|entity| {
                replay.iter().any(|name| {
                    serde_json::to_value(entity.category())
                        .ok()
                        .and_then(|value| value.as_str().map(|v| v == name))
                        .unwrap_or(false)
                }) && !held_back.contains(entity)
            })
            .collect();
        let mut after: Option<(String, crate::sync::clock::Hlc)> = None;
        if !replay.is_empty() {
            loop {
                let changes = engine
                    .store()
                    .sync_changes_of(
                        &replay,
                        after
                            .as_ref()
                            .map(|(origin, clock)| (origin.as_str(), *clock)),
                        PAGE,
                    )
                    .await?;
                let Some(last) = changes.last() else { break };
                after = Some((last.origin.clone(), last.clock));
                if options.fail_after_pages.is_some_and(|limit| pages >= limit) {
                    return Err(Error::Invalid("stopped sending".into()));
                }
                sent += changes.len();
                pages += 1;
                writer
                    .write(&Message::Page { changes })
                    .await
                    .map_err(interrupted)?;
            }
        }
        let mut cursor = theirs.clone();
        loop {
            let changes = engine.changes_for(&cursor, PAGE).await?;
            if changes.is_empty() {
                break;
            }
            for change in &changes {
                let at = cursor.entry(change.origin.clone()).or_default();
                if change.clock > *at {
                    *at = change.clock;
                }
            }
            if options.fail_after_pages.is_some_and(|limit| pages >= limit) {
                return Err(Error::Invalid("stopped sending".into()));
            }
            sent += changes.len();
            pages += 1;
            writer
                .write(&Message::Page { changes })
                .await
                .map_err(interrupted)?;
        }
        writer.write(&Message::Done).await.map_err(interrupted)?;
        Ok::<_, Error>((sent, cursor))
    };
    let receive = async {
        let mut received = 0;
        loop {
            match reader
                .read::<Message>(MAX_LINE)
                .await
                .map_err(interrupted)?
            {
                Message::Page { changes } => {
                    received += engine.receive(&changes).await?;
                }
                Message::Done => break,
                Message::Hello { .. } | Message::Ack { .. } => {
                    return Err(Error::Invalid(crate::tr!(
                        "the other device sent something out of turn"
                    )));
                }
            }
        }
        Ok::<_, Error>(received)
    };
    let (sent, received) = tokio::join!(send, receive);
    let (sent, _cursor) = sent?;
    let received = received?;
    // Both sides have applied what they received: each says what it has now,
    // which is what compaction may count on, and the exchange ends only when
    // the other side is done too.
    let seen = engine.seen().await?;
    writer
        .write(&Message::Ack { seen })
        .await
        .map_err(interrupted)?;
    let Message::Ack { seen: theirs } = reader.read(MAX_LINE).await.map_err(interrupted)? else {
        return Err(Error::Invalid(crate::tr!("the other device didn't finish")));
    };
    engine.store().set_sync_peer_seen(peer, &theirs).await?;
    engine.store().set_sync_peer_replay(peer, &[]).await?;
    // Then the files the changes name (#239), and what was waiting for them
    // (artwork, #273).
    let blobs = super::blobs::exchange(engine, writer, reader).await?;
    if blobs.received > 0 {
        engine.apply_pending().await?;
    }
    // Last, what's running on each device (#240).
    super::presence::exchange(engine, peer, writer, reader).await?;
    Ok(Exchanged { sent, received })
}

/// Marks `categories` to send to every paired device again in full.
pub async fn replay(engine: &SyncEngine, categories: &[Category]) -> Result<()> {
    if categories.is_empty() {
        return Ok(());
    }
    let names: HashSet<String> = categories
        .iter()
        .filter_map(|category| serde_json::to_value(category).ok())
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect();
    for device in engine.store().paired_devices().await? {
        if device.forgotten_at_ms.is_some() {
            continue;
        }
        let mut replay: HashSet<String> = engine
            .store()
            .sync_peer_replay(&device.endpoint_id)
            .await?
            .into_iter()
            .collect();
        replay.extend(names.iter().cloned());
        let replay: Vec<String> = replay.into_iter().collect();
        engine
            .store()
            .set_sync_peer_replay(&device.endpoint_id, &replay)
            .await?;
    }
    Ok(())
}
