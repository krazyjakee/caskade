//! The Emulated tab's breakdown by platform (#99), headless over the
//! fixture library, as `PlatformGrid.test.tsx` has it.

use crate::frame::tests::{ONE, Window};
use crate::router::Route;

#[test]
fn emulated_games_show_by_platform_and_a_platform_opens_its_games() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    window.go("/library?filter=emulated&sort=title&layout=grid");
    window.settle_until("the platforms", |window| {
        window.find_all("data-cs", "platform-card").len() > 1
    });
    let cards = window.find_all("data-cs", "platform-card");
    // By title, each with its count; a known platform has its own render.
    let titles: Vec<String> = cards
        .iter()
        .map(|&card| {
            let title = window
                .find_in(card, "data-part", "title")
                .expect("its title");
            window.text(title)
        })
        .collect();
    let mut sorted = titles.clone();
    sorted.sort_by_key(|title| title.to_lowercase());
    assert_eq!(titles, sorted);
    let n64 = cards
        .iter()
        .copied()
        .find(|&card| {
            window
                .attr(card, "aria-label")
                .is_some_and(|label| label.starts_with("Nintendo 64, "))
        })
        .expect("Nintendo 64's card");
    assert_eq!(window.attr(n64, "data-art").as_deref(), Some("platform"));
    let count = window.find_in(n64, "data-part", "count").unwrap();
    assert!(
        window.text(count).ends_with(" games"),
        "{}",
        window.text(count)
    );
    let subtitle = window.find_in(n64, "data-part", "subtitle").unwrap();
    assert_eq!(window.text(subtitle), "Nintendo · 64-bit console");
    // No game grid meanwhile.
    assert!(window.find_all("data-cs", "game-card").is_empty());

    // A card opens the filter on its platform: its games, and the way back.
    window.click(n64);
    window.settle_until("Nintendo 64's games", |window| {
        !window.find_all("data-cs", "game-card").is_empty()
    });
    match window.route() {
        Route::Library(library) => assert_eq!(library.platform.as_deref(), Some("Nintendo 64")),
        other => panic!("not the library: {other:?}"),
    }
    assert!(window.find("data-cs", "platform-card").is_none());
    let back = window.get("data-part", "platform-back");
    window.click(back);
    window.settle_until("the platforms again", |window| {
        window.find("data-cs", "platform-card").is_some()
    });
}
