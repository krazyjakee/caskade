//! What screen readers hear of the real pages (#141): Blitz's AccessKit
//! tree of each page in the frame over the fixture library, headless, as
//! `a11y_snapshot` writes it. Every control a screen reader can land on
//! (a button, link, tab, field, switch, menu item…) has a name, on Home,
//! the library, a game's page, the Store, Collections, Activity, every
//! Settings section and the frame around them. The components' own trees
//! are `ui/tests`; the tree following the UI and its actions reaching the
//! handlers is `tests/accessibility.rs`.

use crate::frame::tests::{ONE, Window};
use crate::router::SETTINGS_TABS;
use blitz_dom::NodeId;

/// AccessKit's roles (as `a11y_snapshot` writes them) for what a screen
/// reader moves to and acts on: each needs a name.
const CONTROLS: &[&str] = &[
    "Button",
    "CheckBox",
    "ComboBox",
    "EditableComboBox",
    "Link",
    "ListBoxOption",
    "MenuItem",
    "MenuItemCheckBox",
    "MenuItemRadio",
    "MultilineTextInput",
    "RadioButton",
    "SearchInput",
    "Slider",
    "SpinButton",
    "Switch",
    "Tab",
    "TextInput",
    "TreeItem",
];

/// The roles AccessKit names from the text inside them when they have no
/// label of their own (`accesskit_consumer`'s `labelled_by`). A tab, a
/// field or a switch needs its own.
const NAMED_BY_CONTENTS: &[&str] = &[
    "Button",
    "CheckBox",
    "DefaultButton",
    "Link",
    "MenuItem",
    "MenuItemCheckBox",
    "MenuItemRadio",
    "RadioButton",
];

/// The lines of `tree` that are controls without a name: no name of their
/// own, and for the roles named by their contents no text inside them
/// either (a quoted line indented under them).
fn unnamed(tree: &str) -> Vec<String> {
    let lines: Vec<&str> = tree.lines().collect();
    let indent = |line: &str| line.len() - line.trim_start().len();
    lines
        .iter()
        .enumerate()
        .filter(|(at, line)| {
            let trimmed = line.trim_start();
            let role = trimmed.split(' ').next().unwrap_or_default();
            if !CONTROLS.contains(&role) || trimmed[role.len()..].starts_with(" \"") {
                return false;
            }
            let has_text = NAMED_BY_CONTENTS.contains(&role)
                && lines[at + 1..]
                    .iter()
                    .take_while(|inner| indent(inner) > indent(line))
                    .any(|inner| inner.trim_start().starts_with('"'));
            !has_text
        })
        .map(|(_, line)| line.trim_start().to_owned())
        .collect()
}

fn heard(window: &Window, node: NodeId) -> String {
    crate::a11y_snapshot::snapshot_of(&window.doc.inner.borrow(), node)
}

/// Adds what `node` (one page, or a part of the frame) has unnamed to
/// `found`, under `place`.
fn check(window: &Window, place: &str, node: NodeId, found: &mut Vec<String>) {
    let tree = heard(window, node);
    assert!(!tree.trim().is_empty(), "{place} has an empty tree");
    for line in unnamed(&tree) {
        found.push(format!("{place}: {line}"));
    }
}

#[test]
fn every_control_on_every_page_has_a_name_for_screen_readers() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    let mut found = Vec::new();
    for (href, view) in [
        ("/", "home"),
        ("/library?filter=all", "library"),
        ("/games/fixture-1", "game"),
        ("/store?tab=browse", "store"),
        ("/collections", "collections"),
        ("/activity?tab=jobs", "activity"),
    ] {
        window.go(href);
        window.settle_until(href, |window| window.find("data-view", view).is_some());
        // What loads after the page (cards, rows) settles too.
        window.settle();
        check(&window, href, window.get("data-cs", "view"), &mut found);
    }
    for tab in SETTINGS_TABS {
        let href = format!("/settings?tab={tab}");
        window.go(&href);
        let panel = format!("settings-tab-{tab}-panel");
        window.settle_until(&href, |window| window.find("id", &panel).is_some());
        window.settle();
        check(&window, &href, window.get("data-cs", "view"), &mut found);
    }
    // The frame around them.
    for part in ["sidebar", "topbar", "status-bar"] {
        check(&window, part, window.get("data-cs", part), &mut found);
    }
    assert!(
        found.is_empty(),
        "controls a screen reader can't name:\n{}",
        found.join("\n")
    );
}

#[test]
fn a_control_without_a_name_is_found() {
    let tree = "Link \"Home\" current=Page [focus click]\n  Button [focus click]\n  Tab \"General\" selected=true\n  \"Some text\"\n";
    assert_eq!(unnamed(tree), ["Button [focus click]"]);
}
