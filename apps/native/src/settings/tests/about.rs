//! Settings → About (#105): the app's version and build, the four credits
//! and the way into Legal, whose sections open in place with their
//! documents; the packages grouped by licence, a group opening on its
//! packages and licence text; Back to
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
    let _one = caskade_core::lock(&ONE);
    let mut window = open("/settings?tab=about");
    window.settle_until("About", |window| window.find("data-cs", "about").is_some());
    let version = window.text(window.get("data-part", "version"));
    assert!(version.contains(env!("CARGO_PKG_VERSION")), "{version}");
    assert!(version.contains("Debug build"), "{version}");
    assert!(
        version.contains("GPU (") || version.contains("CPU ("),
        "renderer shown in build details: {version}"
    );
    if let Some(sha) = option_env!("CASKADE_GIT_SHA").filter(|sha| !sha.is_empty()) {
        assert!(version.contains(sha), "git SHA shown: {version}");
    }
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
    let _one = caskade_core::lock(&ONE);
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
    assert!(text.contains("Rust crate"));
    assert!(
        text.contains("Permission is hereby granted"),
        "the MIT licence's text follows"
    );
}

/// A pad alone (#153): into About from its tab, along to Licenses, A opens
/// Legal in place, its way back returns to About, and B goes back to Home.
#[test]
fn a_pad_alone_opens_legal_comes_back_and_goes_back() {
    use crate::frame::pad_tests::{back_home, enter_panel, reach, settings_by_pad};
    use caskade_ui_input::Action;
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let (mut window, panel) = settings_by_pad(
        crate::frame::tests::source().clone(),
        || {},
        "about",
        "About",
        |window| window.find("data-cs", "about").is_some(),
    );
    reach(&mut window, panel, "Licenses", |window, node| {
        window.attr(node, "href").is_some() && window.text(node).trim() == "Licenses"
    });
    window.pad(Action::Confirm);
    window.settle_until("Legal", |window| window.find("data-cs", "legal").is_some());
    assert_eq!(window.href(), "/settings?tab=about&section=licenses");
    let panel = enter_panel(&mut window);
    reach(&mut window, panel, "Legal's way back", |window, node| {
        window.attr(node, "data-part").as_deref() == Some("back")
    });
    window.pad(Action::Confirm);
    window.settle_until("About again", |window| {
        window.find("data-cs", "about").is_some()
    });
    assert_eq!(window.href(), "/settings?tab=about");
    back_home(&mut window);
}

#[test]
fn about_diagnostics_copies_system_info_and_hides_logs_folder_in_console_mode() {
    use caskade_ui_input::Action;

    struct ResetDesktop;
    impl Drop for ResetDesktop {
        fn drop(&mut self) {
            let _ = caskade_app::runtime::block_on(
                crate::frame::tests::source()
                    .settings
                    .remember_console(false),
            );
        }
    }

    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _desktop = ResetDesktop;
    let mut window = open("/settings?tab=about");
    window.settle_until("About", |window| window.find("data-cs", "about").is_some());

    // Desktop mode shows both Open logs folder and Copy system info.
    let open_logs = window
        .find("id", "about-open-logs")
        .expect("Open logs folder button in desktop mode");
    assert_eq!(window.text(open_logs).trim(), "Open logs folder");

    let copy = window
        .find("id", "about-copy-system-info")
        .expect("Copy system info button");
    assert_eq!(window.text(copy).trim(), "Copy system info");

    // Pressing Copy system info gives inline "Copied" feedback and a toast.
    press(&mut window, copy);
    window.settle_until("copied feedback", |window| {
        window
            .find("id", "about-copy-system-info")
            .is_some_and(|node| window.text(node).trim() == "Copied")
    });
    assert!(
        window
            .find_all("data-part", "toast")
            .into_iter()
            .any(|toast| window.text(toast).contains("System info copied.")),
        "copy toast shown"
    );

    // Console mode hides Open logs folder while keeping Copy system info.
    crate::console::request_console();
    window.settle_until("console mode hides logs button", |window| {
        window.find("data-mode", "console").is_some()
            && window.find("id", "about-open-logs").is_none()
    });
    assert!(
        window.find("id", "about-copy-system-info").is_some(),
        "Copy system info stays available in console mode"
    );

    // Returning to desktop mode brings Open logs folder back.
    window.pad(Action::View);
    window.settle_until("desktop mode restores logs button", |window| {
        window.find("data-mode", "desktop").is_some()
            && window.find("id", "about-open-logs").is_some()
    });
}
