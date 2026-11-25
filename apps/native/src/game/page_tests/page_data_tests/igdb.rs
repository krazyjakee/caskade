//! Playtime sources, IGDB facts/matching, videos, age ratings and online
//! lookups on the game page.

use super::super::{ONE, open, text};
use super::{PageScript, page, page_script};

#[test]
fn play_time_reported_by_stores_says_where_it_comes_from() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_core::db::{PlaytimeBreakdown, ReportedPlaytime};
    let _script = page_script(PageScript {
        playtime: Some(PlaytimeBreakdown {
            total_ms: 3 * 3_600_000,
            tracked_ms: 3_600_000,
            reported_ms: 2 * 3_600_000,
            last_played_ms: None,
            sources: vec![ReportedPlaytime {
                provider: "steam".to_owned(),
                account_name: Some("me".to_owned()),
                total_ms: Some(2 * 3_600_000),
                counted_ms: 2 * 3_600_000,
                last_played_ms: None,
                observed_at_ms: 1_700_000_000_000,
            }],
        }),
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("the sources", |window| {
        window.find("data-part", "playtime-sources").is_some()
    });
    let said = text(&window, "playtime-sources");
    assert!(
        said.contains("in Caskade") && said.contains("more reported by Steam (me)"),
        "{said}"
    );
}

fn matched(game_id: &str) -> caskade_core::metadata::GameIgdb {
    caskade_core::metadata::GameIgdb {
        game_id: game_id.to_owned(),
        igdb_id: Some(1942),
        source: caskade_core::metadata::MatchSource::Store,
        pending: false,
        game: Some(caskade_core::igdb::IgdbGame {
            id: 1942,
            title: "Tidewater".to_owned(),
            release_date: Some("2024-02-29".to_owned()),
            total_rating: Some(86.4),
            genres: vec!["Adventure".to_owned()],
            videos: vec![caskade_core::igdb::IgdbVideo {
                video_id: "dQw4w9WgXcQ".to_owned(),
                name: Some("Trailer".to_owned()),
            }],
            ..caskade_core::igdb::IgdbGame::default()
        }),
    }
}

#[test]
fn a_matched_game_shows_igdbs_facts_and_changes_its_match() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = page_script(PageScript {
        igdb: Some(matched("fixture-1")),
        igdb_results: vec![caskade_core::igdb::IgdbGame {
            id: 77,
            title: "Tidewater Remastered".to_owned(),
            ..caskade_core::igdb::IgdbGame::default()
        }],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("the facts", |window| {
        window.find("data-part", "igdb-facts").is_some()
    });
    let facts = text(&window, "igdb-facts");
    assert!(
        facts.contains("Tidewater") && facts.contains("86 / 100"),
        "{facts}"
    );
    let source = text(&window, "igdb-source");
    assert!(
        source.contains("Matched by store id") && source.contains("IGDB ID 1942"),
        "{source}"
    );
    // Not on IGDB.
    let none = window.get("data-part", "igdb-none");
    window.click(none);
    window.settle_until("the change", |_| {
        page(|script| script.igdb_changes.len()) == 1
    });
    assert_eq!(
        page(|script| script.igdb_changes[0].clone()),
        crate::game::data::IgdbChange::Set(None)
    );
    // Change IGDB game: searches the title, then uses a result.
    let change = window.get("data-part", "igdb-change");
    window.click(change);
    window.settle_until("the results", |window| {
        window.find("data-part", "igdb-result").is_some()
    });
    assert_eq!(
        page(|script| script.searches.clone()),
        ["Fixture Game 0002"]
    );
    let result = window.get("data-part", "igdb-result");
    let use_it = window.find_in(result, "data-part", "use").expect("Use");
    window.click(use_it);
    window.settle_until("the new match", |_| {
        page(|script| script.igdb_changes.len()) == 2
    });
    assert_eq!(
        page(|script| script.igdb_changes[1].clone()),
        crate::game::data::IgdbChange::Set(Some("77".to_owned()))
    );
}

#[test]
fn igdbs_videos_play_in_the_video_window_and_the_age_rating_shows() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = page_script(PageScript {
        igdb: Some(matched("fixture-1")),
        ratings: vec![
            caskade_core::igdb::IgdbAgeRating {
                board: "esrb".to_owned(),
                rating: "T".to_owned(),
            },
            caskade_core::igdb::IgdbAgeRating {
                board: "pegi".to_owned(),
                rating: "12".to_owned(),
            },
        ],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("the rating", |window| {
        text(window, "meta").contains("PEGI 12")
    });
    window.settle_until("the videos", |window| {
        window.find("data-cs", "videos").is_some()
    });
    let video = window.get("data-part", "video");
    assert!(window.text(video).contains("Trailer"));
    window.click(video);
    window.settle_until("the video", |_| {
        page(|script| script.videos_played.len()) == 1
    });
    assert_eq!(
        page(|script| script.videos_played[0].clone()),
        (
            "dQw4w9WgXcQ".to_owned(),
            "Fixture Game 0002 · Trailer".to_owned()
        )
    );
    // No screenshots in the fixtures: the empty grid says so.
    assert!(window.find("data-part", "screenshots-empty").is_some());
}

#[test]
fn with_online_lookups_off_the_igdb_panel_says_so_and_turns_them_on() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let settings = &crate::frame::tests::source().settings;
    let before = caskade_app::runtime::block_on(settings.get("core.metadata"))
        .unwrap()
        .value;
    let off = {
        let mut value = before.clone();
        value["online"] = serde_json::Value::Bool(false);
        value
    };
    caskade_app::runtime::block_on(settings.update("core.metadata", |_| off.clone())).unwrap();
    let _script = page_script(PageScript::default());
    let mut window = open("/games/fixture-1");
    window.settle_until("the note", |window| {
        window.find("data-part", "igdb-lookups-off").is_some()
    });
    let note = window.get("data-part", "igdb-lookups-off");
    assert!(
        window
            .text(note)
            .contains("Online lookups are off on this device"),
        "{}",
        window.text(note)
    );
    let turn_on = window
        .find_all("data-spatial", "focusable")
        .into_iter()
        .chain(window.find_all("type", "button"))
        .find(|&node| window.text(node).contains("Turn on online lookups"))
        .expect("Turn on online lookups");
    window.click(turn_on);
    window.settle_until("on again", |window| {
        window.find("data-part", "igdb-lookups-off").is_none()
    });
    let now = caskade_app::runtime::block_on(settings.get("core.metadata"))
        .unwrap()
        .value;
    assert_eq!(now["online"], serde_json::Value::Bool(true));
    caskade_app::runtime::block_on(settings.update("core.metadata", |_| before.clone())).unwrap();
}
