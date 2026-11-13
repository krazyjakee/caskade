//! What the Store reads (#102): the IGDB catalogue through the metadata
//! server, and which catalogue games the library has. [`StoreData`] is the
//! running app's; a test provides one of its own (`use_context_provider`).
//! Without the app's services (fixtures mode) every read says the catalogue
//! isn't available.

use crate::data::change;
use crate::game::launch::Answer;
use caskade_core::db::IgdbOwned;
use caskade_core::error::ErrorReport;
use caskade_core::igdb::{
    CatalogReleases, CatalogSearch, IgdbCatalogFilters, IgdbCatalogPage, IgdbLookup,
};
use dioxus_native::prelude::*;
use std::rc::Rc;

/// Which list of releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    /// The last 90 days', newest first.
    Recent,
    /// Releases to come, soonest first.
    Upcoming,
    /// Announced releases without a date.
    Tbd,
}

#[derive(Clone)]
pub struct StoreData {
    /// A page of games matching a search, or the most-rated without one.
    pub search: Rc<dyn Fn(CatalogSearch) -> Answer<IgdbCatalogPage>>,
    /// A page of recent releases (`tbd` is ignored).
    pub recent: Rc<dyn Fn(CatalogReleases) -> Answer<IgdbCatalogPage>>,
    /// A page of upcoming releases; with `tbd`, the undated ones.
    pub upcoming: Rc<dyn Fn(CatalogReleases) -> Answer<IgdbCatalogPage>>,
    /// The platforms and genres the catalogue can be narrowed by.
    pub filters: Rc<dyn Fn() -> Answer<IgdbCatalogFilters>>,
    /// Catalogue games by IGDB id, in the order asked for.
    pub games: Rc<dyn Fn(Vec<String>) -> Answer<IgdbLookup>>,
    /// The library's games matched to these IGDB games.
    pub owned: Rc<dyn Fn(Vec<String>) -> Answer<Vec<IgdbOwned>>>,
}

impl PartialEq for StoreData {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.search, &other.search)
    }
}

fn services() -> Result<&'static caskade_app::start::Services, ErrorReport> {
    crate::shell::services().ok_or_else(|| {
        ErrorReport::from(caskade_ui_i18n::t!(
            "That isn't available in this copy of Caskade."
        ))
    })
}

impl StoreData {
    /// The running app's.
    pub fn of_app() -> Self {
        use caskade_app::commands::igdb;
        Self {
            search: Rc::new(|search| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        igdb::igdb_catalog_search(&services.igdb, &search)
                            .await
                            .map_err(ErrorReport::from)
                    })
                    .await
                })
            }),
            recent: Rc::new(|releases| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        igdb::igdb_catalog_recent(&services.igdb, &releases)
                            .await
                            .map_err(ErrorReport::from)
                    })
                    .await
                })
            }),
            upcoming: Rc::new(|releases| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        igdb::igdb_catalog_upcoming(&services.igdb, &releases)
                            .await
                            .map_err(ErrorReport::from)
                    })
                    .await
                })
            }),
            filters: Rc::new(|| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        igdb::igdb_catalog_filters(&services.igdb)
                            .await
                            .map_err(ErrorReport::from)
                    })
                    .await
                })
            }),
            games: Rc::new(|ids| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        igdb::igdb_catalog_games(&services.igdb, &ids)
                            .await
                            .map_err(ErrorReport::from)
                    })
                    .await
                })
            }),
            owned: Rc::new(|ids| {
                Box::pin(async move {
                    let Some(services) = crate::shell::services() else {
                        return Ok(Vec::new());
                    };
                    change(async move {
                        igdb::igdb_owned(&services.store, &ids)
                            .await
                            .map_err(ErrorReport::from)
                    })
                    .await
                })
            }),
        }
    }

    /// A page of `shelf`'s releases.
    pub fn releases(&self, shelf: Shelf, releases: CatalogReleases) -> Answer<IgdbCatalogPage> {
        match shelf {
            Shelf::Recent => (self.recent)(releases),
            Shelf::Upcoming | Shelf::Tbd => (self.upcoming)(CatalogReleases {
                tbd: shelf == Shelf::Tbd,
                ..releases
            }),
        }
    }
}

/// A test's Store data, else the running app's.
pub fn use_store_data() -> StoreData {
    let provided = try_use_context::<StoreData>();
    use_hook(move || provided.unwrap_or_else(StoreData::of_app))
}
