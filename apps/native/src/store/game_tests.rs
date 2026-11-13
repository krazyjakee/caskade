//! A catalogue game's page (#102) in the frame over a scripted catalogue,
//! headless, as `StoreView.test.tsx` has it: opened from the results, its
//! store links, releases, rating and library copy, similar games that go
//! back to it and Back to the results; IGDB's screenshots in the gallery
//! and its videos in the video window; and what it says without videos,
//! stores or the game. Store and website links open the browser, so the
//! tests read them and never press them.

use super::{screenshots, similar_ids, website_label};
use crate::frame::tests::{ONE, Window};
use crate::game::page_tests::page_data_tests::{PageScript, page_script};
use crate::router::Route;
use crate::store::harness::{StoreScript, catalog_game, page, store, store_script};
use blitz_dom::NodeId;
use caskade_core::db::IgdbOwned;
use caskade_core::igdb::{
    IgdbAgeRating, IgdbGame, IgdbImage, IgdbPlatform, IgdbReleaseDate, IgdbStoreLink, IgdbVideo,
    IgdbWebsite, ReleasePrecision,
};

fn shot(id: &str) -> IgdbImage {
    IgdbImage {
        image_id: id.to_owned(),
        width: Some(1920),
        height: Some(1080),
        alpha_channel: None,
        image_type: None,
        url: format!("https://images.igdb.com/igdb/image/upload/t_screenshot_huge/{id}.jpg"),
    }
}

fn platform(id: u64, name: &str, abbreviation: &str) -> IgdbPlatform {
    IgdbPlatform {
        id: Some(id),
        name: Some(name.to_owned()),
        abbreviation: Some(abbreviation.to_owned()),
    }
}

fn ps5() -> IgdbPlatform {
    platform(167, "PlayStation 5", "PS5")
}

fn switch2() -> IgdbPlatform {
    platform(508, "Nintendo Switch 2", "Switch 2")
}

fn rating(board: &str, rating: &str) -> IgdbAgeRating {
    IgdbAgeRating {
        board: board.to_owned(),
        rating: rating.to_owned(),
    }
}

fn store_link(store: &str, name: &str, url: &str, platforms: &[&str]) -> IgdbStoreLink {
    IgdbStoreLink {
        store: store.to_owned(),
        name: name.to_owned(),
        url: url.to_owned(),
        platforms: platforms.iter().map(|name| (*name).to_owned()).collect(),
        countries: Vec::new(),
    }
}

/// The webview's fixture catalogue's Lantern Tide.
fn lantern() -> IgdbGame {
    let mut game = catalog_game(1001, "Lantern Tide");
    game.genres = vec!["Role-playing (RPG)".to_owned(), "Adventure".to_owned()];
    game.platforms.push(ps5());
    game.release_date = Some("2024-03-14".to_owned());
    game.images.cover = None;
    game.images.screenshots = vec![shot("sc1001a"), shot("sc1001b")];
    game.videos = vec![
        IgdbVideo {
            video_id: "lanternTr01".to_owned(),
            name: Some("Launch Trailer".to_owned()),
        },
        IgdbVideo {
            video_id: "lanternGp02".to_owned(),
            name: None,
        },
    ];
    game.age_ratings = vec![rating("pegi", "16"), rating("esrb", "T")];
    let pc = game.platforms[0].clone();
    game.release_dates = vec![
        IgdbReleaseDate {
            platform: Some(pc),
            region: Some("worldwide".to_owned()),
            date: Some("2024-03-14".to_owned()),
            precision: ReleasePrecision::Day,
            year: Some(2024),
            month: Some(3),
            ..IgdbReleaseDate::default()
        },
        IgdbReleaseDate {
            platform: Some(ps5()),
            region: Some("europe".to_owned()),
            date: Some("2024-03-14".to_owned()),
            precision: ReleasePrecision::Day,
            year: Some(2024),
            month: Some(3),
            status: Some("Early Access".to_owned()),
            ..IgdbReleaseDate::default()
        },
        IgdbReleaseDate {
            platform: Some(switch2()),
            date: Some("2027-01-01".to_owned()),
            precision: ReleasePrecision::Quarter,
            year: Some(2027),
            quarter: Some(1),
            human: Some("Q1 2027".to_owned()),
            ..IgdbReleaseDate::default()
        },
    ];
    game.store_links = vec![
        store_link(
            "steam",
            "Steam",
            "https://store.steampowered.com/app/1001",
            &["PC (Microsoft Windows)"],
        ),
        store_link(
            "playstation",
            "PlayStation Store",
            "https://store.playstation.com/product/1001",
            &["PlayStation 5"],
        ),
    ];
    game.websites = vec![
        IgdbWebsite {
            kind: "official".to_owned(),
            name: Some("Official Website".to_owned()),
            url: "https://lantern.example".to_owned(),
        },
        IgdbWebsite {
            kind: "wiki".to_owned(),
            name: Some("Fandom".to_owned()),
            url: "https://lantern.fandom.example".to_owned(),
        },
    ];
    // Itself and one the server sends nothing for are left out.
    game.similar_games = vec![1002, 1003, 1001, 9999];
    game
}

/// Every game the catalogue knows; 9999 is not one.
fn catalogue() -> Vec<IgdbGame> {
    let mut copper = catalog_game(1002, "Copper Orchard");
    copper.genres = vec!["Puzzle".to_owned()];
    copper.store_links = vec![store_link(
        "gog",
        "GOG",
        "https://www.gog.example/game/copper",
        &[],
    )];
    let mut starfall = catalog_game(1003, "Starfall Ledger");
    starfall.platforms = vec![switch2()];
    starfall.status = Some("Alpha".to_owned());
    let mut quiet = catalog_game(1004, "Quiet Harbour");
    quiet.platforms.push(switch2());
    quiet.release_dates = vec![IgdbReleaseDate {
        platform: Some(switch2()),
        ..IgdbReleaseDate::default()
    }];
    vec![lantern(), copper, starfall, quiet]
}

/// The hero's title, once a game's page shows.
fn heading(window: &Window) -> Option<String> {
    let hero = window.find("data-cs", "hero")?;
    window
        .find_in(hero, "data-part", "title")
        .map(|title| window.text(title))
}

fn store_game(href: &str, title: &str) -> Window {
    let mut window = Window::open();
    window.go(href);
    window.settle_until(title, |window| heading(window).as_deref() == Some(title));
    window
}

/// The card titled `title` among `cards`.
fn card_in(window: &Window, cards: Vec<NodeId>, title: &str) -> Option<NodeId> {
    cards.into_iter().find(|&card| {
        window
            .find_in(card, "data-part", "title")
            .is_some_and(|node| window.text(node) == title)
    })
}

fn card(window: &Window, title: &str) -> Option<NodeId> {
    card_in(window, window.find_all("data-cs", "game-card"), title)
}

/// The cards on the similar games' shelf.
fn similar(window: &Window) -> Vec<NodeId> {
    window
        .find("data-focus-section", "shelf-store-similar")
        .map(|shelf| window.find_all_in(shelf, "data-cs", "game-card"))
        .unwrap_or_default()
}

fn back_text(window: &Window) -> String {
    window
        .find("data-part", "back")
        .map(|back| window.text(back))
        .unwrap_or_default()
}

#[test]
fn a_catalog_game_opens_from_the_results_and_back_returns_to_them() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = store_script(StoreScript {
        search: Some(page(vec![lantern()])),
        games: catalogue(),
        owned: vec![IgdbOwned {
            igdb_id: 1001,
            game_id: "fixture-1".to_owned(),
            installed: true,
        }],
        ..StoreScript::default()
    });
    let mut window = Window::open();
    window.go("/store?tab=browse&q=lantern");
    window.settle_until("the results", |window| {
        card(window, "Lantern Tide").is_some()
    });
    let lantern_card = card(&window, "Lantern Tide").unwrap();
    window.click(lantern_card);
    window.settle_until("Lantern Tide's page", |window| {
        heading(window).as_deref() == Some("Lantern Tide")
    });
    match window.route() {
        Route::StoreGame { igdb_id, from } => {
            assert_eq!(igdb_id, "1001");
            assert!(
                from.as_deref()
                    .is_some_and(|from| from.starts_with("/store?")),
                "{from:?}"
            );
        }
        other => panic!("{other:?}"),
    }
    let hero = window.get("data-cs", "hero");
    let eyebrow = window.find_in(hero, "data-part", "eyebrow").unwrap();
    assert_eq!(window.text(eyebrow), "IGDB catalog");

    // Where to buy: each storefront, opened outside the app; the first
    // takes focus.
    let links = window.find_all("data-part", "store-link");
    assert_eq!(links.len(), 2);
    assert_eq!(
        window.attr(links[0], "href").as_deref(),
        Some("https://store.steampowered.com/app/1001")
    );
    assert_eq!(
        window.attr(links[0], "data-store").as_deref(),
        Some("steam")
    );
    assert!(window.text(links[0]).contains("Steam"));
    assert_eq!(
        window.attr(links[0], "data-focus-default").as_deref(),
        Some("true")
    );
    assert!(window.text(links[1]).contains("PlayStation Store"));
    assert_eq!(window.attr(links[1], "data-focus-default"), None);

    // Its releases per platform, and its rating.
    let said = window.view_text();
    assert!(said.contains("Q1 2027"), "{said}");
    let early = window.get("data-release", "early-access");
    assert!(window.text(early).contains("Europe"));
    window.settle_until("the rating", |window| {
        window.find("data-age-rating", "pegi").is_some()
    });
    let rated = window.get("data-age-rating", "pegi");
    assert_eq!(window.text(rated), "PEGI 16");
    assert_eq!(
        window.attr(rated, "title").as_deref(),
        Some("PEGI 16 · ESRB T")
    );

    // In the library already.
    window.settle_until("the library's copy", |window| {
        window.find("href", "/games/fixture-1").is_some()
    });
    let open = window.get("href", "/games/fixture-1");
    assert_eq!(window.text(open), "Open in library");
    let said = window.text(hero);
    assert!(said.contains("Installed · Open in library"), "{said}");

    // Similar games, without itself or the one IGDB sent nothing for.
    window.settle_until("the similar games", |window| similar(window).len() == 2);
    let shelf = similar(&window);
    assert!(card_in(&window, shelf.clone(), "Copper Orchard").is_some());
    assert!(card_in(&window, shelf, "Starfall Ledger").is_some());
    assert!(store(|script| {
        script.lookups.contains(&vec![
            "1002".to_owned(),
            "1003".to_owned(),
            "9999".to_owned(),
        ])
    }));

    // A similar game goes back to this one, and this one to the results.
    let copper = card_in(&window, similar(&window), "Copper Orchard").unwrap();
    window.click(copper);
    window.settle_until("Copper Orchard's page", |window| {
        heading(window).as_deref() == Some("Copper Orchard")
    });
    window.settle_until("no similar games", |window| {
        window.view_text().contains("IGDB lists no similar games.")
    });
    window.settle_until("Back to Lantern Tide", |window| {
        back_text(window).contains("Back to Lantern Tide")
    });
    let back = window.get("data-part", "back");
    window.click(back);
    window.settle_until("Lantern Tide again", |window| {
        heading(window).as_deref() == Some("Lantern Tide")
    });
    window.settle_until("Back to Store", |window| {
        back_text(window).contains("Back to Store")
    });
    let back = window.get("data-part", "back");
    window.click(back);
    window.settle_until("the results again", |window| {
        matches!(window.route(), Route::Store(store) if store.q.as_deref() == Some("lantern"))
    });
}

#[test]
fn igdbs_videos_play_in_the_video_window_and_its_screenshots_fill_the_gallery() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = store_script(StoreScript {
        games: catalogue(),
        ..StoreScript::default()
    });
    let _page = page_script(PageScript::default());
    let played =
        || crate::game::page_tests::page_data_tests::page(|script| script.videos_played.clone());
    let mut window = store_game("/store/games/1001", "Lantern Tide");

    let videos = window.get("data-cs", "videos");
    let buttons = window.find_all_in(videos, "data-part", "video");
    assert_eq!(buttons.len(), 2);
    assert_eq!(
        window.attr(buttons[0], "aria-label").as_deref(),
        Some("Play Launch Trailer")
    );
    window.click(buttons[0]);
    window.settle_until("the trailer", |_| played().len() == 1);
    assert_eq!(
        played()[0],
        (
            "lanternTr01".to_owned(),
            "Lantern Tide · Launch Trailer".to_owned()
        )
    );
    // IGDB gives the second no name.
    assert_eq!(
        window.attr(buttons[1], "aria-label").as_deref(),
        Some("Play Video 2")
    );
    window.click(buttons[1]);
    window.settle_until("the second video", |_| played().len() == 2);
    assert_eq!(
        played()[1],
        (
            "lanternGp02".to_owned(),
            "Lantern Tide · Video 2".to_owned()
        )
    );

    // The row shows IGDB's smaller size; the gallery the full one.
    let thumb = window.get("alt", "Lantern Tide, screenshot 2");
    assert_eq!(
        window.attr(thumb, "src").as_deref(),
        Some("https://images.igdb.com/igdb/image/upload/t_screenshot_med/sc1001b.jpg")
    );
    let shots = window.find_all("data-part", "screenshot");
    assert_eq!(shots.len(), 2);
    // Below the window's fold: a pad's A on it, as a pad user opens it.
    window.focus(shots[1]);
    window.pad(caskade_ui_input::Action::Confirm);
    window.settle_until("the gallery", |window| {
        window.find("data-cs", "screenshot-viewer").is_some()
    });
    let viewer = window.get("data-cs", "screenshot-viewer");
    let image = window.find_in(viewer, "data-part", "image").unwrap();
    assert_eq!(
        window.attr(image, "src").as_deref(),
        Some("https://images.igdb.com/igdb/image/upload/t_screenshot_huge/sc1001b.jpg")
    );
}

#[test]
fn a_game_says_so_without_videos_stores_or_the_game_itself() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = store_script(StoreScript {
        games: catalogue(),
        ..StoreScript::default()
    });
    let mut window = store_game("/store/games/1004", "Quiet Harbour");
    assert!(window.find("data-cs", "videos").is_none());
    assert!(window.find("data-part", "screenshots-empty").is_some());
    assert!(window.find("data-cs", "store-links").is_none());
    let none = window.get("data-part", "no-stores");
    assert_eq!(window.text(none), "IGDB lists no stores for this game.");
    // Opened from nowhere: back to the Store.
    assert!(back_text(&window).contains("Back to Store"));
    // An undated release is TBD.
    assert!(window.view_text().contains("TBD"));

    // A game in development says so above its title.
    window.go("/store/games/1003");
    window.settle_until("Starfall Ledger's page", |window| {
        heading(window).as_deref() == Some("Starfall Ledger")
    });
    let hero = window.get("data-cs", "hero");
    let eyebrow = window.find_in(hero, "data-part", "eyebrow").unwrap();
    assert_eq!(window.text(eyebrow), "Alpha");
    assert!(
        window
            .view_text()
            .contains("IGDB has no release dates for this game.")
    );

    window.go("/store/games/9999");
    window.settle_until("no such game", |window| {
        window.view_text().contains("IGDB has no game 9999.")
    });
    assert!(window.find("data-cs", "hero").is_none());
}

#[test]
fn the_gallery_takes_igdbs_full_size_and_the_row_its_smaller_one() {
    let shots = screenshots(&lantern());
    assert_eq!(shots.len(), 2);
    assert_eq!(
        shots[0].thumb,
        "https://images.igdb.com/igdb/image/upload/t_screenshot_med/sc1001a.jpg"
    );
    assert_eq!(
        shots[0].full,
        "https://images.igdb.com/igdb/image/upload/t_screenshot_huge/sc1001a.jpg"
    );
}

#[test]
fn similar_games_leave_out_the_game_itself_and_repeats_and_stop_at_twelve() {
    let mut game = catalog_game(1, "One");
    game.similar_games = [2, 2, 1, 3].into_iter().chain(4..30).collect();
    let ids = similar_ids(&game);
    assert_eq!(ids.len(), 12);
    assert_eq!(ids[..3], [2, 3, 4]);
    assert!(similar_ids(&catalog_game(1, "None")).is_empty());
}

#[test]
fn a_website_is_named_by_its_kind_else_by_igdb() {
    let site = |kind: &str, name: Option<&str>| IgdbWebsite {
        kind: kind.to_owned(),
        name: name.map(str::to_owned),
        url: "https://example.invalid".to_owned(),
    };
    assert_eq!(
        website_label(&site("official", Some("Official Website"))),
        "Official site"
    );
    assert_eq!(website_label(&site("wiki", Some("Fandom"))), "Wiki");
    assert_eq!(website_label(&site("other", Some("Fandom"))), "Fandom");
    assert_eq!(website_label(&site("other", None)), "Website");
}
