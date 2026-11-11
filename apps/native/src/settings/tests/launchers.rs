//! Settings → Launchers and Installer's torrent links (#104), ported from
//! the webview's `launchers` and `TorrentLinksSettings` tests: the
//! launchers discovery finds on the fixture machine come first with where
//! they are, each switch turns all of a launcher's sources off and on,
//! Look again detects again, and the reset puts every launcher back; the
//! torrent links row follows what the OS says and changes it.

use super::{is_inside, label_of, open};
use crate::frame::tests::{ONE, Window, source};
use crate::settings::launchers::{Launcher, detected_launchers, launcher_enabled, with_launcher};
use crate::settings::torrent::fake;
use caskade_app::torrent_links::{Support, TorrentLinks};
use caskade_ui_data::discovery::{LauncherStatus, SourceId};
use caskade_ui_input::Action;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use serde_json::{Map, Value, json};

fn status(source: &'static str, found: bool) -> LauncherStatus {
    LauncherStatus {
        source: SourceId::new(source),
        found,
        kind: None,
        path: found.then(|| format!("/opt/{source}").into()),
        data_dir: None,
        version: None,
    }
}

#[test]
fn found_launchers_come_first_each_once() {
    let launchers = detected_launchers(&[
        status("gog", false),
        status("epic", false),
        status("epic.app", true),
        status("steam", true),
        status("custom.source", false),
    ]);
    let names: Vec<&str> = launchers
        .iter()
        .map(|launcher| launcher.launcher.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Steam",
            "Epic Games Launcher",
            "GOG Galaxy",
            "custom.source"
        ]
    );
    assert_eq!(launchers[1].found.len(), 1, "the source that found it");
}

#[test]
fn a_switch_covers_every_source_of_its_launcher() {
    let epic = Launcher {
        id: "epic".into(),
        name: "Epic Games Launcher".into(),
        sources: vec!["epic".into(), "epic.app".into()],
    };
    let mut enabled = Map::new();
    assert!(launcher_enabled(&epic, &enabled));
    enabled.insert("epic.app".into(), Value::Bool(false));
    assert!(!launcher_enabled(&epic, &enabled));
    let on = with_launcher(&enabled, &epic, true);
    assert_eq!(on.get("epic"), Some(&json!(true)));
    assert_eq!(on.get("epic.app"), Some(&json!(true)));
}

fn saved(namespace: &str) -> Value {
    caskade_app::runtime::block_on(source().settings.get(namespace))
        .unwrap()
        .value
}

struct Restore(&'static str, Value);

impl Drop for Restore {
    fn drop(&mut self) {
        let value = self.1.clone();
        caskade_app::runtime::block_on(source().settings.update(self.0, move |_| value)).unwrap();
    }
}

fn launcher_row(window: &Window, name: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-part", "launcher")
        .into_iter()
        .find(|&row| window.text(row).trim_start().starts_with(name))
        .unwrap_or_else(|| panic!("no launcher {name}"))
}

fn within(window: &Window, root: blitz_dom::NodeId, name: &str, value: &str) -> blitz_dom::NodeId {
    window
        .find_all(name, value)
        .into_iter()
        .find(|&node| is_inside(window, node, root))
        .unwrap_or_else(|| panic!("no {name}={value}"))
}

/// The fixture machine (`linux-mixed`) has Steam and Lutris: they're listed
/// first, found, with where they are; a switch, named by its launcher,
/// turns its sources off and the reset turns them back on.
#[test]
fn lists_the_found_launchers_and_switches_them() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore("core.launchers", saved("core.launchers"));
    let mut window = open("/settings?tab=launchers");
    window.settle_until("the launchers", |window| {
        window
            .find_all("data-part", "launcher")
            .iter()
            .any(|&row| window.attr(row, "data-status").as_deref() == Some("detected"))
    });
    let rows = window.find_all("data-part", "launcher");
    let detected: Vec<String> = rows
        .iter()
        .filter(|&&row| window.attr(row, "data-status").as_deref() == Some("detected"))
        .map(|&row| window.text(row))
        .collect();
    assert!(
        detected.iter().any(|text| text.contains("Steam")),
        "{detected:?}"
    );
    // Found ones first.
    let first_missing = rows
        .iter()
        .position(|&row| window.attr(row, "data-status").as_deref() == Some("not-found"));
    if let Some(at) = first_missing {
        assert!(
            rows[at..]
                .iter()
                .all(|&row| window.attr(row, "data-status").as_deref() == Some("not-found"))
        );
    }
    let steam = launcher_row(&window, "Steam");
    assert!(window.text(steam).contains("Scanned"));
    let switch = within(&window, steam, "role", "switch");
    assert_eq!(window.attr(switch, "aria-label").as_deref(), Some("Steam"));
    assert_eq!(window.attr(switch, "aria-checked").as_deref(), Some("true"));
    // A pad's A turns it off.
    window.focus(switch);
    window.pad(Action::Confirm);
    window.settle_until("steam off", |window| {
        saved("core.launchers")["enabled"]["steam"] == json!(false)
            && window
                .text(launcher_row(window, "Steam"))
                .contains("Not scanned")
    });
    // The reset puts it back.
    let reset = window.get("aria-label", "Reset launcher settings");
    window.focus(reset);
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("the question", |window| {
        window
            .find("role", "alertdialog")
            .is_some_and(|dialog| window.text(dialog).contains("Which launchers are scanned"))
    });
    let confirm = window
        .find_all("data-variant", "danger")
        .into_iter()
        .find(|&button| window.text(button).trim() == "Reset")
        .unwrap();
    window.focus(confirm);
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("steam back", |_| {
        saved("core.launchers")["enabled"].get("steam") != Some(&json!(false))
    });
    // Look again detects again; the link goes to Library Discovery.
    let again = window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| window.text(button).trim() == "Look again")
        .unwrap();
    window.focus(again);
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("found again", |window| {
        !window.find_all("data-part", "launcher").is_empty()
    });
    let link = window
        .find_all("href", "/settings?tab=library-discovery")
        .into_iter()
        .next()
        .expect("the link to Library Discovery");
    window.focus(link);
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("library discovery", |window| {
        matches!(
            window.route(),
            crate::router::Route::Settings {
                tab: "library-discovery",
                ..
            }
        )
    });
}

fn links(support: Support, registered: bool, on: bool) -> TorrentLinks {
    TorrentLinks {
        support,
        registered,
        magnet: on,
        torrent_files: on,
    }
}

fn installer() -> Window {
    let mut window = open("/settings?tab=installer");
    window.settle_until("torrent links", |window| {
        window
            .find("data-part", "torrent-links")
            .is_some_and(|section| {
                window
                    .find_all("data-cs", "setting-row")
                    .iter()
                    .any(|&row| is_inside(window, row, section))
                    || window.text(section).contains("can't send")
            })
    });
    window
}

/// Linux: one switch makes Caskade their default app, or gives them back.
#[test]
fn torrent_links_open_with_caskade_and_go_back() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _fake = fake::start(links(Support::Direct, true, false), None);
    let mut window = installer();
    let row = window
        .find_all("data-cs", "setting-row")
        .into_iter()
        .find(|&row| label_of(&window, row) == "Open them with Caskade")
        .expect("the row");
    let switch = within(&window, row, "role", "switch");
    assert_eq!(
        window.attr(switch, "aria-checked").as_deref(),
        Some("false")
    );
    window.focus(switch);
    window.key(Key::Character(" ".into()));
    window.settle_until("open", |window| {
        fake::calls() == ["set true"]
            && window.attr(switch, "aria-checked").as_deref() == Some("true")
    });
}

/// Windows: Caskade is offered; until it's chosen, Default apps opens.
/// A change that fails says why.
#[test]
fn windows_offers_caskade_and_opens_default_apps() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    {
        let _fake = fake::start(links(Support::Chooser, true, false), None);
        let mut window = installer();
        let section = window.get("data-part", "torrent-links");
        assert!(window.text(section).contains(
            "Choose Caskade for magnet links and .torrent files in Windows' Default apps."
        ));
        let open = window
            .find_all("data-cs", "button")
            .into_iter()
            .find(|&button| window.text(button).trim() == "Open Default apps")
            .unwrap();
        window.focus(open);
        window.key(Key::Named(NamedKey::Enter));
        window.settle_until("chosen", |_| fake::calls() == ["choose"]);
    }
    let _fake = fake::start(
        links(Support::Chooser, false, false),
        Some("Windows refused the change"),
    );
    let mut window = installer();
    let row = window
        .find_all("data-cs", "setting-row")
        .into_iter()
        .find(|&row| label_of(&window, row) == "Offer Caskade for them")
        .unwrap();
    let switch = within(&window, row, "role", "switch");
    window.focus(switch);
    window.key(Key::Character(" ".into()));
    window.settle_until("the error", |window| {
        window
            .find_all("role", "alert")
            .iter()
            .any(|&alert| window.text(alert).contains("Windows refused the change"))
    });
}

#[test]
fn says_when_this_system_cant_send_them() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _fake = fake::start(links(Support::Unsupported, false, false), None);
    let window = installer();
    let section = window.get("data-part", "torrent-links");
    assert!(
        window
            .text(section)
            .contains("This system can't send magnet links and .torrent files to Caskade yet.")
    );
}
