//! Settings → Appearance (#104), ported from the webview's
//! `AppearanceThemes`, `AppearanceAccent` and `ThemePicker` tests: every
//! installed theme is listed and picked by arrows, one that can't be used
//! says why, a rescan finds new ones; the look, cards and motion read and
//! save; accents are kept readable; custom CSS waits for Save CSS, refuses
//! what can't apply and holds a navigation while unsaved; one reset puts
//! the page back; every control is named.

use super::{is_inside, label_of, open};
use crate::frame::tests::{ONE, Window, source};
use caskade_ui_input::Action;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use serde_json::{Value, json};

fn saved() -> Value {
    caskade_app::runtime::block_on(source().settings.get("core.appearance"))
        .unwrap()
        .value
}

/// Puts the section back for the tests after.
struct Restore(Value);

impl Restore {
    fn new() -> Self {
        Self(saved())
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        let value = self.0.clone();
        caskade_app::runtime::block_on(source().settings.update("core.appearance", move |_| value))
            .unwrap();
    }
}

fn set(key: &str, value: Value) {
    let key = key.to_owned();
    caskade_app::runtime::block_on(source().settings.update("core.appearance", move |mut now| {
        now[key] = value;
        now
    }))
    .unwrap();
}

fn appearance() -> Window {
    let mut window = open("/settings?tab=appearance");
    window.settle_until("Appearance", |window| {
        window.find("data-settings", "appearance").is_some()
            && !window.find_all("data-part", "theme").is_empty()
    });
    window
}

fn row(window: &Window, label: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-cs", "setting-row")
        .into_iter()
        .find(|&row| label_of(window, row) == label)
        .unwrap_or_else(|| panic!("no row {label}"))
}

fn within(
    window: &Window,
    root: blitz_dom::NodeId,
    name: &str,
    value: &str,
) -> Vec<blitz_dom::NodeId> {
    window
        .find_all(name, value)
        .into_iter()
        .filter(|&node| is_inside(window, node, root))
        .collect()
}

fn theme(window: &Window, id: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-theme", id)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no theme {id}"))
}

fn checked_theme(window: &Window) -> Option<String> {
    window
        .find_all("data-part", "theme")
        .into_iter()
        .find(|&option| window.attr(option, "aria-checked").as_deref() == Some("true"))
        .and_then(|option| window.attr(option, "data-theme"))
}

fn press(window: &mut Window, node: blitz_dom::NodeId, key: Key) {
    window.focus(node);
    window.key(key);
}

fn button(window: &Window, text: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| window.text(button).trim() == text)
        .unwrap_or_else(|| panic!("no {text} button"))
}

fn says(window: &Window, text: &str) -> bool {
    window
        .text(window.get("data-settings", "appearance"))
        .contains(text)
}

/// The built-in themes, in the service's order, as the radio group lists
/// them; the chosen one is checked, and arrows pick the next.
#[test]
fn lists_every_theme_and_arrows_pick_the_next() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    let mut window = appearance();
    let group = window
        .find_all("role", "radiogroup")
        .into_iter()
        .find(|&group| window.attr(group, "aria-label").as_deref() == Some("Theme"))
        .expect("the theme group");
    let listed: Vec<String> = within(&window, group, "data-part", "theme")
        .into_iter()
        .filter_map(|option| window.attr(option, "data-theme"))
        .collect();
    let themes = caskade_app::runtime::block_on(source().themes.list()).unwrap();
    let ids: Vec<String> = themes.iter().map(|theme| theme.id.clone()).collect();
    assert_eq!(listed, ids);
    assert_eq!(checked_theme(&window).as_deref(), Some("caskade.default"));
    // Each is named by its name, and says who made it.
    let nexus = theme(&window, "caskade.default");
    let name = window.attr(nexus, "aria-labelledby").unwrap();
    let named = themes
        .iter()
        .find(|theme| theme.id == "caskade.default")
        .unwrap();
    assert_eq!(window.text(window.get("id", &name)).trim(), named.name);
    assert!(window.text(nexus).contains("by Caskade"));
    // Previews load from the theme's own scheme.
    let cobalt = theme(&window, "caskade.cobalt");
    let previews = &themes
        .iter()
        .find(|theme| theme.id == "caskade.cobalt")
        .unwrap()
        .previews;
    assert!(!previews.is_empty(), "Cobalt has previews");
    for preview in previews {
        let url = crate::settings::appearance::preview_url("caskade.cobalt", preview);
        assert!(
            url.starts_with("caskade-theme://localhost/caskade.cobalt/"),
            "{url}"
        );
        assert_eq!(within(&window, cobalt, "src", &url).len(), 1, "{url}");
    }
    // One tab stop: the checked theme.
    assert_eq!(window.attr(nexus, "tabindex").as_deref(), Some("0"));
    assert_eq!(window.attr(cobalt, "tabindex").as_deref(), Some("-1"));
    let next =
        ids[(ids.iter().position(|id| id == "caskade.default").unwrap() + 1) % ids.len()].clone();
    press(&mut window, nexus, Key::Named(NamedKey::ArrowRight));
    window.settle_until("the next theme", |window| {
        saved()["theme"] == json!(next) && checked_theme(window).as_deref() == Some(next.as_str())
    });
    // Space on another picks it too.
    let cobalt = theme(&window, "caskade.cobalt");
    press(&mut window, cobalt, Key::Character(" ".into()));
    window.settle_until("cobalt", |_| saved()["theme"] == json!("caskade.cobalt"));
}

/// A theme added to the themes folder shows after Rescan themes; one that
/// can't be used says why and can't be picked.
#[test]
fn a_rescan_finds_a_new_theme_and_a_broken_one_says_why() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    let folder = caskade_ui_data::fixtures::themes_folder().join("broken");
    std::fs::create_dir_all(&folder).unwrap();
    struct Gone(std::path::PathBuf);
    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
            let _ = caskade_app::runtime::block_on(source().themes.reload());
        }
    }
    let _gone = Gone(folder.clone());
    std::fs::write(
        folder.join("theme.json"),
        r#"{"id": "dev.example.broken", "name": "Broken", "version": "0.1.0", "extends": "caskade.default", "modes": ["dark"]}"#,
    )
    .unwrap();
    std::fs::write(folder.join("theme.css"), ".x { color: red; }\n").unwrap();
    let mut window = appearance();
    assert!(window.find("data-theme", "dev.example.broken").is_none());
    let rescan = button(&window, "Rescan themes");
    press(&mut window, rescan, Key::Named(NamedKey::Enter));
    window.settle_until("the new theme", |window| {
        window.find("data-theme", "dev.example.broken").is_some()
    });
    let broken = theme(&window, "dev.example.broken");
    assert_eq!(
        window.attr(broken, "aria-disabled").as_deref(),
        Some("true")
    );
    let says = window.text(broken);
    assert!(says.contains("Can't be used:"), "{says}");
    assert!(says.contains("theme.css:1"), "{says}");
    assert!(says.contains("Installed"), "{says}");
    // It can't be picked.
    press(&mut window, broken, Key::Named(NamedKey::Enter));
    window.settle();
    assert_ne!(saved()["theme"], json!("dev.example.broken"));
}

/// Density, scale, cards, titles, animations and effects read what's saved
/// and save by keyboard and pad.
#[test]
fn the_look_cards_and_motion_read_and_save() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    set("density", json!("comfortable"));
    set("uiScale", json!(100));
    set("cardSize", json!(176));
    set("showArtTitles", json!(true));
    set("motion", json!("system"));
    set("reduceEffects", json!(false));
    let mut window = appearance();
    let pressed = |window: &Window, label: &str| {
        within(window, row(window, label), "data-part", "segment")
            .into_iter()
            .find(|&segment| window.attr(segment, "aria-pressed").as_deref() == Some("true"))
            .map(|segment| window.text(segment).trim().to_owned())
            .unwrap_or_default()
    };
    let segment = |window: &Window, label: &str, text: &str| {
        within(window, row(window, label), "data-part", "segment")
            .into_iter()
            .find(|&segment| window.text(segment).trim() == text)
            .unwrap()
    };
    let slider =
        |window: &Window, label: &str| within(window, row(window, label), "role", "slider")[0];
    let switch =
        |window: &Window, label: &str| within(window, row(window, label), "role", "switch")[0];
    assert_eq!(pressed(&window, "Density"), "Comfortable");
    assert_eq!(pressed(&window, "Animations"), "System");
    assert_eq!(
        window
            .attr(slider(&window, "Interface scale"), "aria-valuenow")
            .as_deref(),
        Some("100")
    );
    assert_eq!(
        window
            .attr(slider(&window, "Card size"), "aria-valuenow")
            .as_deref(),
        Some("176")
    );
    assert_eq!(
        window
            .attr(switch(&window, "Show titles on artwork"), "aria-checked")
            .as_deref(),
        Some("true")
    );
    // A segment by Enter, another by a pad's A.
    let compact = segment(&window, "Density", "Compact");
    press(&mut window, compact, Key::Named(NamedKey::Enter));
    window.settle_until("compact", |_| saved()["density"] == json!("compact"));
    let reduced = segment(&window, "Animations", "Reduced");
    window.focus(reduced);
    window.pad(Action::Confirm);
    window.settle_until("reduced", |_| saved()["motion"] == json!("reduced"));
    // Sliders save a step at a time.
    let scale = slider(&window, "Interface scale");
    press(&mut window, scale, Key::Named(NamedKey::ArrowRight));
    window.settle_until("scale", |_| saved()["uiScale"] == json!(105));
    let cards = slider(&window, "Card size");
    press(&mut window, cards, Key::Named(NamedKey::ArrowLeft));
    window.settle_until("cards", |_| saved()["cardSize"] == json!(168));
    // Switches by Space.
    let titles = switch(&window, "Show titles on artwork");
    press(&mut window, titles, Key::Character(" ".into()));
    window.settle_until("titles", |_| saved()["showArtTitles"] == json!(false));
    let effects = switch(&window, "Reduce effects");
    press(&mut window, effects, Key::Character(" ".into()));
    window.settle_until("effects", |_| saved()["reduceEffects"] == json!(true));
}

/// A swatch or a typed colour saves, adjusted when it would be hard to
/// see on the theme; Theme accent goes back to the theme's.
#[test]
fn accents_save_kept_readable_and_go_back_to_the_themes() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    set("accent", Value::Null);
    let mut window = appearance();
    let group = window
        .find_all("role", "radiogroup")
        .into_iter()
        .find(|&group| window.attr(group, "aria-label").as_deref() == Some("Accent colour"))
        .expect("the swatches");
    let swatches = within(&window, group, "role", "radio");
    assert_eq!(swatches.len(), 8);
    let green = swatches
        .into_iter()
        .find(|&swatch| window.attr(swatch, "aria-label").as_deref() == Some("Green"))
        .unwrap();
    press(&mut window, green, Key::Named(NamedKey::Enter));
    window.settle_until("green", |window| {
        saved()["accent"].is_string()
            && window.attr(green, "aria-checked").as_deref() == Some("true")
    });
    // A dark blue on the dark theme is lightened, and says so.
    let field = window.get("aria-label", "Custom accent colour");
    window.focus(field);
    window.key(Key::Named(NamedKey::End));
    for _ in 0..8 {
        window.key(Key::Named(NamedKey::Backspace));
    }
    window.type_text("#1a1f4a");
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("adjusted", |window| {
        says(window, "#1a1f4a was lightened to")
            && saved()["accent"]
                .as_str()
                .is_some_and(|accent| accent != "#1a1f4a")
    });
    let theirs = button(&window, "Theme accent");
    press(&mut window, theirs, Key::Named(NamedKey::Enter));
    window.settle_until("the theme's", |_| saved()["accent"].is_null());
}

/// Custom CSS waits for Save CSS, refuses what can't apply, numbers its
/// lines, holds a navigation while unsaved, and switches off without
/// being lost.
#[test]
fn custom_css_saves_refuses_and_switches_off() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    set("customCss", json!(""));
    set("customCssEnabled", json!(true));
    let mut window = appearance();
    let editor = window.get("data-part", "css");
    window.focus(editor);
    window.type_text("@import url(\"https://x.y/a.css\");");
    let save = button(&window, "Save CSS");
    press(&mut window, save, Key::Named(NamedKey::Enter));
    window.settle_until("refused", |window| {
        window.find_all("role", "alert").iter().any(|&alert| {
            window.text(alert).contains("Custom CSS wasn't saved:")
                && window.text(alert).contains("https://x.y/a.css")
        })
    });
    assert_eq!(saved()["customCss"], json!(""));
    assert!(says(&window, "Unsaved changes"));
    // Leaving the section asks first.
    let router = window.router();
    window
        .doc
        .vdom
        .in_runtime(|| router.navigate("/settings?tab=general"));
    window.settle_until("the question", |window| {
        window
            .find("role", "alertdialog")
            .is_some_and(|dialog| window.text(dialog).contains("Custom CSS"))
    });
    let stay = button(&window, "Stay");
    press(&mut window, stay, Key::Named(NamedKey::Enter));
    window.settle_until("stayed", |window| {
        window.find("role", "alertdialog").is_none()
    });
    // Revert, then rules that apply save.
    let revert = button(&window, "Revert");
    press(&mut window, revert, Key::Named(NamedKey::Enter));
    let editor = window.get("data-part", "css");
    window.focus(editor);
    window.type_text("a { }");
    window.key(Key::Named(NamedKey::Enter));
    window.type_text("b { }");
    let save = button(&window, "Save CSS");
    press(&mut window, save, Key::Named(NamedKey::Enter));
    window.settle_until("saved", |_| saved()["customCss"] == json!("a { }\nb { }"));
    assert!(!says(&window, "Unsaved changes"));
    // The gutter numbers each line.
    let gutter = window
        .doc
        .inner
        .borrow()
        .get_node(editor)
        .and_then(|node| node.parent)
        .unwrap();
    assert!(
        window.text(gutter).contains("12"),
        "{}",
        window.text(gutter)
    );
    // Switched off, it's kept.
    let off = within(
        &window,
        row(&window, "Disable custom CSS"),
        "role",
        "switch",
    )[0];
    press(&mut window, off, Key::Character(" ".into()));
    window.settle_until("off", |_| saved()["customCssEnabled"] == json!(false));
    assert_eq!(saved()["customCss"], json!("a { }\nb { }"));
}

/// One reset puts the whole section back, custom CSS included (#44).
#[test]
fn the_reset_puts_the_page_back() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    set("density", json!("compact"));
    set("customCss", json!("a { }"));
    let mut window = appearance();
    let reset = window.get("aria-label", "Reset appearance settings");
    press(&mut window, reset, Key::Named(NamedKey::Enter));
    window.settle_until("the question", |window| {
        window.find("role", "alertdialog").is_some_and(|dialog| {
            let text = window.text(dialog);
            text.contains("Density") && text.contains("Custom CSS")
        })
    });
    let confirm = window
        .find_all("data-variant", "danger")
        .into_iter()
        .find(|&button| window.text(button).trim() == "Reset")
        .unwrap();
    press(&mut window, confirm, Key::Named(NamedKey::Enter));
    window.settle_until("reset", |_| {
        saved()["density"] == json!("comfortable") && saved()["customCss"] == json!("")
    });
}

/// More themes opens the catalogue: Settings → Plugins.
#[test]
fn more_themes_opens_the_catalogue() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = appearance();
    let link = window.get("data-part", "more-themes");
    press(&mut window, link, Key::Named(NamedKey::Enter));
    window.settle_until("plugins", |window| {
        matches!(
            window.route(),
            crate::router::Route::Settings { tab: "plugins", .. }
        )
    });
}

/// A screen reader hears each control by its row's label.
#[test]
fn every_control_is_named() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let window = appearance();
    let panel = window.get("data-settings", "appearance");
    let heard = |node: blitz_dom::NodeId| -> String {
        if let Some(label) = window.attr(node, "aria-label") {
            return label;
        }
        window
            .attr(node, "aria-labelledby")
            .unwrap_or_default()
            .split_whitespace()
            .map(|id| window.text(window.get("id", id)).trim().to_owned())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut seen = 0;
    for row in window.find_all("data-cs", "setting-row") {
        if !is_inside(&window, row, panel) {
            continue;
        }
        let label = label_of(&window, row);
        for (name, value) in [
            ("role", "switch"),
            ("role", "slider"),
            ("data-cs", "segmented"),
        ] {
            for control in within(&window, row, name, value) {
                let named = heard(control);
                assert!(named.starts_with(&label), "{label}: {named:?}");
                seen += 1;
            }
        }
    }
    // Density, scale, cards, titles, animations, effects, disable CSS.
    assert!(seen >= 7, "{seen}");
    assert_eq!(
        window
            .attr(window.get("data-part", "css"), "aria-label")
            .as_deref(),
        Some("Custom CSS")
    );
}
