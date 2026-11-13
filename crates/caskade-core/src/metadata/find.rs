//! Where a game's IGDB id comes from: its RomM ROM, the Hasheous dumps of
//! its platforms, or (asked in one batch per chunk) its store ids.

use super::{MatchSource, Metadata};
use crate::{
    Result,
    hasheous::{DumpGame, MAC, RomHashes, WINDOWS},
    igdb::StoreId,
    model::{Game, GameEntry, LaunchSpec},
};
use caskade_discovery::ExternalId;
use std::collections::{BTreeSet, HashMap};

/// Hasheous results looked at per title.
const SEARCH_LIMIT: u32 = 20;

/// What the game's RomM ROM and the Hasheous dumps say about its IGDB id.
pub(super) enum Local {
    Found(u64, MatchSource),
    NotFound,
    /// The dumps couldn't be searched, so "not found" isn't known (#160):
    /// recording none would keep the game unmatched for a week.
    Unsure,
}

impl Metadata {
    /// An id from the game's RomM ROM or the Hasheous dumps.
    pub(super) async fn find_local(&self, game: &Game, entries: &[GameEntry]) -> Result<Local> {
        let mut hashes = Vec::new();
        for entry in entries {
            if !entry.source_id.as_str().starts_with("romm:") {
                continue;
            }
            let Some(rom) = self.inner.store.romm_rom_by_entry(&entry.id).await? else {
                continue;
            };
            if let Some(id) = rom.rom.igdb_id.filter(|id| *id > 0) {
                return Ok(Local::Found(u64::from(id), MatchSource::Romm));
            }
            hashes.push(RomHashes {
                sha1: rom.rom.sha1_hash,
                md5: rom.rom.md5_hash,
                crc: rom.rom.crc_hash,
                size: Some(rom.rom.fs_size_bytes),
                ..Default::default()
            });
            hashes.extend(
                rom.rom
                    .files
                    .into_iter()
                    .filter(|file| {
                        file.category
                            .as_deref()
                            .is_none_or(|category| category == "game")
                    })
                    .map(|file| RomHashes {
                        sha1: file.sha1_hash,
                        md5: file.md5_hash,
                        crc: file.crc_hash,
                        size: Some(file.file_size_bytes),
                        ..Default::default()
                    }),
            );
        }
        // All RomM ids take precedence over hashes; hashes take precedence
        // over names. A hash can identify a ROM whose filename isn't a title.
        for hashes in hashes {
            match self.inner.hasheous.lookup(&hashes).await {
                Ok(found) => {
                    let ids = igdb_ids(found.iter().map(|matched| &matched.game));
                    if ids.len() == 1 {
                        return Ok(Local::Found(*ids.first().unwrap(), MatchSource::Hasheous));
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, game = %game.id, "Hasheous hash lookup failed");
                    return Ok(Local::Unsure);
                }
            }
        }
        let dumps = self.dumps_for(entries);
        if dumps.is_empty() {
            return Ok(Local::NotFound);
        }
        for title in titles(&game.title) {
            match self
                .inner
                .hasheous
                .search(&title, &dumps, SEARCH_LIMIT)
                .await
            {
                Ok(found) => {
                    if let Some(id) = unique_igdb(&found, &title) {
                        return Ok(Local::Found(id, MatchSource::Hasheous));
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, game = %game.id, "Hasheous search failed");
                    return Ok(Local::Unsure);
                }
            }
        }
        Ok(Local::NotFound)
    }

    /// The dumps of the game's platforms: an emulated entry's platform,
    /// and Windows and Mac for everything else.
    fn dumps_for(&self, entries: &[GameEntry]) -> Vec<String> {
        let mut dumps = BTreeSet::new();
        for entry in entries {
            match &entry.launch {
                Some(LaunchSpec::Emulator { platform_id, .. }) => {
                    if let Some(dump) = self
                        .inner
                        .catalog
                        .platform(platform_id)
                        .and_then(|platform| platform.ids.hasheous.clone())
                    {
                        dumps.insert(dump);
                    }
                }
                _ => {
                    dumps.insert(WINDOWS.to_owned());
                    dumps.insert(MAC.to_owned());
                }
            }
        }
        dumps.into_iter().collect()
    }
}

/// The title, then without trademark signs and doubled spaces.
fn titles(title: &str) -> Vec<String> {
    let plain = title
        .replace(['™', '®', '©'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut titles = vec![title.trim().to_owned()];
    if !plain.is_empty() && plain != titles[0] {
        titles.push(plain);
    }
    titles
}

/// The one IGDB game `found` names: all of them, or else those named
/// exactly `title`. Several different games is no answer.
fn unique_igdb(found: &[DumpGame], title: &str) -> Option<u64> {
    let all = igdb_ids(found.iter());
    if all.len() == 1 {
        return all.first().copied();
    }
    let exact = igdb_ids(
        found
            .iter()
            .filter(|game| game.name.eq_ignore_ascii_case(title)),
    );
    (exact.len() == 1).then(|| exact.first().copied()).flatten()
}

fn igdb_ids<'a>(games: impl Iterator<Item = &'a DumpGame>) -> BTreeSet<u64> {
    games
        .flat_map(|game| &game.metadata)
        .filter_map(|metadata| {
            let link = metadata.link.as_deref().unwrap_or_default();
            // Hasheous also lists the platform itself, with an IGDB
            // platform id.
            (metadata.source.eq_ignore_ascii_case("IGDB")
                && (link.is_empty() || link.contains("/games/")))
            .then(|| metadata.id.trim().parse::<u64>().ok())
            .flatten()
            .filter(|id| *id > 0 && i64::try_from(*id).is_ok())
        })
        .collect()
}

/// The Epic namespaces of `ids`, whose store offers IGDB knows their
/// games by.
pub(super) fn epic_namespaces(ids: &[ExternalId]) -> BTreeSet<String> {
    ids.iter()
        .filter_map(|id| match id {
            ExternalId::Epic { namespace, .. } if !namespace.is_empty() => Some(namespace.clone()),
            _ => None,
        })
        .collect()
}

/// The store ids among `ids` that IGDB links to, in order (the primary
/// entry's first), with `offers`: each Epic namespace's base-game offers.
pub(super) fn store_ids(
    external: &[ExternalId],
    offers: &HashMap<String, Vec<String>>,
) -> Vec<StoreId> {
    let mut ids = Vec::new();
    for id in external {
        match id {
            ExternalId::Steam { app_id } => ids.push(StoreId::new("steam", app_id.to_string())),
            ExternalId::Gog { product_id } => {
                ids.push(StoreId::new("gog", product_id.to_string()));
            }
            // IGDB has the offer the store sells the game as, which the
            // launcher never names; the other ids go too, in case.
            ExternalId::Epic {
                namespace,
                catalog_item,
                app_name,
            } => {
                for offer in offers.get(namespace).into_iter().flatten() {
                    ids.push(StoreId::new("epic", offer.clone()));
                }
                for uid in [catalog_item, namespace, app_name] {
                    if !uid.is_empty() {
                        ids.push(StoreId::new("epic", uid.clone()));
                    }
                }
            }
            ExternalId::Itch { game_id } => ids.push(StoreId::new("itch", game_id.to_string())),
            ExternalId::Amazon { id } => {
                let store = if id.starts_with("amzn1.adg.") {
                    "amazon_adg"
                } else {
                    "amazon"
                };
                ids.push(StoreId::new(store, id.clone()));
            }
            ExternalId::MicrosoftStore { store_id } => {
                ids.push(StoreId::new("microsoft", store_id.clone()));
            }
            _ => {}
        }
    }
    let mut seen = BTreeSet::new();
    ids.retain(|id| seen.insert((id.store.clone(), id.uid.clone())));
    ids
}

#[cfg(test)]
mod unit {
    use super::*;
    use crate::hasheous::MetadataId;

    fn dump_game(name: &str, igdb: &str, link: &str) -> DumpGame {
        DumpGame {
            dump: WINDOWS.into(),
            id: "1".into(),
            name: name.into(),
            metadata: vec![MetadataId {
                source: "IGDB".into(),
                id: igdb.into(),
                link: Some(link.into()),
            }],
        }
    }

    #[test]
    fn titles_drop_trademark_signs() {
        assert_eq!(titles("Portal 2"), ["Portal 2"]);
        assert_eq!(
            titles("The Lord of the Rings: Return to Moria™"),
            [
                "The Lord of the Rings: Return to Moria™",
                "The Lord of the Rings: Return to Moria"
            ]
        );
    }

    #[test]
    fn one_igdb_game_is_a_match_and_platform_rows_are_not() {
        let worms = dump_game("Worms", "9331", "https://www.igdb.com/games/worms");
        let platform = dump_game("Worms", "6", "https://www.igdb.com/platforms/win");
        assert_eq!(unique_igdb(&[worms.clone(), platform], "Worms"), Some(9331));
        // Two games: the one with the exact name, or none.
        let alias = dump_game("Worms Armageddon", "9332", "https://www.igdb.com/games/wa");
        assert_eq!(
            unique_igdb(&[worms.clone(), alias.clone()], "Worms"),
            Some(9331)
        );
        let other = dump_game("Worms", "9333", "https://www.igdb.com/games/worms-2");
        assert_eq!(unique_igdb(&[worms, other, alias], "Worms"), None);
        assert_eq!(unique_igdb(&[], "Worms"), None);
    }
}
