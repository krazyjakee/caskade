//! Settings → Plugins' catalogue (#104), ported from the webview's
//! `catalogue` and `DeviceOffers` tests over the fixtures' catalogue
//! service, given the add-ons crate's sample catalogue. Its address is one
//! nothing answers, so refreshes and downloads fail at once and the tests
//! never reach the internet.

use super::{is_inside, open};
use crate::frame::tests::{ONE, Window, source};
use crate::settings::addons::permission_lines;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};

const SAMPLE: &str =
    include_str!("../../../../../crates/caskade-addons/src/testdata/catalogue.json");

fn seed() {
    let service = source().addons.service().expect("the fixtures' catalogue");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    caskade_app::runtime::block_on(service.accept(SAMPLE.as_bytes(), None, now)).unwrap();
}

fn catalogue() -> Window {
    seed();
    let mut window = open("/settings?tab=plugins");
    window.settle_until("the catalogue", |window| {
        window.find("data-cs", "addon-catalogue").is_some()
            && window.find_all("data-part", "item").len() >= 2
    });
    window
}

fn item(window: &Window, name: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-part", "item")
        .into_iter()
        .find(|&item| window.text(item).contains(name))
        .unwrap_or_else(|| panic!("no add-on {name}"))
}

fn press(window: &mut Window, node: blitz_dom::NodeId) {
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

#[test]
fn permission_words_read_as_the_reviews() {
    assert_eq!(
        permission_lines(&[
            "network:example.org".into(),
            "library.read".into(),
            "fs:$HOME/ROMs:ro".into(),
            "something.new".into(),
        ]),
        [
            "Connect to example.org",
            "Read files in $HOME/ROMs",
            "Read your library",
            "something.new",
        ]
    );
}

/// The catalogue lists what fits, says when it was fetched, and filters by
/// words and kind.
#[test]
fn lists_searches_and_filters_the_catalogue() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = catalogue();
    let status = window.get("data-part", "status");
    assert!(
        window.text(status).contains("Updated"),
        "{}",
        window.text(status)
    );
    let stats = item(&window, "Playtime stats");
    assert_eq!(window.attr(stats, "data-kind").as_deref(), Some("plugin"));
    assert!(window.text(stats).contains("Charts of your play time."));
    assert!(window.text(stats).contains("Plugin · Caskade"));
    // By kind: themes only.
    let themes = window
        .find_all("data-part", "segment")
        .into_iter()
        .find(|&segment| window.text(segment).trim() == "Themes")
        .unwrap();
    press(&mut window, themes);
    window.settle_until("themes", |window| {
        window
            .find_all("data-part", "item")
            .iter()
            .all(|&item| window.attr(item, "data-kind").as_deref() == Some("theme"))
    });
    // By words: nothing matches.
    let all = window
        .find_all("data-part", "segment")
        .into_iter()
        .find(|&segment| window.text(segment).trim() == "All")
        .unwrap();
    press(&mut window, all);
    let search = window.get("aria-label", "Search the catalogue");
    window.focus(search);
    window.type_text("zzz");
    window.settle_until("no match", |window| {
        window
            .text(window.get("data-cs", "addon-catalogue"))
            .contains("Nothing in the catalogue matches.")
    });
}

/// Details say what an add-on is and asks for; an install that can't
/// download says to try again later and is offered again.
#[test]
fn details_install_and_a_failed_download_says_so() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = catalogue();
    let details = window.get("aria-label", "Details of Playtime stats");
    press(&mut window, details);
    window.settle_until("the details", |window| {
        window.find("data-cs", "addon-details").is_some()
    });
    let dialog = window.get("data-cs", "addon-details");
    let description = window
        .find_all("data-part", "description")
        .into_iter()
        .find(|&part| is_inside(&window, part, dialog))
        .expect("the description");
    // Text, never markup.
    let words = window.text(description);
    assert!(
        words.contains("A page of charts.") && words.contains("stays text."),
        "{words}"
    );
    let release = window.get("data-part", "release");
    let text = window.text(release);
    assert!(text.contains("Read your library"), "{text}");
    assert!(text.contains("Desktop mode"), "{text}");
    let install = window
        .find_all("data-part", "install")
        .into_iter()
        .find(|&button| is_inside(&window, button, dialog))
        .expect("Install");
    press(&mut window, install);
    window.settle_until("the failure", |window| {
        window
            .find("data-outcome", "failed")
            .is_some_and(|outcome| window.text(outcome).contains("It didn't install"))
    });
    let close = window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| is_inside(&window, button, dialog) && window.text(button).trim() == "Close")
        .unwrap();
    press(&mut window, close);
    window.settle_until("closed", |window| {
        window.find("data-cs", "addon-details").is_none()
    });
}

/// The update check's preference saves, and a check that can't reach the
/// catalogue keeps the last one and says why.
#[test]
fn the_update_preference_saves_and_a_failed_check_keeps_the_last() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let addons = source().addons.clone();
    let before = caskade_app::runtime::block_on(addons.records())
        .unwrap()
        .updates;
    let mut window = catalogue();
    let switch = window.get("aria-label", "Check for add-on updates when Settings opens");
    window.focus(switch);
    window.key(Key::Character(" ".into()));
    window.settle_until("saved", |_| {
        caskade_app::runtime::block_on(addons.records())
            .unwrap()
            .updates
            != before
    });
    let restore = before;
    caskade_app::runtime::block_on(addons.set_update_preference(restore)).unwrap();
    let refresh = window.get("data-part", "refresh");
    press(&mut window, refresh);
    window.settle_until("the failure", |window| {
        let status = window.text(window.get("data-part", "status"));
        status.contains("the last check failed")
            || status.contains("Offline: showing the catalogue from")
    });
    assert!(
        window.find_all("data-part", "item").len() >= 2,
        "the last catalogue stays"
    );
}
