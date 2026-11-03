//! Files that travel beside the change log (#239; artwork files in #273).
//!
//! The change log carries small records; the files they name (save files,
//! imported artwork) travel as blobs on the same granted `sync` session,
//! after both sides' `Ack`. iroh-blobs' own protocol isn't used because it
//! serves any blob to any endpoint that knows its hash, with no grant.
//!
//! Each kind of blob ([`BlobKind`]) says which blobs it lacks, serves the
//! ones it has, and checks each one it receives against its hash before
//! keeping it. Hashes are opaque strings to this module, prefixed by their
//! kind's scheme (`sha256:…` for saves). Every frame names its kind.
//!
//! An exchange runs in rounds, both directions at once:
//!
//! ```text
//!   Want { wants }                 ◀─▶   Want { wants }
//!   Blob { kind, hash, offset, … } ◀─▶   Blob …            the other's wants it has
//!   BlobsDone                      ◀─▶   BlobsDone
//! ```
//!
//! A blob can name more blobs (a save version's file list), so a round that
//! moved anything is followed by another, up to [`MAX_ROUNDS`]. Both sides
//! stop together: what one sent is what the other received, so both count
//! the same total. A kind whose category is This device only here neither
//! asks nor serves.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use super::engine::SyncEngine;
use super::policy::Category;
use crate::{Error, Result};

/// Raw bytes per frame; base64 makes a frame about a third larger.
pub const CHUNK: usize = 512 * 1024;
/// The longest blob frame this build reads.
const MAX_LINE: usize = 2 * 1024 * 1024;
/// Rounds per exchange: a file list, then its files, with room to spare.
pub const MAX_ROUNDS: usize = 4;
/// Blobs asked for per round.
const MAX_WANTS: usize = 10_000;

/// One kind of blob: save files, artwork files.
#[async_trait]
pub trait BlobKind: Send + Sync {
    /// Its name in frames: `save`, `artwork`.
    fn name(&self) -> &'static str;
    /// The category whose choice governs it.
    fn category(&self) -> Category;
    /// Blobs this device's records name and doesn't have.
    async fn wanted(&self) -> Result<Vec<String>>;
    /// Whether this device has `hash` to serve.
    async fn has(&self, hash: &str) -> bool;
    /// Up to `len` bytes of `hash` from `offset`; fewer only at its end.
    async fn read(&self, hash: &str, offset: u64, len: usize) -> Result<Vec<u8>>;
    /// Starts receiving `hash`.
    async fn receive(&self, hash: &str) -> Result<Box<dyn BlobWriter>>;
    /// Governed by a setting of its own (backups kept on a device, #243)
    /// rather than by its category.
    fn own_setting(&self) -> bool {
        false
    }
    /// A note for the device on the other end, sent with the first round's
    /// wants: what this device offers it, say.
    async fn announce(&self, _peer: &str) -> Option<serde_json::Value> {
        None
    }
    /// The note the device on the other end sent for this kind; what it
    /// adds to [`Self::wanted`] is asked for in the next round.
    async fn heard(&self, _peer: &str, _note: serde_json::Value) {}
}

/// A blob being received, chunk by chunk in order.
#[async_trait]
pub trait BlobWriter: Send {
    async fn write(&mut self, chunk: &[u8]) -> Result<()>;
    /// Checks the content against its hash and keeps it: `true`, or throws
    /// it away: `false`.
    async fn finish(self: Box<Self>) -> Result<bool>;
}

/// The kinds this device exchanges. Cheap to clone.
#[derive(Clone, Default)]
pub struct Registry {
    kinds: Arc<RwLock<Vec<Arc<dyn BlobKind>>>>,
}

impl Registry {
    pub fn register(&self, kind: Arc<dyn BlobKind>) {
        let mut kinds = self.kinds.write().unwrap_or_else(|e| e.into_inner());
        kinds.retain(|known| known.name() != kind.name());
        kinds.push(kind);
    }

    fn kinds(&self) -> Vec<Arc<dyn BlobKind>> {
        self.kinds.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Want {
    kind: String,
    hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message {
    Want {
        wants: Vec<Want>,
        /// Each kind's note, in the first round only; an older build
        /// ignores it.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        notes: BTreeMap<String, serde_json::Value>,
    },
    Blob {
        kind: String,
        hash: String,
        offset: u64,
        data: String,
        last: bool,
    },
    BlobsDone,
}

fn interrupted(error: caskade_sync::Error) -> Error {
    tracing::debug!(%error, "blob exchange interrupted");
    Error::Invalid(crate::tr!("the connection to the other device failed"))
}

/// How a blob exchange went.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlobsExchanged {
    pub sent: usize,
    pub received: usize,
}

/// Exchanges blobs with `peer`, the device on the other end, after the
/// changes.
pub async fn exchange(
    engine: &SyncEngine,
    peer: &str,
    writer: &mut caskade_sync::FrameWriter,
    reader: &mut caskade_sync::FrameReader,
) -> Result<BlobsExchanged> {
    let categories = engine.categories().await;
    let kinds: HashMap<String, Arc<dyn BlobKind>> = engine
        .blobs()
        .kinds()
        .into_iter()
        .filter(|kind| kind.own_setting() || categories.allows(kind.category()))
        .map(|kind| (kind.name().to_owned(), kind))
        .collect();
    let mut total = BlobsExchanged::default();
    for round in 0..MAX_ROUNDS {
        let mut notes = BTreeMap::new();
        if round == 0 {
            for kind in kinds.values() {
                if let Some(note) = kind.announce(peer).await {
                    notes.insert(kind.name().to_owned(), note);
                }
            }
        }
        let mut mine = Vec::new();
        for kind in kinds.values() {
            match kind.wanted().await {
                Ok(hashes) => mine.extend(hashes.into_iter().map(|hash| Want {
                    kind: kind.name().to_owned(),
                    hash,
                })),
                Err(error) => tracing::warn!(%error, kind = kind.name(), "wanted blobs unknown"),
            }
        }
        mine.sort_by(|a, b| (&a.kind, &a.hash).cmp(&(&b.kind, &b.hash)));
        mine.dedup();
        mine.truncate(MAX_WANTS);
        writer
            .write(&Message::Want {
                wants: mine.clone(),
                notes: notes.clone(),
            })
            .await
            .map_err(interrupted)?;
        let Message::Want {
            wants: theirs,
            notes: their_notes,
        } = reader.read(MAX_LINE).await.map_err(interrupted)?
        else {
            return Err(Error::Invalid(crate::tr!(
                "the other device sent something out of turn"
            )));
        };
        // What the other device noted may add to this device's wants: they're
        // asked for in the next round, which both sides then run.
        for (name, note) in &their_notes {
            if let Some(kind) = kinds.get(name) {
                kind.heard(peer, note.clone()).await;
            }
        }
        let noted = !notes.is_empty() || !their_notes.is_empty();
        let send = async {
            let mut sent = 0;
            for want in theirs.iter().take(MAX_WANTS) {
                let Some(kind) = kinds.get(&want.kind) else {
                    continue;
                };
                if !kind.has(&want.hash).await {
                    continue;
                }
                let mut offset = 0u64;
                loop {
                    let chunk = kind.read(&want.hash, offset, CHUNK).await?;
                    let last = chunk.len() < CHUNK;
                    writer
                        .write(&Message::Blob {
                            kind: want.kind.clone(),
                            hash: want.hash.clone(),
                            offset,
                            data: base64::engine::general_purpose::STANDARD.encode(&chunk),
                            last,
                        })
                        .await
                        .map_err(interrupted)?;
                    offset += chunk.len() as u64;
                    if last {
                        break;
                    }
                }
                sent += 1;
            }
            writer
                .write(&Message::BlobsDone)
                .await
                .map_err(interrupted)?;
            Ok::<_, Error>(sent)
        };
        let receive = async {
            // Every blob whose last frame came, kept or not, and those kept.
            let (mut arrived, mut kept) = (0, 0);
            let mut current: Option<(String, String, u64, Box<dyn BlobWriter>)> = None;
            loop {
                match reader
                    .read::<Message>(MAX_LINE)
                    .await
                    .map_err(interrupted)?
                {
                    Message::Blob {
                        kind,
                        hash,
                        offset,
                        data,
                        last,
                    } => {
                        if last {
                            arrived += 1;
                        }
                        let same =
                            matches!(&current, Some((k, h, _, _)) if *k == kind && *h == hash);
                        if !same {
                            current = None;
                            let asked = mine
                                .iter()
                                .any(|want| want.kind == kind && want.hash == hash);
                            if asked
                                && offset == 0
                                && let Some(blob_kind) = kinds.get(&kind)
                            {
                                match blob_kind.receive(&hash).await {
                                    Ok(writer) => current = Some((kind, hash, 0, writer)),
                                    Err(error) => {
                                        tracing::warn!(%error, "a blob couldn't be received")
                                    }
                                }
                            }
                        }
                        // A blob nobody asked for, or out of order, is skipped.
                        let Some((_, _, at, blob)) = current.as_mut() else {
                            continue;
                        };
                        let bytes = match base64::engine::general_purpose::STANDARD.decode(&data) {
                            Ok(bytes) if *at == offset => bytes,
                            _ => {
                                current = None;
                                continue;
                            }
                        };
                        blob.write(&bytes).await?;
                        *at += bytes.len() as u64;
                        if last && let Some((_, hash, _, blob)) = current.take() {
                            if blob.finish().await? {
                                kept += 1;
                            } else {
                                tracing::warn!(%hash, "a blob didn't match its hash");
                            }
                        }
                    }
                    Message::BlobsDone => break,
                    Message::Want { .. } => {
                        return Err(Error::Invalid(crate::tr!(
                            "the other device sent something out of turn"
                        )));
                    }
                }
            }
            Ok::<_, Error>((arrived, kept))
        };
        let (sent, received) = tokio::join!(send, receive);
        let (sent, (arrived, kept)) = (sent?, received?);
        total.sent += sent;
        total.received += kept;
        // The same on both sides: what one sent is what arrived at the other,
        // and both saw the same notes.
        if sent + arrived == 0 && !noted {
            break;
        }
    }
    Ok(total)
}
