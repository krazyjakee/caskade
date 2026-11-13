//! The Store's catalogue in tests: a scripted [`StoreData`] the test
//! window's app provides (`frame::tests`), answering from a [`StoreScript`]
//! and keeping what it was asked. Nothing reaches the network.

use super::data::StoreData;
use caskade_core::db::IgdbOwned;
use caskade_core::error::ErrorReport;
use caskade_core::igdb::{
    CatalogReleases, CatalogSearch, IgdbCatalogFilters, IgdbCatalogPage, IgdbGame, IgdbImage,
    IgdbImages, IgdbLookup, IgdbPlatform,
};
use std::rc::Rc;
use std::sync::Mutex;

/// What the catalogue answers, and what it was asked.
#[derive(Default)]
pub(crate) struct StoreScript {
    /// Every search's answer; `None` fails it.
    pub(crate) search: Option<IgdbCatalogPage>,
    /// Pages after the first, by offset.
    pub(crate) more: Vec<(u32, IgdbCatalogPage)>,
    pub(crate) recent: Option<IgdbCatalogPage>,
    pub(crate) upcoming: Option<IgdbCatalogPage>,
    pub(crate) tbd: Option<IgdbCatalogPage>,
    pub(crate) filters: IgdbCatalogFilters,
    /// The games a lookup knows; others are not found.
    pub(crate) games: Vec<IgdbGame>,
    pub(crate) owned: Vec<IgdbOwned>,
    pub(crate) searches: Vec<CatalogSearch>,
    pub(crate) releases: Vec<(&'static str, CatalogReleases)>,
    pub(crate) lookups: Vec<Vec<String>>,
}

static STORE: Mutex<Option<StoreScript>> = Mutex::new(None);

pub(crate) fn store<T>(f: impl FnOnce(&mut StoreScript) -> T) -> T {
    let mut script = caskade_core::lock(&STORE);
    f(script.get_or_insert_with(StoreScript::default))
}

pub(crate) struct StoreScripted;

/// Scripts the catalogue until the guard drops.
pub(crate) fn store_script(script: StoreScript) -> StoreScripted {
    *caskade_core::lock(&STORE) = Some(script);
    StoreScripted
}

impl Drop for StoreScripted {
    fn drop(&mut self) {
        *caskade_core::lock(&STORE) = None;
    }
}

fn answer<T: 'static>(value: Result<T, ErrorReport>) -> crate::game::launch::Answer<T> {
    Box::pin(async move { value })
}

fn unreachable() -> ErrorReport {
    ErrorReport::from("the metadata server didn't answer".to_owned())
}

/// The Store data every test window reads.
pub(crate) fn fake_store_data() -> StoreData {
    StoreData {
        search: Rc::new(|search| {
            answer(store(|script| {
                script.searches.push(search.clone());
                if search.offset > 0 {
                    return script
                        .more
                        .iter()
                        .find(|(offset, _)| *offset == search.offset)
                        .map(|(_, page)| page.clone())
                        .ok_or_else(unreachable);
                }
                script.search.clone().ok_or_else(unreachable)
            }))
        }),
        recent: Rc::new(|releases| {
            answer(store(|script| {
                script.releases.push(("recent", releases));
                script.recent.clone().ok_or_else(unreachable)
            }))
        }),
        upcoming: Rc::new(|releases| {
            answer(store(|script| {
                let tbd = releases.tbd;
                script
                    .releases
                    .push((if tbd { "tbd" } else { "upcoming" }, releases));
                if tbd {
                    script.tbd.clone()
                } else {
                    script.upcoming.clone()
                }
                .ok_or_else(unreachable)
            }))
        }),
        filters: Rc::new(|| answer(Ok(store(|script| script.filters.clone())))),
        games: Rc::new(|ids| {
            answer(Ok(store(|script| {
                script.lookups.push(ids.clone());
                let mut lookup = IgdbLookup::default();
                for id in &ids {
                    let id: u64 = id.parse().unwrap_or_default();
                    match script.games.iter().find(|game| game.id == id) {
                        Some(game) => lookup.games.push(game.clone()),
                        None => lookup.not_found.push(id),
                    }
                }
                lookup
            })))
        }),
        owned: Rc::new(|ids| {
            answer(Ok(store(|script| {
                script
                    .owned
                    .iter()
                    .filter(|copy| ids.contains(&copy.igdb_id.to_string()))
                    .cloned()
                    .collect()
            })))
        }),
    }
}

/// A catalogue game for tests: on PC, with a cover.
pub(crate) fn catalog_game(id: u64, title: &str) -> IgdbGame {
    IgdbGame {
        id,
        title: title.to_owned(),
        platforms: vec![IgdbPlatform {
            id: Some(6),
            name: Some("PC (Microsoft Windows)".to_owned()),
            abbreviation: Some("PC".to_owned()),
        }],
        release_date: Some("2024-05-01".to_owned()),
        images: IgdbImages {
            cover: Some(IgdbImage {
                image_id: format!("co{id}"),
                width: None,
                height: None,
                alpha_channel: None,
                image_type: None,
                url: format!("https://images.igdb.com/igdb/image/upload/t_cover_big/co{id}.jpg"),
            }),
            ..IgdbImages::default()
        },
        ..IgdbGame::default()
    }
}

/// A catalogue page of `games`, with nothing after it.
pub(crate) fn page(games: Vec<IgdbGame>) -> IgdbCatalogPage {
    IgdbCatalogPage {
        games,
        ..IgdbCatalogPage::default()
    }
}
