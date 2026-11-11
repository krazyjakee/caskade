//! Settings → About (#105), as the webview's `about` tests: the app's
//! version and build, the four credits and the way into Legal, whose
//! sections open in place with their documents; the packages grouped by
//! licence, a group opening on its packages and licence text; Back to
//! About.

use super::open;
use crate::frame::tests::{ONE, Window};
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};

fn press(window: &mut Window, node: blitz_dom::NodeId) {
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

fn link(window: &Window, text: &str) -> blitz_dom::NodeId {
    let anchors: Vec<blitz_dom::NodeId> = window
        .doc
        .inner
        .borrow()
        .query_selector_all("a")
        .map(|nodes| nodes.into_iter().collect())
        .unwrap_or_default();
    anchors
        .into_iter()
        .find(|&node| window.text(node).trim() == text)
        .unwrap_or_else(|| panic!("no link {text}"))
}

#[test]
fn about_shows_the_build_credits_and_opens_legal_in_place() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let mut window = open("/settings?tab=about");
    window.settle_until("About", |window| window.find("data-cs", "about").is_some());
    let version = window.text(window.get("data-part", "version"));
    assert!(version.contains(env!("CARGO_PKG_VERSION")), "{version}");
    assert!(version.contains("Debug build"), "{version}");
    let credits = window.find_all("data-part", "credit");
    assert_eq!(credits.len(), 4);
    assert_eq!(
        window.attr(credits[0], "aria-label").as_deref(),
        Some("Game metadata by IGDB (igdb.com)")
    );
    // Licenses opens in place, at its own address.
    let licenses = link(&window, "Licenses");
    press(&mut window, licenses);
    window.settle_until("Legal", |window| window.find("data-cs", "legal").is_some());
    assert_eq!(window.href(), "/settings?tab=about&section=licenses");
    let document = window.text(window.get("data-part", "document"));
    assert!(
        document.starts_with("Licenses"),
        "{}",
        &document[..80.min(document.len())]
    );
    assert!(document.contains("7-Zip"), "the 7-Zip licences follow");
    let current: Vec<String> = window
        .find_all("data-part", "section-link")
        .into_iter()
        .filter(|&node| window.attr(node, "aria-current").as_deref() == Some("page"))
        .map(|node| window.text(node))
        .collect();
    assert_eq!(current, ["Licenses"]);
    // Terms, then back to About.
    let terms = link(&window, "Terms and conditions");
    press(&mut window, terms);
    window.settle_until("the terms", |window| {
        window.href() == "/settings?tab=about&section=terms"
    });
    assert!(
        window
            .text(window.get("data-part", "document"))
            .starts_with("Terms and conditions")
    );
    let back = window.get("data-part", "back");
    press(&mut window, back);
    window.settle_until("About again", |window| {
        window.find("data-cs", "about").is_some()
    });
    assert_eq!(window.href(), "/settings?tab=about");
}

#[test]
fn the_packages_open_by_licence() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let mut window = open("/settings?tab=about&section=packages");
    window.settle_until("the packages", |window| {
        !window.find_all("data-part", "license-group").is_empty()
    });
    let mit = window
        .find_all("data-part", "license-group")
        .into_iter()
        .find(|&group| window.text(group).trim_start().starts_with("MIT"))
        .expect("an MIT group");
    let toggle = window
        .find_in(mit, "aria-expanded", "false")
        .expect("closed at first");
    press(&mut window, toggle);
    window.settle_until("open", |window| {
        window.find_in(mit, "aria-expanded", "true").is_some()
    });
    let text = window.text(mit);
    assert!(text.contains("Rust crate") || text.contains("JavaScript package"));
    assert!(
        text.contains("Permission is hereby granted"),
        "the MIT licence's text follows"
    );
}
