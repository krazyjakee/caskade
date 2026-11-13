//! Settings → General (#104), ported from the webview's
//! `AppUpdateSection` and `generalUpdates` tests: each field reads what's
//! saved, changes by keyboard and pad, and resets; Caskade's updates check,
//! download, install and restart; a link to them scrolls there; every
//! control is named by its row for a screen reader.

use super::{is_inside, label_of, open};
use crate::frame::tests::{ONE, Window, source};
use crate::updater::fake;
use caskade_app::commands::app_update::{AppRelease, AppUpdatePhase, AppUpdateStatus};
use caskade_ui_input::Action;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use serde_json::{Value, json};

/// Every section General changes.
const SECTIONS: [&str; 6] = [
    "core.general",
    "core.console",
    "core.appUpdates",
    "core.metadata",
    "core.ports",
    "core.onboarding",
];

fn saved(namespace: &str) -> Value {
    caskade_app::runtime::block_on(source().settings.get(namespace))
        .unwrap()
        .value
}

fn save(namespace: &str, value: Value) {
    caskade_app::runtime::block_on(source().settings.update(namespace, |_| value)).unwrap();
}

/// Puts back what General's sections held, for the tests after.
struct Restore(Vec<(&'static str, Value)>);

impl Restore {
    fn new() -> Self {
        Self(SECTIONS.iter().map(|ns| (*ns, saved(ns))).collect())
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        for (namespace, value) in self.0.drain(..) {
            save(namespace, value);
        }
    }
}

/// Changes `key` of `namespace` from what's saved.
fn set(namespace: &str, key: &str, value: Value) {
    let mut next = saved(namespace);
    next[key] = value;
    save(namespace, next);
}

/// The setting row labelled `label`.
fn row(window: &Window, label: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-cs", "setting-row")
        .into_iter()
        .find(|&row| label_of(window, row) == label)
        .unwrap_or_else(|| panic!("no row {label}"))
}

/// The element in `root` whose `name` attribute is `value`.
fn within(window: &Window, root: blitz_dom::NodeId, name: &str, value: &str) -> blitz_dom::NodeId {
    window
        .find_all(name, value)
        .into_iter()
        .find(|&node| is_inside(window, node, root))
        .unwrap_or_else(|| panic!("no {name}={value} in the row"))
}

fn control(window: &Window, label: &str, name: &str, value: &str) -> blitz_dom::NodeId {
    within(window, row(window, label), name, value)
}

fn checked(window: &Window, label: &str) -> bool {
    let switch = control(window, label, "role", "switch");
    window.attr(switch, "aria-checked").as_deref() == Some("true")
}

/// The segment pressed in `label`'s row.
fn pressed(window: &Window, label: &str) -> String {
    let root = row(window, label);
    window
        .find_all("data-part", "segment")
        .into_iter()
        .filter(|&segment| is_inside(window, segment, root))
        .find(|&segment| window.attr(segment, "aria-pressed").as_deref() == Some("true"))
        .map(|segment| window.text(segment).trim().to_owned())
        .unwrap_or_default()
}

fn segment(window: &Window, label: &str, text: &str) -> blitz_dom::NodeId {
    let root = row(window, label);
    window
        .find_all("data-part", "segment")
        .into_iter()
        .filter(|&segment| is_inside(window, segment, root))
        .find(|&segment| window.text(segment).trim() == text)
        .unwrap_or_else(|| panic!("no {text} in {label}"))
}

/// What `label`'s select shows.
fn chosen(window: &Window, label: &str) -> String {
    let value = control(window, label, "data-part", "value");
    window.text(value).trim().to_owned()
}

fn general(href: &'static str) -> Window {
    let mut window = open(href);
    window.settle_until("General", |window| {
        window.find("data-settings", "general").is_some()
            && window.find("data-settings", "core.updates").is_some()
            && window
                .find_all("data-cs", "setting-row")
                .iter()
                .any(|&row| label_of(window, row) == "Show ports of games")
    });
    window
}

fn press(window: &mut Window, node: blitz_dom::NodeId, key: Key) {
    window.focus(node);
    window.key(key);
}

#[test]
fn every_field_reads_what_its_section_saved() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    set("core.general", "launchOnLogin", json!(true));
    set("core.general", "startMinimized", json!(true));
    set("core.general", "startView", json!("library"));
    set("core.general", "onLaunch", json!("tray"));
    set("core.general", "onClose", json!("quit"));
    set("core.general", "confirmQuit", json!(false));
    set("core.general", "timeFormat", json!("24h"));
    set("core.console", "start", json!("console"));
    let window = general("/settings?tab=general");
    assert!(checked(&window, "Launch on system startup"));
    assert!(checked(&window, "Start minimized"));
    let minimized = control(&window, "Start minimized", "role", "switch");
    assert_ne!(
        window.attr(minimized, "aria-disabled").as_deref(),
        Some("true")
    );
    assert_eq!(chosen(&window, "Open to"), "Library");
    assert_eq!(chosen(&window, "Start in"), "Console mode");
    assert_eq!(chosen(&window, "When a game launches"), "Hide to tray");
    assert_eq!(pressed(&window, "Close button"), "Quit");
    assert!(!checked(&window, "Confirm before quitting"));
    assert!(chosen(&window, "Language").starts_with("System ("));
    assert_eq!(pressed(&window, "Time format"), "24-hour");
    assert_eq!(
        checked(&window, "Fetch artwork and game info online"),
        saved("core.metadata")["online"] == json!(true)
    );
    assert_eq!(
        checked(&window, "Show ports of games"),
        saved("core.ports")["enabled"] == json!(true)
    );
    // The cache folder, read-only, and the storage summary's way to Storage.
    let cache = saved("core.general")["cacheDir"]
        .as_str()
        .unwrap()
        .to_owned();
    let field = window
        .find_all("readonly", "")
        .into_iter()
        .chain(window.find_all("readonly", "true"))
        .next()
        .expect("the read-only cache folder");
    assert_eq!(window.attr(field, "value").as_deref(), Some(cache.as_str()));
    assert!(window.find("data-part", "storage-summary").is_some());
    let setup = row(&window, "First-run setup");
    assert!(window.text(setup).contains("Run setup again"));
}

/// Start minimized needs Launch on system startup, and says so (#55).
#[test]
fn changes_save_by_keyboard_and_pad() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    set("core.general", "launchOnLogin", json!(false));
    let mut window = general("/settings?tab=general");
    let minimized = control(&window, "Start minimized", "role", "switch");
    assert_eq!(
        window.attr(minimized, "aria-disabled").as_deref(),
        Some("true")
    );
    assert!(
        window
            .text(row(&window, "Start minimized"))
            .contains("Needs Launch on system startup.")
    );
    // A switch by Space.
    let launch = control(&window, "Launch on system startup", "role", "switch");
    press(&mut window, launch, Key::Character(" ".into()));
    window.settle_until("launch on login saved", |window| {
        saved("core.general")["launchOnLogin"] == json!(true)
            && checked(window, "Launch on system startup")
    });
    let minimized = control(&window, "Start minimized", "role", "switch");
    assert_ne!(
        window.attr(minimized, "aria-disabled").as_deref(),
        Some("true")
    );
    // A select by its arrows: it opens on the chosen option.
    let trigger = control(&window, "Open to", "data-part", "trigger");
    press(&mut window, trigger, Key::Named(NamedKey::ArrowDown));
    window.settle_until("the list", |window| {
        window.find("role", "listbox").is_some()
    });
    window.key(Key::Named(NamedKey::ArrowDown));
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("open to saved", |window| {
        saved("core.general")["startView"] == json!("library")
            && chosen(window, "Open to") == "Library"
    });
    assert!(window.find("role", "listbox").is_none());
    // A segment by Enter.
    let quit = segment(&window, "Close button", "Quit");
    press(&mut window, quit, Key::Named(NamedKey::Enter));
    window.settle_until("close button saved", |window| {
        saved("core.general")["onClose"] == json!("quit")
            && pressed(window, "Close button") == "Quit"
    });
    // A pad's A on a segment and on a switch.
    let hours = segment(&window, "Time format", "24-hour");
    window.focus(hours);
    window.pad(Action::Confirm);
    window.settle_until("time format saved", |window| {
        saved("core.general")["timeFormat"] == json!("24h")
            && pressed(window, "Time format") == "24-hour"
    });
    let ports = saved("core.ports")["enabled"].clone();
    let switch = control(&window, "Show ports of games", "role", "switch");
    window.focus(switch);
    window.pad(Action::Confirm);
    window.settle_until("ports saved", |window| {
        saved("core.ports")["enabled"] != ports
            && checked(window, "Show ports of games")
                == (saved("core.ports")["enabled"] == json!(true))
    });
    // The console's own section.
    let start = control(&window, "Start in", "data-part", "trigger");
    press(&mut window, start, Key::Named(NamedKey::ArrowDown));
    window.settle_until("the list", |window| {
        window.find("role", "listbox").is_some()
    });
    window.key(Key::Named(NamedKey::End));
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("start in saved", |window| {
        saved("core.console")["start"] == json!("console")
            && chosen(window, "Start in") == "Console mode"
    });
    // First-run setup again.
    set("core.onboarding", "completed", json!(true));
    window.settle();
    let again = within(
        &window,
        row(&window, "First-run setup"),
        "data-cs",
        "button",
    );
    press(&mut window, again, Key::Named(NamedKey::Enter));
    window.settle_until("setup again", |_| {
        saved("core.onboarding")["completed"] == json!(false)
    });
}

/// One reset covers every preference on the page, from whichever section
/// keeps it (#44), and says what goes back first.
#[test]
fn the_reset_puts_back_every_preference_on_the_page() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    let console = saved("core.console")["start"].clone();
    set("core.general", "onClose", json!("quit"));
    set("core.general", "confirmQuit", json!(false));
    set(
        "core.console",
        "start",
        json!(if console == json!("desktop") {
            "console"
        } else {
            "desktop"
        }),
    );
    set("core.appUpdates", "check", json!(false));
    let mut window = general("/settings?tab=general");
    let reset = window
        .find_all("aria-label", "Reset general settings")
        .into_iter()
        .next()
        .expect("the reset button");
    press(&mut window, reset, Key::Named(NamedKey::Enter));
    window.settle_until("the reset question", |window| {
        window.find("role", "alertdialog").is_some_and(|dialog| {
            let text = window.text(dialog);
            text.contains("Close button")
                && text.contains("Confirm before quitting")
                && text.contains("Start in")
                && text.contains("Check automatically")
        })
    });
    let question = window.text(window.get("role", "alertdialog"));
    assert!(
        question.contains("Game updates, which has its own reset"),
        "{question}"
    );
    let confirm = window
        .find_all("data-variant", "danger")
        .into_iter()
        .find(|&button| window.text(button).trim() == "Reset")
        .expect("Reset");
    press(&mut window, confirm, Key::Named(NamedKey::Enter));
    window.settle_until("reset", |window| {
        window.find("role", "alertdialog").is_none()
            && saved("core.general")["onClose"] == json!("tray")
            && saved("core.general")["confirmQuit"] == json!(true)
            && saved("core.appUpdates")["check"] == json!(true)
            && saved("core.console")["start"] == json!("auto")
    });
}

fn idle() -> AppUpdateStatus {
    AppUpdateStatus {
        current_version: "0.1.0".into(),
        unavailable: None,
        phase: AppUpdatePhase::Idle,
        release: None,
        checked_at: None,
        downloaded: 0,
        total: None,
        error: None,
    }
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
        .text(window.get("data-settings", "general"))
        .contains(text)
}

#[test]
fn checks_installs_with_progress_and_offers_the_restart() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _fake = fake::start(idle());
    let mut window = general("/settings?tab=general");
    window.settle_until("the version", |window| says(window, "Caskade v0.1.0"));
    assert!(says(&window, "Not checked yet."));
    let check = button(&window, "Check now");
    press(&mut window, check, Key::Named(NamedKey::Enter));
    window.settle_until("the release", |window| {
        says(window, "Version 0.2.0 is available.") && says(window, "Faster library scans.")
    });
    let install = button(&window, "Download and install");
    window.focus(install);
    window.pad(Action::Confirm);
    window.settle_until("downloading", |window| {
        says(window, "Downloading version 0.2.0…")
    });
    assert_eq!(fake::calls(), ["check", "install"]);
    let downloading = crate::updater::status();
    fake::emit(AppUpdateStatus {
        downloaded: 25,
        total: Some(100),
        ..downloading.clone()
    });
    window.settle_until("the progress", |window| {
        window
            .find("aria-label", "Update download")
            .is_some_and(|bar| window.attr(bar, "aria-valuenow").as_deref() == Some("25"))
    });
    fake::emit(AppUpdateStatus {
        phase: AppUpdatePhase::Ready,
        ..downloading
    });
    window.settle_until("ready", |window| {
        says(
            window,
            "Version 0.2.0 is installed. Restart Caskade to use it.",
        )
    });
    assert!(window.find("aria-label", "Update download").is_none());
    let restart = button(&window, "Restart now");
    press(&mut window, restart, Key::Named(NamedKey::Enter));
    window.settle_until("restart", |_| {
        fake::calls() == ["check", "install", "restart"]
    });
}

#[test]
fn offers_the_release_again_after_a_failed_download() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _fake = fake::start(AppUpdateStatus {
        phase: AppUpdatePhase::Failed,
        release: Some(AppRelease {
            version: "0.2.0".into(),
            notes: None,
            date: None,
        }),
        error: Some("Couldn't download the update: connection reset".into()),
        ..idle()
    });
    let mut window = general("/settings?tab=general");
    window.settle_until("the error", |window| {
        says(window, "Couldn't download the update: connection reset")
    });
    let again = button(&window, "Try again");
    press(&mut window, again, Key::Named(NamedKey::Enter));
    window.settle_until("install", |_| fake::calls() == ["install"]);
}

/// A copy that can't update itself says why and hides the automatic
/// checks; one that can turns them off.
#[test]
fn says_why_a_copy_cant_update_itself_and_turns_the_checks_off() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    {
        let _fake = fake::start(AppUpdateStatus {
            unavailable: Some("Development builds don't update themselves.".into()),
            ..idle()
        });
        let mut window = general("/settings?tab=general");
        window.settle_until("why", |window| {
            says(window, "Development builds don't update themselves.")
        });
        let check = button(&window, "Check now");
        assert_eq!(window.attr(check, "aria-disabled").as_deref(), Some("true"));
        assert!(!says(&window, "Check automatically"));
    }
    let _fake = fake::start(idle());
    set("core.appUpdates", "check", json!(true));
    let mut window = general("/settings?tab=general");
    window.settle_until("the automatic checks", |window| {
        window
            .find_all("data-cs", "setting-row")
            .iter()
            .any(|&row| label_of(window, row) == "Check automatically")
    });
    let switch = control(&window, "Check automatically", "role", "switch");
    press(&mut window, switch, Key::Character(" ".into()));
    window.settle_until("checks off", |_| {
        saved("core.appUpdates")["check"] == json!(false)
    });
}

/// Links to Caskade updates land on them, not on the top of General (#68).
#[test]
fn a_link_to_caskades_updates_scrolls_to_them() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let top = |window: &Window| {
        let anchor = window.get("data-anchor", "updates");
        window
            .doc
            .inner
            .borrow()
            .get_client_bounding_rect(anchor)
            .unwrap()
            .y
    };
    let mut window = general("/settings?tab=general");
    window.settle_until("the updates", |window| {
        window.find("data-anchor", "updates").is_some()
    });
    let unscrolled = top(&window);
    assert!(unscrolled > 800.0, "below the fold: {unscrolled}");
    drop(window);
    let mut window = general("/settings?tab=general&section=updates");
    window.settle_until("scrolled", |window| {
        window.find("data-anchor", "updates").is_some() && top(window) < 400.0
    });
}

/// A screen reader hears each control by its row's label (and the row's
/// description).
#[test]
fn every_control_is_named_by_its_row() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let window = general("/settings?tab=general");
    let panel = window.get("data-settings", "general");
    let named = |node: blitz_dom::NodeId| -> String {
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
            ("data-part", "trigger"),
            ("data-cs", "segmented"),
        ] {
            for control in window.find_all(name, value) {
                if is_inside(&window, control, row) {
                    let heard = named(control);
                    assert!(heard.starts_with(&label), "{label}: {heard:?}");
                    seen += 1;
                }
            }
        }
    }
    // Launch, minimized, open to, start in, when a game launches, close
    // button, confirm, language, time format, online, ports, and game
    // updates' own.
    assert!(seen >= 11, "{seen} controls");
}

#[test]
fn the_language_choices_keep_one_this_version_lacks() {
    let options = crate::settings::general::language_options("xx");
    assert_eq!(options[0].value, "system");
    assert!(
        options[0].label.starts_with("System ("),
        "{}",
        options[0].label
    );
    assert!(options.iter().any(|option| option.value == "en"));
    let last = options.last().unwrap();
    assert_eq!(last.value, "xx");
    assert!(
        last.label.ends_with("(not available, shown in English)"),
        "{}",
        last.label
    );
    // A language it has isn't listed twice.
    let en = crate::settings::general::language_options("en");
    assert_eq!(en.iter().filter(|option| option.value == "en").count(), 1);
}
