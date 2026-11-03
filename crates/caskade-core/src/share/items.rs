//! Smaller things shared once (#248): a save version, a web game's offline
//! copy, a ROM with the emulator profile it plays with, and a collection as
//! a list of games.
//!
//! They share a game's link, session and transfer (`Shares`). What sets
//! each apart is its [`Details`]: kept in the share's `details_json` and
//! sent in the `About` frame, so a recipient sees what it is before taking
//! it. The files, when there are any, are a folder offered like a game's:
//! the save's files named by their SHA-256, the bundle's folder, or the ROM
//! files. A collection has none: it is only its list.
//!
//! Where each one goes on the recipient ([`Shares::place`]):
//! - a save becomes a new version of the same game's saves (found by its
//!   identity, as sync finds games) that descends from nothing, so it never
//!   replaces a save there without asking ([`SaveVersions::add_shared`]);
//! - a web game's offline copy becomes the offline copy of the web game at
//!   the same address there, in the storage chosen for it;
//! - a ROM becomes a game of its platform in the storage chosen for it,
//!   pinned to the same emulator profile when that emulator is here, and
//!   otherwise emulator setup installs the emulator it played with;
//! - a collection becomes a collection of the recipient's, holding the games
//!   they have, and a title-only record of each game they don't
//!   ([`Shares::receive_collection`]).

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ShareExpiry, ShareRecord, Shares};
use crate::db::{LibraryStore, ShareRow};
use crate::install::Placed;
use crate::metadata::{Applied, IgdbMatch, MatchSource};
use crate::model::{
    Collection, CollectionKind, GameEntry, InstallLocation, InstallState, LaunchSpec,
    SaveCollection,
};
use crate::saves::versions::{ManifestFile, SaveVersions};
pub use crate::transfer::rom::EmulatorInstaller;
use crate::transfer::rom::{self, RomCopy};
use crate::{Error, Result};

/// The most games a shared collection lists.
pub(super) const MAX_GAMES: usize = 1000;

/// Something other than a game to share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ShareItem {
    /// One version of a game's saves (its Saves panel).
    Save { version_id: String },
    /// A web game's offline copy.
    Web { game_id: String },
    /// An emulated release: its ROM files, platform and emulator profile.
    Rom { entry_id: String },
    /// One of the account's collections, as a list of games.
    Collection { collection_id: String },
}

/// What a share other than a game is, in a form another library reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(super) enum Details {
    Save {
        /// The game, as sync names it (`LibraryStore::game_identity`).
        game: Value,
        files: Vec<ManifestFile>,
    },
    Web {
        url: String,
    },
    /// Its files, platform and emulator profile, as copies send them.
    Rom(RomCopy),
    Collection {
        name: String,
        description: String,
        /// Each game's identity.
        games: Vec<Value>,
    },
}

/// What a share is, as the recipient sees it before taking it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ShareContents {
    Game,
    Save {
        /// The game it's a save of, and whether it's in the library here.
        game: String,
        here: Option<String>,
        files: u32,
    },
    Web {
        url: String,
        /// The web game here with that address, when there is one.
        here: Option<String>,
    },
    Rom {
        platform: String,
        /// The emulator it played with (its definition), and whether this
        /// device has one for the platform or that emulator.
        emulator: Option<String>,
        ready: bool,
    },
    Collection {
        name: String,
        games: Vec<SharedGame>,
    },
}

/// A game of a shared collection, and whether the recipient has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SharedGame {
    pub title: String,
    pub igdb_id: Option<String>,
    pub here: GameHere,
    /// The game here, when there is one.
    pub game_id: Option<String>,
}

/// Whether a library has a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum GameHere {
    /// Installed on this device.
    Installed,
    /// A store account owns it.
    Owned,
    /// In the library, neither installed nor owned.
    InLibrary,
    Missing,
}

/// What the items need besides the library.
#[derive(Default)]
pub(super) struct Parts {
    saves: OnceLock<SaveVersions>,
}

impl Details {
    pub(super) fn parse(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }

    /// The details a sharer sent, when they're a known kind and within
    /// bounds.
    pub(super) fn read(value: Option<&Value>) -> Option<Self> {
        let details: Self = serde_json::from_value(value?.clone()).ok()?;
        let fits = match &details {
            Self::Save { files, .. } => {
                !files.is_empty()
                    && files.len() <= 20_000
                    && files.iter().all(|file| blob_name(&file.hash).is_some())
            }
            Self::Web { url } => crate::web::page_url(url).is_ok(),
            Self::Rom(rom) => rom.sound(),
            Self::Collection { games, .. } => games.len() <= MAX_GAMES,
        };
        fits.then_some(details)
    }
}

/// `sha256:<hex>` as the file name its content is shared under.
fn blob_name(hash: &str) -> Option<&str> {
    let hex = hash.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

/// Whether the library has `game`: installed, owned or only listed.
async fn has_game(store: &LibraryStore, game: &str) -> Result<GameHere> {
    let entries = store.entries_for_game(game).await?;
    let elsewhere = |entry: &GameEntry| {
        let source = entry.source_id.as_str();
        source.starts_with("peer:") || source.starts_with("store:") || source.starts_with("stream:")
    };
    Ok(
        if entries
            .iter()
            .any(|entry| matches!(entry.state, InstallState::Installed) && !elsewhere(entry))
        {
            GameHere::Installed
        } else if entries
            .iter()
            .any(|entry| entry.source_id.as_str().starts_with("store:"))
        {
            GameHere::Owned
        } else {
            GameHere::InLibrary
        },
    )
}

/// The game here `identity` names, or a title-only one made for it (with
/// its IGDB id, so its details come as for any game), and whether it was
/// made.
async fn found_or_made(store: &LibraryStore, identity: &Value) -> Result<(String, bool)> {
    if let Some((game, _)) = store.sync_find_game(identity).await? {
        return Ok((game, false));
    }
    let game = store.sync_create_game(identity).await?;
    if let Some(igdb) = identity
        .get("igdb")
        .and_then(Value::as_i64)
        .and_then(|id| u64::try_from(id).ok())
        .filter(|id| *id > 0)
    {
        store
            .save_igdb_match(&IgdbMatch {
                game_id: game.clone(),
                igdb_id: Some(igdb),
                source: MatchSource::Manual,
                pending: true,
                checked_at_ms: crate::now_ms(),
                dumps: String::new(),
                applied: Applied::default(),
            })
            .await?;
    }
    Ok((game, true))
}

fn title_of(identity: &Value) -> String {
    identity
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

impl Shares {
    /// Keeps a save version's files and lets a shared ROM's emulator be
    /// installed. Without them, those kinds can't be shared or received.
    pub fn set_saves(&self, saves: SaveVersions) {
        let _ = self.inner.items.saves.set(saves);
    }

    pub fn set_emulator_installer(&self, installer: Arc<dyn EmulatorInstaller>) {
        self.inner.transfers.set_emulator_installer(installer);
    }

    fn saves(&self) -> Result<&SaveVersions> {
        self.inner
            .items
            .saves
            .get()
            .ok_or_else(|| Error::Invalid(crate::tr!("saves aren't available")))
    }

    /// Where a share's files are put together on this device.
    pub(super) fn outgoing(&self, id: &str) -> PathBuf {
        self.inner.incoming_dir.with_file_name("outgoing").join(id)
    }

    /// Shares `item` until `expiry`, as `account` (whose collections are
    /// theirs alone), and returns the share; its link is
    /// [`Shares::link`]. Its files are read in the background meanwhile.
    pub async fn share_item(
        &self,
        item: ShareItem,
        expiry: ShareExpiry,
        account: &str,
    ) -> Result<ShareRecord> {
        let store = &self.inner.store;
        let (kind, title, game_id, entry_id, size, details) = match item {
            ShareItem::Save { version_id } => {
                let saves = self.saves()?;
                let version = store
                    .save_version(&version_id)
                    .await?
                    .ok_or_else(|| Error::Invalid(crate::tr!("that save isn't here any more")))?;
                let files: Vec<ManifestFile> = saves
                    .files(&version)
                    .ok_or_else(|| {
                        Error::Invalid(crate::tr!(
                            "that save's files haven't all arrived on this device"
                        ))
                    })?
                    .into_iter()
                    .map(|(file, _)| file)
                    .filter(|file| !file.root.is_empty())
                    .collect();
                if files.is_empty() {
                    return Err(Error::Invalid(crate::tr!(
                        "that save's files are only where this device keeps them, so they can't go elsewhere"
                    )));
                }
                let game = store
                    .game_identity(&version.game_id)
                    .await?
                    .ok_or_else(|| crate::db::missing("game", &version.game_id))?;
                let size = files.iter().map(|file| file.size).sum::<u64>();
                (
                    "save",
                    title_of(&game),
                    Some(version.game_id.clone()),
                    None,
                    size,
                    Details::Save { game, files },
                )
            }
            ShareItem::Web { game_id } => {
                let (entry, _) = self.offline_copy(&game_id).await?;
                let Some(LaunchSpec::Web { url }) = &entry.launch else {
                    return Err(Error::Invalid(crate::tr!("that isn't a web game")));
                };
                let title = store
                    .game(&game_id)
                    .await?
                    .map(|game| game.title)
                    .unwrap_or_default();
                (
                    "web",
                    title,
                    Some(game_id),
                    Some(entry.id.clone()),
                    entry.size.unwrap_or(0),
                    Details::Web { url: url.clone() },
                )
            }
            ShareItem::Rom { entry_id } => {
                let (entry, rom, files) = rom::describe(store, &entry_id).await?;
                let title = store
                    .game(&entry.game_id)
                    .await?
                    .map(|game| game.title)
                    .unwrap_or_default();
                let size = files
                    .iter()
                    .filter_map(|(path, _)| std::fs::metadata(path).ok())
                    .map(|metadata| metadata.len())
                    .sum();
                (
                    "rom",
                    title,
                    Some(entry.game_id.clone()),
                    Some(entry_id),
                    size,
                    Details::Rom(rom),
                )
            }
            ShareItem::Collection { collection_id } => {
                let collection = store
                    .get_collection(account, &collection_id)
                    .await?
                    .ok_or_else(|| crate::db::missing("collection", &collection_id))?;
                let ids = match &collection.kind {
                    CollectionKind::Manual => collection.game_ids.clone(),
                    CollectionKind::Smart { query } => {
                        store.game_ids_for_account(query, Some(account)).await?
                    }
                };
                let mut games = Vec::new();
                for id in ids {
                    if games.len() == MAX_GAMES {
                        break;
                    }
                    if store.game(&id).await?.is_some_and(|game| game.hidden) {
                        continue;
                    }
                    if let Some(identity) = store.game_identity(&id).await? {
                        games.push(identity);
                    }
                }
                (
                    "collection",
                    collection.name.clone(),
                    None,
                    None,
                    0,
                    Details::Collection {
                        name: collection.name,
                        description: collection.description,
                        games,
                    },
                )
            }
        };
        let now = crate::now_ms();
        let row = ShareRow {
            id: crate::model::new_id(),
            kind: kind.into(),
            game_id,
            entry_id,
            title,
            size: i64::try_from(size).unwrap_or(i64::MAX),
            root: None,
            extras_root: None,
            grant: caskade_sync::Grant::generate().to_hex(),
            created_at_ms: now,
            expires_at_ms: expiry.duration().map(|until| {
                now.saturating_add(i64::try_from(until.as_millis()).unwrap_or(i64::MAX))
            }),
            stopped_at_ms: None,
            details_json: serde_json::to_string(&details)?,
        };
        store.insert_share(&row).await?;
        self.run(row.clone());
        self.changed();
        self.record(row).await
    }

    /// `game_id`'s web release with its offline copy, and the copy's folder.
    async fn offline_copy(&self, game_id: &str) -> Result<(GameEntry, PathBuf)> {
        let store = &self.inner.store;
        let entry = store
            .entries_for_game(game_id)
            .await?
            .into_iter()
            .find(|entry| entry.source_id == crate::web::WEB_SOURCE)
            .ok_or_else(|| Error::Invalid(crate::tr!("that isn't a web game")))?;
        let Some(InstallLocation::Managed { root_id, rel_dir }) = &entry.location else {
            return Err(Error::Invalid(crate::tr!(
                "save it for offline play first: only an offline copy can be shared"
            )));
        };
        let root = store
            .get_root(root_id)
            .await?
            .ok_or_else(|| Error::Invalid(crate::tr!("the game's storage isn't available")))?;
        let folder = root.path.join(rel_dir);
        if !folder.join(crate::web::bundle::MANIFEST).is_file() {
            return Err(Error::Invalid(crate::tr!(
                "the offline copy's files aren't there"
            )));
        }
        Ok((entry, folder))
    }

    /// The folder to offer for share `row` other than a game's; `None` for
    /// a collection, which has no files. The save's and the ROM's files are
    /// put together under [`Shares::outgoing`] (linked where the disk
    /// allows), the offline copy's folder is offered as it is.
    pub(super) async fn staged(&self, row: &ShareRow) -> Result<Option<PathBuf>> {
        let details = Details::parse(&row.details_json)
            .ok_or_else(|| Error::Invalid(crate::tr!("this share's description is damaged")))?;
        match details {
            Details::Collection { .. } => Ok(None),
            Details::Web { .. } => {
                let game = row
                    .game_id
                    .as_deref()
                    .ok_or_else(|| Error::Invalid(crate::tr!("this share has no game")))?;
                Ok(Some(self.offline_copy(game).await?.1))
            }
            Details::Save { files, .. } => {
                let saves = self.saves()?;
                let mut sources = Vec::new();
                for file in &files {
                    let name = blob_name(&file.hash).ok_or_else(|| {
                        Error::Invalid(crate::tr!("this share's description is damaged"))
                    })?;
                    let path = saves
                        .blobs()
                        .path(&file.hash)
                        .filter(|path| path.is_file())
                        .ok_or_else(|| {
                            Error::Invalid(crate::tr!(
                                "a file of that save isn't on this device any more"
                            ))
                        })?;
                    sources.push((path, name.to_owned()));
                }
                self.put_together(&row.id, sources).await.map(Some)
            }
            Details::Rom(_) => {
                let entry = row
                    .entry_id
                    .as_deref()
                    .ok_or_else(|| Error::Invalid(crate::tr!("this share has no game")))?;
                let (_, _, sources) = rom::describe(&self.inner.store, entry).await?;
                self.put_together(&row.id, sources).await.map(Some)
            }
        }
    }

    /// A folder of `sources` under their names, made afresh.
    async fn put_together(&self, id: &str, sources: Vec<(PathBuf, String)>) -> Result<PathBuf> {
        rom::put_together(&self.outgoing(id), sources).await
    }

    /// Removes what [`Shares::staged`] put together for share `id`.
    pub(super) async fn unstage(&self, id: &str) {
        let _ = tokio::fs::remove_dir_all(self.outgoing(id)).await;
    }

    /// What `details` holds, as this library sees it.
    pub(super) async fn contents(&self, details: Option<&Details>) -> Result<ShareContents> {
        let store = &self.inner.store;
        Ok(match details {
            None => ShareContents::Game,
            Some(Details::Save { game, files }) => ShareContents::Save {
                game: title_of(game),
                here: store.sync_find_game(game).await?.map(|(id, _)| id),
                files: u32::try_from(files.len()).unwrap_or(u32::MAX),
            },
            Some(Details::Web { url }) => ShareContents::Web {
                url: url.clone(),
                here: store
                    .entry_by_source(crate::web::WEB_SOURCE.as_str(), url)
                    .await?
                    .map(|entry| entry.game_id),
            },
            Some(Details::Rom(rom)) => ShareContents::Rom {
                platform: rom.platform.clone(),
                emulator: rom.definition.clone(),
                ready: rom::emulator_here(store, &rom.platform, rom.definition.as_deref()).await?,
            },
            Some(Details::Collection { name, games, .. }) => {
                let mut list = Vec::with_capacity(games.len());
                for identity in games {
                    let game_id = store.sync_find_game(identity).await?.map(|(id, _)| id);
                    let here = match &game_id {
                        Some(id) => has_game(store, id).await?,
                        None => GameHere::Missing,
                    };
                    list.push(SharedGame {
                        title: title_of(identity),
                        igdb_id: identity
                            .get("igdb")
                            .and_then(Value::as_i64)
                            .map(|id| id.to_string()),
                        here,
                        game_id,
                    });
                }
                ShareContents::Collection {
                    name: name.clone(),
                    games: list,
                }
            }
        })
    }

    /// Puts what arrived in `folder` where `details` says it belongs here,
    /// in `root` when it has files to keep, and returns the game it's part
    /// of.
    pub(super) async fn place(
        &self,
        details: &Details,
        folder: &Path,
        root: &Path,
        title: &str,
    ) -> Result<Placed> {
        let store = &self.inner.store;
        match details {
            Details::Collection { .. } => Err(Error::Invalid(crate::tr!(
                "a collection has no files to receive"
            ))),
            Details::Save { game, files } => {
                let saves = self.saves()?;
                let mut arrived = Vec::with_capacity(files.len());
                for file in files {
                    let name = blob_name(&file.hash).ok_or_else(|| {
                        Error::Invalid(crate::tr!("this share's description is damaged"))
                    })?;
                    arrived.push((file.clone(), folder.join(name)));
                }
                let (game_id, added) = found_or_made(store, game).await?;
                saves.add_shared(&game_id, arrived).await?;
                Ok(Placed { game_id, added })
            }
            Details::Web { url } => {
                let url = crate::web::page_url(url)?.to_string();
                let root = rom::root_at(store, root).await?;
                let existing = store
                    .entry_by_source(crate::web::WEB_SOURCE.as_str(), &url)
                    .await?;
                if let Some(entry) = existing.as_ref().filter(|entry| entry.location.is_some()) {
                    // Theirs stays: it's already offline here.
                    return Ok(Placed {
                        game_id: entry.game_id.clone(),
                        added: false,
                    });
                }
                let added = existing.is_none();
                let (rel_dir, size) = {
                    let (folder, base) = (folder.to_owned(), root.path.clone());
                    let name = crate::transfer::folder_name(title);
                    tokio::task::spawn_blocking(move || {
                        let rel_dir = rom::free_folder(&base, &name)?;
                        let target = base.join(&rel_dir);
                        rom::move_path(&folder, &target)?;
                        let bundle = crate::web::bundle::Bundle::open(&target).map_err(|_| {
                            let _ = std::fs::remove_dir_all(&target);
                            Error::Invalid(crate::tr!("that isn't a web game's offline copy"))
                        })?;
                        Ok::<_, Error>((rel_dir, bundle.stats().1))
                    })
                    .await
                    .map_err(|error| Error::Io(std::io::Error::other(error)))??
                };
                let location = Some(InstallLocation::Managed {
                    root_id: root.id.clone(),
                    rel_dir,
                });
                let entry = match existing {
                    Some(mut entry) => {
                        entry.location = location;
                        entry.size = Some(size);
                        entry
                    }
                    None => {
                        let mut game = crate::model::Game::new(if title.trim().is_empty() {
                            "Web game"
                        } else {
                            title.trim()
                        });
                        game.platforms = vec![crate::web::PLATFORM.to_owned()];
                        let game = store.upsert_game(&game).await?;
                        let mut entry =
                            GameEntry::new(&game.id, crate::web::WEB_SOURCE, url.as_str());
                        entry.state = InstallState::Installed;
                        entry.launch = Some(LaunchSpec::Web { url });
                        entry.location = location;
                        entry.size = Some(size);
                        entry
                    }
                };
                store.upsert_entry(&entry).await?;
                Ok(Placed {
                    game_id: entry.game_id,
                    added,
                })
            }
            Details::Rom(copy) => {
                rom::place(
                    store,
                    self.inner.transfers.emulator_installer(),
                    copy,
                    folder,
                    root,
                    title,
                    None,
                )
                .await
            }
        }
    }

    /// Receives the collection `text` links to as a collection of
    /// `account`'s: the games they have, and a title-only record of each one
    /// they don't, so it lists every game. Nothing is downloaded.
    pub async fn receive_collection(&self, text: &str, account: &str) -> Result<Collection> {
        let (info, details) = self.look(text).await?;
        let Some(Details::Collection {
            name,
            description,
            games,
        }) = details
        else {
            return Err(Error::Invalid(crate::tr!(
                "that link doesn't share a collection"
            )));
        };
        let store = &self.inner.store;
        let mut ids = Vec::with_capacity(games.len());
        for identity in &games {
            let (id, _) = found_or_made(store, identity).await?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let description = if description.trim().is_empty() {
            crate::tr!("Shared by {sharer}", sharer = info.sharer)
        } else {
            description
        };
        let collection = store
            .save_collection(
                account,
                &SaveCollection {
                    id: None,
                    name: if name.trim().is_empty() {
                        info.title
                    } else {
                        name
                    },
                    description,
                    kind: CollectionKind::Manual,
                },
            )
            .await?;
        if !ids.is_empty() {
            store
                .add_to_collection(account, &collection.id, &ids)
                .await?;
        }
        self.changed();
        store
            .get_collection(account, &collection.id)
            .await?
            .ok_or_else(|| crate::db::missing("collection", &collection.id))
    }
}
