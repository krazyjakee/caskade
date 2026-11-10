//! The router's hrefs, Back chain and history, against the webview's own
//! cases (`router.tsx`, `views/game/origin.test.tsx`,
//! `shell/startView.test.ts`).

use super::href::{self, Href, MAX_GAMES, Value};
use super::route::{LibraryPrefs, Remembered};
use super::*;
use crate::library::Layout;
use caskade_core::model::{GameFilter, GameSort};

fn parse(href: &str) -> Route {
    Route::parse(href, &Remembered::default())
}

#[test]
fn hrefs_read_and_write_as_tanstack_routers() {
    let href = Href::parse("/library?filter=installed&q=a%20b&opening=true&n=%22123%22&year=1999")
        .unwrap();
    assert_eq!(href.get_text("filter"), Some("installed"));
    assert_eq!(href.get_text("q"), Some("a b"));
    assert_eq!(href.get("opening"), Some(&Value::Bool(true)));
    // Text that would read back as JSON is quoted.
    assert_eq!(href.get_text("n"), Some("123"));
    assert_eq!(href.get("year"), Some(&Value::Number(1999.0)));
    assert_eq!(
        Href::new("/library")
            .text("q", Some("123"))
            .text("t", Some("Pokémon & co"))
            .with("opening", Some(Value::Bool(true)))
            .to_string(),
        "/library?q=%22123%22&t=Pok%C3%A9mon%20%26%20co&opening=true"
    );
    for outside in ["https://evil.example/", "//evil.example", "/\\x", "library"] {
        assert_eq!(Href::parse(outside), None, "{outside}");
    }
}

#[test]
fn every_page_of_the_webview_parses_and_writes_back() {
    for href in [
        "/",
        "/library?filter=installed&sort=title&layout=list&q=zelda&platform=SNES",
        "/games/abc?from=%2Flibrary%3Ffilter%3Dall%26sort%3Drecent%26layout%3Dgrid",
        "/activity?tab=notifications&unread=true&open=n1",
        "/store?tab=upcoming&sort=popular&q=dark&platform=6&genre=12&year=2001&tbd=true",
        "/store/games/1001",
        "/collections?filter=smart",
        "/collections/weekend",
        "/settings?tab=devices&section=pairing",
        "/plugins/dev.me.stats/overview",
    ] {
        let route = parse(href);
        assert!(!matches!(route, Route::NotFound { .. }), "{href}");
        assert_eq!(parse(&route.href()), route, "{href} → {}", route.href());
    }
    assert!(matches!(parse("/nowhere"), Route::NotFound { .. }));
    assert_eq!(
        parse("/activity"),
        Route::Activity {
            tab: ActivityTab::Jobs,
            unread: false,
            open: None
        }
    );
    // The Store checks its search as the webview's `validateStoreSearch`.
    let Route::Store(store) = parse("/store?tab=x&sort=y&platform=abc&year=1800&q=%20%20") else {
        panic!()
    };
    assert_eq!(
        store,
        StoreRoute {
            tab: "browse",
            ..StoreRoute::default()
        }
    );
}

#[test]
fn a_library_link_leaving_things_out_comes_back_to_the_remembered_view() {
    let remembered = Remembered {
        library: LibraryPrefs {
            filter: GameFilter::Favorites,
            sort: GameSort::Title,
            layout: Layout::List,
        },
        settings_tab: Some("storage"),
    };
    let Route::Library(library) = Route::parse("/library", &remembered) else {
        panic!()
    };
    assert_eq!(
        (
            library.filter,
            library.sort,
            library.layout,
            library.opening
        ),
        (GameFilter::Favorites, GameSort::Title, Layout::List, true)
    );
    let Route::Library(library) = Route::parse("/library?filter=web&q=%20x%20", &remembered) else {
        panic!()
    };
    assert_eq!(library.filter, GameFilter::Web);
    assert_eq!(library.q.as_deref(), Some("x"));
    assert!(!library.opening, "a link that names the filter chose it");
    assert_eq!(
        Route::parse("/settings", &remembered),
        Route::Settings {
            tab: "storage",
            section: None
        }
    );
}

#[test]
fn game_pages_go_back_only_to_app_pages_and_never_too_deep() {
    let valid = |value: &str| href::valid_return(Some(value));
    assert_eq!(
        valid("/library?filter=installed&q=a%20b").as_deref(),
        Some("/library?filter=installed&q=a%20b")
    );
    assert_eq!(valid("/").as_deref(), Some("/"));
    for outside in [
        "https://evil.example/",
        "//evil.example",
        "/\\evil.example",
        "javascript:alert(1)",
        "library",
        "/settings?tab=general",
        "/nowhere",
    ] {
        assert_eq!(valid(outside), None, "{outside}");
    }
    assert_eq!(href::valid_return(None), None);

    // A game opened from a game keeps that game's own way back.
    let nested = format!(
        "/games/b?from={}",
        href::segment("/games/a?from=%2Flibrary")
    );
    assert_eq!(valid(&nested).as_deref(), Some(nested.as_str()));
    // A game's link that names itself skips itself.
    assert_eq!(
        href::back_target("b", Some(&nested), "/games").as_deref(),
        Some("/games/a?from=%2Flibrary")
    );
    // A bad link inside the chain ends it there.
    assert_eq!(
        valid("/games/a?from=https%3A%2F%2Fevil.example").as_deref(),
        Some("/games/a")
    );

    // Deeper than MAX_GAMES: the oldest games go, the browsing page stays.
    let mut chain = "/".to_owned();
    for i in 0..MAX_GAMES + 2 {
        chain = format!("/games/g{i}?from={}", href::segment(&chain));
    }
    let mut kept = Vec::new();
    let mut at = valid(&chain);
    while let Some(page) = at.as_deref().filter(|page| page.starts_with("/games/")) {
        let parsed = Href::parse(page).unwrap();
        kept.push(parsed.path.clone());
        at = parsed.get_text("from").map(str::to_owned);
    }
    let expected: Vec<String> = (0..MAX_GAMES)
        .map(|i| format!("/games/g{}", MAX_GAMES + 1 - i))
        .collect();
    assert_eq!(kept, expected);
    assert_eq!(at.as_deref(), Some("/"));

    // The Store's catalog pages chain the same way (#163).
    let store = "/store?tab=upcoming&platform=6";
    assert_eq!(valid(store).as_deref(), Some(store));
    let similar = format!("/store/games/1001?from={}", href::segment(store));
    assert_eq!(valid(&similar).as_deref(), Some(similar.as_str()));
    assert_eq!(
        href::back_target("1001", Some(&similar), "/store/games").as_deref(),
        Some(store)
    );
    assert_eq!(
        href::back_target("lantern", Some(&similar), "/games").as_deref(),
        Some(similar.as_str())
    );
}

#[test]
fn history_goes_back_and_forward_and_replaces_in_place() {
    let mut history = History::new(Memory::default());
    assert!(history.untouched());
    history.push("/library?filter=all");
    history.push("/games/a");
    assert!(!history.untouched());
    assert!(matches!(history.current().route, Route::Game { .. }));
    // The same page again is no new visit.
    let key = history.current().key.clone();
    history.push("/games/a");
    assert_eq!(history.current().key, key);
    assert!(history.back());
    let library_key = history.current().key.clone();
    assert!(matches!(history.current().route, Route::Library(_)));
    // A search typed in place keeps the visit.
    history.replace("/library?filter=all&q=zel");
    assert_eq!(history.current().key, library_key);
    assert!(history.forward());
    assert!(!history.forward());
    history.back();
    // A new page drops the ones ahead.
    history.push("/store");
    assert!(!history.forward());
    assert!(history.back() && history.back());
    assert_eq!(history.current().route, Route::Home);
    assert!(!history.back());
}

#[test]
fn navigation_remembers_the_library_view_and_the_last_page() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("native-views.json");
    let mut history = History::new(Memory::load(Some(file.clone())));
    history.push("/library?filter=installed&sort=title&layout=list");
    // A search keeps the remembered filter; its sort and layout stay.
    history.push("/library?filter=all&sort=title&layout=list&q=zelda");
    history.push("/collections/weekend");
    let memory = Memory::load(Some(file));
    assert_eq!(
        memory.remembered().library,
        LibraryPrefs {
            filter: GameFilter::Installed,
            sort: GameSort::Title,
            layout: Layout::List,
        }
    );
    assert_eq!(memory.last_view(), Some("/collections/weekend"));
}

#[test]
fn open_to_names_the_first_page() {
    assert_eq!(start_href("home", Some("/library")), None);
    assert_eq!(start_href("library", None).as_deref(), Some("/library"));
    assert_eq!(
        start_href("collections", None).as_deref(),
        Some("/collections")
    );
    assert_eq!(
        start_href("last", Some("/store")).as_deref(),
        Some("/store")
    );
    assert_eq!(start_href("last", Some("/")), None);
    assert_eq!(start_href("last", None), None);
    assert_eq!(start_href("anything", Some("/store")), None);
}
