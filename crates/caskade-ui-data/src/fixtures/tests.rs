use super::*;
use crate::{Changed, Key, PAGE_LIMIT};
use caskade_core::{
    error::ErrorCode,
    model::{GameFilter, GameSort, ListGames},
};
use std::time::Duration;

/// The fixture library, game by game (#139): what the webview's
/// `fixtures.ts` showed, so fixtures mode keeps the library its screenshots,
/// tests and docs were made on.
const GAMES_JSON: &str = include_str!("games.json");

fn all(limit: u32) -> ListGames {
    ListGames {
        filter: GameFilter::All,
        sort: GameSort::Title,
        limit,
        ..ListGames::default()
    }
}

/// The games follow `games.json`, record by record, so the fixture
/// library doesn't drift unnoticed.
#[test]
fn the_games_are_the_maintained_fixtures() {
    let want: Vec<serde_json::Value> = serde_json::from_str(GAMES_JSON).unwrap();
    let games = library_games(DEFAULT_COUNT);
    assert_eq!(games.len(), want.len());
    for (game, want) in games.iter().zip(&want) {
        let got = serde_json::json!({
            "id": game.id,
            "entryId": game.entry_id,
            "title": game.title,
            "genre": game.genre,
            "platform": game.platform,
            "installed": game.installed,
            "emulated": game.emulated,
            "favorite": game.favorite,
            "playtimeSeconds": game.playtime_ms / 1000,
            "lastPlayedDaysAgo": game.last_played_days_ago,
            "releaseDate": game.release_date,
        });
        assert_eq!(&got, want);
        assert_eq!(game.playtime_ms % 1000, 0, "{}", game.id);
    }
    for (platform, ..) in EMULATED {
        assert!(
            want.iter().any(|game| game["platform"] == platform),
            "games.json lacks {platform}"
        );
    }
}

/// The fixture library answers as `games.json` has it, record by record:
/// every game, with its title, genre, platform, favourite, installed state,
/// play time and last play.
#[tokio::test(flavor = "multi_thread")]
async fn fixtures_mode_has_the_fixture_games() {
    let fixtures = open(DEFAULT_COUNT).await.unwrap();
    let expected = library_games(DEFAULT_COUNT);
    assert_eq!(expected.len(), 24 + 29);
    let page = fixtures.library.page(&all(PAGE_LIMIT)).await.unwrap();
    assert_eq!(page.total, expected.len() as u64);
    for want in &expected {
        let got = page
            .items
            .iter()
            .find(|game| game.id == want.id)
            .unwrap_or_else(|| panic!("{} missing", want.id));
        assert_eq!(got.title, want.title);
        assert_eq!(got.genres, vec![want.genre.to_owned()]);
        assert_eq!(got.platforms, vec![want.platform.clone()]);
        assert_eq!(got.favorite, want.favorite, "{}", want.id);
        assert_eq!(got.installed, want.installed, "{}", want.id);
        assert_eq!(got.playtime_ms, want.playtime_ms, "{}", want.id);
        assert_eq!(
            got.last_played_at_ms.is_some(),
            want.last_played_days_ago.is_some(),
            "{}",
            want.id
        );
    }
    // The filters are the core's.
    let favorites = fixtures
        .library
        .page(&ListGames {
            filter: GameFilter::Favorites,
            ..all(PAGE_LIMIT)
        })
        .await
        .unwrap();
    assert_eq!(favorites.total, 8, "fixture-0, -3, … -21");
    let emulated = fixtures
        .library
        .platforms(GameFilter::Emulated)
        .await
        .unwrap();
    assert_eq!(emulated.len(), EMULATED.len());
    let search = fixtures.library.search("Nintendo").await.unwrap();
    assert!(search.items.len() <= crate::SEARCH_LIMIT as usize);
}

/// A page never holds more than 100 summaries, whatever a view asks.
#[tokio::test(flavor = "multi_thread")]
async fn pages_are_bounded() {
    let fixtures = open(150).await.unwrap();
    let page = fixtures.library.page(&all(10_000)).await.unwrap();
    assert_eq!(page.items.len(), PAGE_LIMIT as usize);
    assert_eq!(page.total, 150 + 29);
}

/// A core write reaches a subscribed view: the event names the game that
/// changed, and the view's next read shows the change.
#[tokio::test(flavor = "multi_thread")]
async fn a_core_write_updates_a_subscribed_view() {
    let fixtures = open(DEFAULT_COUNT).await.unwrap();
    let mut events = fixtures.events.subscribe();
    let watched = Key::Game("fixture-1".into());
    let before = fixtures.library.game("fixture-1").await.unwrap().unwrap();
    assert!(!before.game.favorite);

    fixtures
        .library
        .set_favorite("fixture-1", true)
        .await
        .unwrap();

    let changed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let changed = Changed::from_event(&events.recv().await.unwrap());
            if changed.affects(&watched) {
                return changed;
            }
        }
    })
    .await
    .expect("the change reaches the view");
    assert!(changed.affects(&Key::Games), "pages refetch too");
    assert!(
        !changed.affects(&Key::Game("fixture-2".into())),
        "another game's view doesn't"
    );
    let after = fixtures.library.game("fixture-1").await.unwrap().unwrap();
    assert!(after.game.favorite);
}

/// A failure keeps the core's report: a code to match, hints to label.
#[tokio::test(flavor = "multi_thread")]
async fn failures_keep_the_cores_report() {
    let fixtures = open(1).await.unwrap();
    let error = fixtures
        .library
        .set_favorite("no-such-game", true)
        .await
        .unwrap_err();
    assert_ne!(error.code, ErrorCode::Other, "{error:?}");
    assert!(!error.summary.is_empty());
    assert!(!error.reference.is_empty());
    for hint in &error.hints {
        assert!(!crate::labels::hint(*hint).is_empty());
    }
}
