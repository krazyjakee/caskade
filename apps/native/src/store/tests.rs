//! The Store (#102) in the frame over a scripted catalogue, headless, as
//! `StoreView.test.tsx` has it: Browse's most-rated beside the release
//! shelves a page at a time, search once typing stops, what the lists say
//! when nothing matches or the server is away, the library's games marked,
//! upcoming dates as precise as known, and LB/RB through the tabs.

use super::harness::{StoreScript, catalog_game, page, store, store_script};
use crate::frame::tests::{ONE, Window};
use crate::router::Route;
use caskade_core::db::IgdbOwned;
use caskade_core::igdb::{IgdbCatalogPage, IgdbListedRelease, IgdbReleaseDate, ReleasePrecision};
use caskade_ui_input::Action;

fn games(range: std::ops::Range<u64>) -> Vec<caskade_core::igdb::IgdbGame> {
    range
        .map(|id| catalog_game(id, &format!("Catalog Game {id}")))
        .collect()
}

fn open(href: &str) -> Window {
    let mut window = Window::open();
    window.go(href);
    window.settle_until("the Store", |window| {
        window.find("data-view", "store").is_some()
    });
    window
}

/// The titles in the results grid.
fn results(window: &Window) -> Vec<String> {
    let Some(grid) = window.find("data-catalog", "results") else {
        return Vec::new();
    };
    window
        .find_all_in(grid, "data-cs", "game-card")
        .into_iter()
        .filter_map(|card| window.find_in(card, "data-part", "title"))
        .map(|title| window.text(title))
        .collect()
}

fn button(window: &Window, label: &str) -> Option<blitz_dom::NodeId> {
    window
        .find_all("type", "button")
        .into_iter()
        .find(|&node| window.text(node).trim() == label)
}

#[test]
fn browse_lists_the_past_years_most_rated_beside_the_shelves_a_page_at_a_time() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = store_script(StoreScript {
        search: Some(IgdbCatalogPage {
            next_offset: Some(24),
            ..page(games(1..25))
        }),
        more: vec![(24, page(games(25..42)))],
        recent: Some(page(games(100..104))),
        upcoming: Some(page(games(200..203))),
        ..StoreScript::default()
    });
    let mut window = open("/store");
    window.settle_until("the results", |window| results(window).len() == 24);
    assert_eq!(results(&window)[0], "Catalog Game 1");
    let search = store(|script| script.searches[0].clone());
    // The past year, to today, without a query.
    assert!(search.released_from.is_some(), "{search:?}");
    assert_eq!(search.released_to, None);
    assert_eq!(search.query, None);
    let said = window.view_text();
    assert!(said.contains("Most rated this past year"), "{said}");
    window.settle_until("the shelves", |window| {
        window
            .find("data-focus-section", "shelf-store-recent")
            .is_some()
            && window
                .find("data-focus-section", "shelf-store-upcoming")
                .is_some()
    });
    let more = button(&window, "Load more").expect("Load more");
    window.click(more);
    window.settle_until("the next page", |window| results(window).len() == 41);
    assert!(button(&window, "Load more").is_none());
    assert_eq!(store(|script| script.searches.last().unwrap().offset), 24);
}

#[test]
fn search_waits_for_typing_to_stop_and_keeps_the_text_in_the_route() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = store_script(StoreScript {
        search: Some(page(games(1..3))),
        ..StoreScript::default()
    });
    let mut window = open("/store");
    window.settle_until("the results", |window| results(window).len() == 2);
    let field = window
        .find_all("aria-label", "Search the catalog")
        .into_iter()
        .find(|&node| window.attr(node, "data-part").as_deref() == Some("input"))
        .or_else(|| window.find("aria-label", "Search the catalog"))
        .unwrap();
    window.focus(field);
    window.type_text("tide");
    window.settle_until("one search for the last text", |window| {
        matches!(window.route(), Route::Store(store) if store.q.as_deref() == Some("tide"))
    });
    let queries: Vec<Option<String>> = store(|script| {
        script
            .searches
            .iter()
            .map(|search| search.query.clone())
            .collect()
    });
    // The first load, then only "tide": no search per keystroke.
    assert_eq!(queries, [None, Some("tide".to_owned())]);
    let said = window.view_text();
    assert!(said.contains("Results for “tide”"), "{said}");
}

#[test]
fn the_lists_say_when_nothing_matches_the_server_is_away_or_results_are_saved() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let script = store_script(StoreScript {
        search: Some(page(Vec::new())),
        ..StoreScript::default()
    });
    let mut window = open("/store?tab=browse&q=nothing");
    window.settle_until("nothing matches", |window| {
        window.find("data-part", "empty").is_some()
    });
    assert!(window.view_text().contains("No games match “nothing”."));
    drop(script);
    let _script = store_script(StoreScript::default());
    window.go("/store?tab=browse&q=away");
    window.settle_until("the server is away", |window| {
        window
            .view_text()
            .contains("Could not reach the game catalog")
    });
    assert!(button(&window, "Try again").is_some());
    store(|script| {
        script.search = Some(IgdbCatalogPage {
            stale: true,
            sort_unavailable: true,
            ..page(games(1..3))
        });
    });
    let again = button(&window, "Try again").unwrap();
    window.click(again);
    window.settle_until("saved results", |window| results(window).len() == 2);
    let said = window.view_text();
    assert!(said.contains("Showing saved results"), "{said}");
    assert!(window.find("data-part", "sort-unavailable").is_some());
}

#[test]
fn the_librarys_games_are_marked_installed_or_in_the_library() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = store_script(StoreScript {
        search: Some(page(games(1..4))),
        owned: vec![
            IgdbOwned {
                igdb_id: 1,
                game_id: "fixture-1".into(),
                installed: true,
            },
            IgdbOwned {
                igdb_id: 2,
                game_id: "fixture-0".into(),
                installed: false,
            },
        ],
        ..StoreScript::default()
    });
    let mut window = open("/store?tab=browse&q=x");
    window.settle_until("marked", |window| {
        window.find("data-part", "owned").is_some()
    });
    let grid = window.get("data-catalog", "results");
    let statuses: Vec<Option<String>> = window
        .find_all_in(grid, "data-cs", "game-card")
        .into_iter()
        .map(|card| window.attr(card, "data-status"))
        .collect();
    assert_eq!(
        statuses,
        [
            Some("installed".to_owned()),
            Some("not-installed".to_owned()),
            None
        ]
    );
    let said = window.text(grid);
    assert!(
        said.contains("Installed") && said.contains("In library"),
        "{said}"
    );
}

#[test]
fn upcoming_shows_dates_as_precisely_as_known_and_tbd_apart() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let release = |game: u64, release: IgdbReleaseDate| IgdbListedRelease { game, release };
    let _script = store_script(StoreScript {
        upcoming: Some(IgdbCatalogPage {
            releases: vec![
                release(
                    1,
                    IgdbReleaseDate {
                        precision: ReleasePrecision::Quarter,
                        year: Some(2027),
                        quarter: Some(1),
                        ..IgdbReleaseDate::default()
                    },
                ),
                release(
                    2,
                    IgdbReleaseDate {
                        precision: ReleasePrecision::Year,
                        year: Some(2028),
                        status: Some("Cancelled".into()),
                        ..IgdbReleaseDate::default()
                    },
                ),
            ],
            ..page(games(1..3))
        }),
        tbd: Some(page(games(5..6))),
        ..StoreScript::default()
    });
    let mut window = open("/store?tab=upcoming");
    window.settle_until("the releases", |window| results(window).len() == 2);
    let grid = window.get("data-catalog", "results");
    let metas: Vec<String> = window
        .find_all_in(grid, "data-part", "meta")
        .into_iter()
        .map(|meta| window.text(meta))
        .collect();
    assert_eq!(metas, ["Q1 2027", "2028 · Cancelled"]);
    let tbd = window
        .find_all("role", "radio")
        .into_iter()
        .chain(window.find_all("type", "button"))
        .find(|&node| window.text(node).trim() == "TBD")
        .expect("TBD");
    window.click(tbd);
    window.settle_until("the undated ones", |window| {
        results(window) == ["Catalog Game 5"]
    });
    assert!(matches!(window.route(), Route::Store(store) if store.tbd));
    assert_eq!(store(|script| script.releases.last().unwrap().0), "tbd");
}

#[test]
fn lb_and_rb_switch_the_stores_tabs() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = store_script(StoreScript {
        search: Some(page(games(1..3))),
        recent: Some(page(games(10..12))),
        upcoming: Some(page(games(20..22))),
        ..StoreScript::default()
    });
    let mut window = open("/store");
    window.settle_until("the results", |window| results(window).len() == 2);
    let tab = |window: &Window| match window.route() {
        Route::Store(store) => store.tab,
        other => panic!("{other:?}"),
    };
    window.pad(Action::TabNext);
    window.settle_until("Recently released", |window| tab(window) == "recent");
    window.pad(Action::TabNext);
    window.settle_until("Upcoming", |window| tab(window) == "upcoming");
    window.pad(Action::TabNext);
    window.settle_until("Browse again", |window| tab(window) == "browse");
    window.pad(Action::TabPrev);
    window.settle_until("Upcoming again", |window| tab(window) == "upcoming");
    let selected = window
        .find_all("role", "tab")
        .into_iter()
        .find(|&node| window.attr(node, "data-selected").as_deref() == Some("true"))
        .unwrap();
    assert_eq!(window.text(selected), "Upcoming");
}
