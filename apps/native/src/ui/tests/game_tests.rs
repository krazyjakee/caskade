//! Game components.

use super::*;

#[component]
fn Games() -> Element {
    rsx! {
        InputRoot {
            GameCard {
                id: "card",
                game: card_game("g1", "Hollow Pines"),
                focus_key: "game-g1",
                "data-index": "0",
                "data-focus-default": "true",
                on_open: |_| did("open card"),
                on_focus: |_| did("focus card"),
            }
            GameCard {
                id: "store",
                game: CardGame { stores: Some(2), size_bytes: Some(4_200_000_000), status: GameStatus::Available, ..card_game("g2", "Tidewater") },
                variant: CardVariant::Store,
            }
            GameCard { id: "chosen", game: card_game("g3", "Moss & Iron"), selected: Some(true) }
            GameRow { id: "row", game: card_game("g4", "Glasswing"), on_open: |_| did("open row") }
        }
    }
}

fn games() -> Element {
    rsx! { Games {} }
}

#[test]
fn a_game_card_opens_by_mouse_enter_and_a_pad_and_reports_focus() {
    let _one = one_window();
    let mut window = Window::open(games);
    assert_eq!(window.attr("card", "data-index").as_deref(), Some("0"));
    assert_eq!(
        window.attr("card", "data-focus-key").as_deref(),
        Some("game-g1")
    );
    assert_eq!(window.attr("card", "data-cs").as_deref(), Some("game-card"));
    assert_eq!(
        window.attr("card", "data-variant").as_deref(),
        Some("library")
    );
    // The input layer puts focus on a clicked control after the click.
    window.click("card");
    assert_eq!(take_done(), ["open card", "focus card"]);
    window.key(ENTER);
    assert_eq!(take_done(), ["open card"]);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["open card"]);
    window.click("row");
    assert_eq!(take_done(), ["open row"]);
    window.key(ENTER);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["open row", "open row"]);
}

#[test]
fn game_cards_and_rows_as_a_screen_reader_hears_them() {
    let _one = one_window();
    let window = Window::open(games);
    let card = window.heard("card");
    assert!(
        card.starts_with("Link \"Hollow Pines\" [focus click]"),
        "{card}"
    );
    assert!(card.contains("Image \"Favorite\""), "{card}");
    assert_eq!(
        window.attr("chosen", "data-selected").as_deref(),
        Some("true")
    );
    assert_eq!(
        window.attr("chosen", "aria-pressed").as_deref(),
        Some("true")
    );
    let store = window.heard("store");
    assert!(
        store.contains("Available to Buy") && store.contains("2 stores"),
        "{store}"
    );
    let row = window.heard("row");
    assert!(row.starts_with("Link \"Glasswing\""), "{row}");
}
