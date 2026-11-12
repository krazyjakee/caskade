//! Settings → Plugins (#104), ported from the webview's `PluginsSettings`
//! tests over the fixtures' plugin host and its own folder: a plugin is
//! listed with its status and access; one that asks for more is reviewed
//! and allowed; it turns off and on, shows its details and uninstalls; an
//! invalid one says why; hooks and permissions read as words.

use super::{is_inside, open};
use crate::frame::tests::{ONE, Window, source};
use crate::settings::plugins::{access_summary, capability_lines, hook_names};
use caskade_ui_data::plugins::PluginStatus;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use serde_json::json;

fn write_plugin(folder: &str, manifest: serde_json::Value) -> std::path::PathBuf {
    let dir = caskade_ui_data::fixtures::plugins_folder()
        .join("installed")
        .join(folder);
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    // Never run: enabling a plugin starts nothing until it's asked.
    std::fs::write(dir.join("dist/backend.wasm"), b"").unwrap();
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    dir
}

/// Removes a test's plugin folders, whatever happened.
struct Gone(Vec<std::path::PathBuf>);

impl Drop for Gone {
    fn drop(&mut self) {
        for folder in &self.0 {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}

fn reader() -> serde_json::Value {
    json!({
        "id": "dev.example.reader",
        "name": "Library Reader",
        "version": "1.0.0",
        "api": "^0.1",
        "backend": "dist/backend.wasm",
        "activation": ["onRequest"],
        "description": "Reads what you play.",
        "permissions": {"library": "read", "network": ["example.com"]},
    })
}

fn plugins(name: &str) -> Window {
    let listed = caskade_app::runtime::block_on(source().plugins.list());
    assert!(
        listed
            .as_ref()
            .is_ok_and(|list| list.iter().any(|plugin| plugin.name == name)),
        "the host lists {listed:?}"
    );
    let mut window = open("/settings?tab=plugins");
    window.settle_until("Plugins", |window| {
        window.find("data-settings", "plugins").is_some()
            && window
                .find_all("data-part", "plugin")
                .iter()
                .any(|&row| window.text(row).contains(name))
    });
    window
}

fn row(window: &Window, name: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-part", "plugin")
        .into_iter()
        .find(|&row| window.text(row).contains(name))
        .unwrap_or_else(|| panic!("no plugin {name}"))
}

fn button(window: &Window, within: blitz_dom::NodeId, text: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| is_inside(window, button, within) && window.text(button).trim() == text)
        .unwrap_or_else(|| panic!("no {text} button"))
}

fn press(window: &mut Window, node: blitz_dom::NodeId) {
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

fn status(id: &str) -> Option<PluginStatus> {
    caskade_app::runtime::block_on(source().plugins.list())
        .unwrap()
        .into_iter()
        .find(|plugin| plugin.id == id)
        .map(|plugin| plugin.status)
}

#[test]
fn hooks_and_permissions_read_as_words() {
    let (changes, hears) = hook_names(&[
        "launch.before".into(),
        "game.exited".into(),
        "game.started".into(),
        "custom.thing".into(),
    ]);
    assert_eq!(changes, ["games before they start"]);
    assert_eq!(
        hears,
        ["games starting, stopping and moving", "custom.thing"]
    );
    let plugin: caskade_ui_data::plugins::PluginSummary = serde_json::from_value(json!({
        "id": "x", "name": "X", "version": null, "description": null, "author": null,
        "folder": "", "status": "disabled", "errors": [],
        "permissions": {"network": ["a.com", "b.com"], "fs": ["$HOME/ROMs:ro", "$HOME/Saves:rw"], "process": true, "library": "write"},
        "contributes": null, "hasUi": false, "hasBackend": true, "failures": 0,
        "lastError": null, "settingsError": null, "dev": false
    }))
    .unwrap();
    assert_eq!(
        capability_lines(&plugin),
        [
            "Connect to a.com, b.com",
            "Read files in $HOME/ROMs",
            "Read and change files in $HOME/Saves",
            "Run programs and change how games are launched",
            "Read and change your library",
        ]
    );
    assert_eq!(
        access_summary(&plugin),
        "Uses network, files, programs, your library"
    );
}

/// A plugin that asks for more than it was granted is reviewed and
/// allowed; then it turns off by its switch, shows its details, and
/// uninstalls after Uninstall asks.
#[test]
fn a_plugin_is_reviewed_allowed_turned_off_and_uninstalled() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _gone = Gone(vec![write_plugin("reader", reader())]);
    let mut window = plugins("Library Reader");
    let reader = row(&window, "Library Reader");
    assert!(window.text(reader).contains("Reads what you play."));
    assert!(window.text(reader).contains("Uses network, your library"));
    // Allowing it: the review says what it asks for, then turns it on.
    if status("dev.example.reader") != Some(PluginStatus::Enabled) {
        let allow = match window
            .find_all("data-cs", "button")
            .into_iter()
            .find(|&button| {
                is_inside(&window, button, reader)
                    && window.text(button).trim() == "Review and allow…"
            }) {
            Some(allow) => allow,
            None => {
                // Off rather than waiting for consent: its switch turns it on.
                let switch = window.get("aria-label", "Enable Library Reader");
                window.focus(switch);
                window.key(Key::Character(" ".into()));
                window.settle_until("on", |_| {
                    status("dev.example.reader") == Some(PluginStatus::Enabled)
                });
                window.settle();
                button(&window, row(&window, "Library Reader"), "Details…")
            }
        };
        if window.text(allow).trim() == "Review and allow…" {
            press(&mut window, allow);
            window.settle_until("the review", |window| {
                window.find("data-cs", "plugin-review").is_some()
            });
            let review = window.get("data-cs", "plugin-review");
            let text = window.text(review);
            assert!(text.contains("It asks to"), "{text}");
            assert!(text.contains("Connect to example.com"), "{text}");
            assert!(text.contains("Read your library"), "{text}");
            let confirm = button(&window, review, "Allow");
            press(&mut window, confirm);
            window.settle_until("allowed", |window| {
                window.find("data-cs", "plugin-review").is_none()
                    && status("dev.example.reader") == Some(PluginStatus::Enabled)
            });
        }
    }
    window.settle_until("its switch", |window| {
        window.find("aria-label", "Enable Library Reader").is_some()
    });
    let switch = window.get("aria-label", "Enable Library Reader");
    assert_eq!(window.attr(switch, "aria-checked").as_deref(), Some("true"));
    // Off by its switch.
    window.focus(switch);
    window.key(Key::Character(" ".into()));
    window.settle_until("off", |_| {
        status("dev.example.reader") == Some(PluginStatus::Disabled)
    });
    // Its details say what it can do.
    let details = window.get("aria-label", "Library Reader details");
    press(&mut window, details);
    window.settle_until("the details", |window| {
        window
            .find("data-cs", "plugin-review")
            .is_some_and(|dialog| window.text(dialog).contains("It can"))
    });
    let dialog = window.get("data-cs", "plugin-review");
    let close = button(&window, dialog, "Close");
    press(&mut window, close);
    window.settle_until("closed", |window| {
        window.find("data-cs", "plugin-review").is_none()
    });
    // Uninstall asks, then removes it.
    let reader = row(&window, "Library Reader");
    let uninstall = button(&window, reader, "Uninstall…");
    press(&mut window, uninstall);
    let confirm = button(
        &window,
        row(&window, "Library Reader"),
        "Uninstall Library Reader",
    );
    press(&mut window, confirm);
    window.settle_until("uninstalled", |window| {
        status("dev.example.reader").is_none()
            && window
                .find_all("data-part", "plugin")
                .iter()
                .all(|&row| !window.text(row).contains("Library Reader"))
    });
}

/// An invalid manifest is listed by its folder with what's wrong, and
/// can't be turned on.
#[test]
fn an_invalid_plugin_says_why() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut manifest = reader();
    manifest["id"] = json!("NotReverseDns");
    manifest["name"] = json!("Broken Plugin");
    let _gone = Gone(vec![write_plugin("broken", manifest)]);
    let window = plugins("broken");
    let broken = row(&window, "broken");
    assert_eq!(
        window.attr(broken, "data-status").as_deref(),
        Some("invalid")
    );
    assert!(window.text(broken).contains("Invalid"));
    // Named by its folder, with what's wrong.
    assert!(
        window.text(broken).contains("id: must be reverse-DNS"),
        "{}",
        window.text(broken)
    );
    let switch = window
        .find_all("role", "switch")
        .into_iter()
        .find(|&switch| is_inside(&window, switch, broken))
        .unwrap();
    assert_eq!(
        window.attr(switch, "aria-disabled").as_deref(),
        Some("true")
    );
}

#[test]
fn a_0_1_plugins_ui_says_it_needs_an_update_and_a_drawn_page_shows() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    // A 0.1 plugin with a JavaScript UI half (#109): it doesn't run here.
    let mut old = reader();
    old["id"] = json!("dev.example.old-ui");
    old["name"] = json!("Old UI");
    old["ui"] = json!("dist/ui.js");
    let old_folder = write_plugin("old-ui", old);
    std::fs::write(old_folder.join("dist/ui.js"), b"export default {};").unwrap();
    // A 0.2 plugin whose backend draws its own settings page.
    let drawn = json!({
        "id": "dev.example.drawn",
        "name": "Drawn Settings",
        "version": "1.0.0",
        "api": "^0.2",
        "backend": "dist/backend.wasm",
        "activation": ["onRequest"],
        "contributes": {"slots": [{"slot": "settings.pages", "id": "page", "title": "Options"}]}
    });
    let _gone = Gone(vec![old_folder, write_plugin("drawn", drawn)]);
    // Listing reads the folders again, so the host knows it.
    caskade_app::runtime::block_on(source().plugins.list()).unwrap();
    caskade_app::runtime::block_on(source().plugins.set_enabled("dev.example.drawn", true))
        .unwrap();
    let fake = source().plugin_fake.as_ref().unwrap();
    fake.set_plugins(vec![caskade_ui_data::plugin_ui::DrawnPlugin {
        id: "dev.example.drawn".into(),
        name: "Drawn Settings".into(),
        version: "1.0.0".into(),
        contributes: vec![caskade_ui_data::plugin_ui::UiContribution {
            slot: "settings.pages".into(),
            id: "page".into(),
            title: Some("Options".into()),
            modes: Vec::new(),
        }],
    }]);
    fake.draw(
        "dev.example.drawn",
        "settings.pages",
        "page",
        "*",
        Ok(vec![caskade_ui_data::plugin_ui::ui::note(
            "Nothing to set yet.",
        )]),
    );
    struct Clean;
    impl Drop for Clean {
        fn drop(&mut self) {
            let fake = source().plugin_fake.as_ref().unwrap();
            fake.clear();
            fake.set_plugins(Vec::new());
        }
    }
    let _clean = Clean;
    let mut window = plugins("Old UI");
    let old = row(&window, "Old UI");
    let notice = window
        .find_in(old, "data-part", "old-ui")
        .expect("the notice");
    assert!(
        window
            .text(notice)
            .contains("Its UI was made for plugin API 0.1, which this app doesn't run."),
        "{}",
        window.text(notice)
    );
    window.settle_until("the drawn page", |window| {
        window
            .find_all("data-part", "plugin-settings")
            .into_iter()
            .any(|section| {
                window.attr(section, "data-plugin").as_deref() == Some("dev.example.drawn")
                    && window.text(section).contains("Nothing to set yet.")
            })
    });
}
