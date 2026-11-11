//! Settings → Storage (#104), ported from the webview's `StoragePanel`
//! tests, over a real storage service on a temporary folder: the locations
//! list with their status and default; a card selects one; its label and
//! folder template save; a folder is added; another location becomes the
//! default, by its button or a right click's menu; one is removed keeping
//! its files; "always use the default" saves.

use super::label_of;
use crate::data::use_data_source;
use crate::frame::tests::{ONE, Window, source};
use crate::input::InputRoot;
use caskade_core::storage::{RootSummary, StorageService};
use caskade_ui_data::storage::Storage;
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;
use serde_json::json;
use std::sync::Mutex;

static STORAGE: Mutex<Option<Storage>> = Mutex::new(None);

fn app() -> Element {
    use_data_source(|| source().clone());
    let storage = STORAGE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .expect("the test's storage");
    use_context_provider(|| storage);
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            crate::settings::storage::StorageSettings {}
        }
    }
}

struct Locations {
    temp: tempfile::TempDir,
    service: StorageService,
    archive: String,
    fast: String,
}

/// Two locations: Archive (the default) and Fast SSD.
fn locations() -> Locations {
    caskade_app::runtime::block_on(async move {
        use caskade_core::{
            AppPaths, db::LibraryStore, launch::LaunchService, settings::SettingsService,
        };
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(&temp.path().join("app"));
        let store = LibraryStore::in_memory().await.unwrap();
        let settings = SettingsService::new(store.clone());
        settings.register_core(&paths).unwrap();
        let launch = LaunchService::new(store.clone()).with_aliases(None);
        let service = StorageService::new(store.clone(), settings.clone(), launch)
            .unwrap()
            .with_paths(paths);
        let archive = service
            .register_root(temp.path().join("one"), Some("Archive".into()))
            .await
            .unwrap()
            .root
            .id;
        let fast = service
            .register_root(temp.path().join("two"), Some("Fast SSD".into()))
            .await
            .unwrap()
            .root
            .id;
        let bus = caskade_core::bus::Bus::default();
        let accounts = caskade_core::accounts::AccountService::open(store.clone(), bus.clone())
            .await
            .unwrap();
        let source = caskade_ui_data::storage::StorageSource::new(
            store.clone(),
            bus,
            service.reconciler().unwrap(),
        );
        let storage =
            Storage::new(service.clone(), None, settings).with_locations(source, accounts);
        *STORAGE.lock().unwrap_or_else(|e| e.into_inner()) = Some(storage);
        Locations {
            temp,
            service,
            archive,
            fast,
        }
    })
}

fn roots(service: &StorageService) -> Vec<RootSummary> {
    let service = service.clone();
    caskade_app::runtime::block_on(async move { service.list_roots().await }).unwrap()
}

fn open() -> Window {
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        blitz_dom::DocumentConfig {
            viewport: Some(blitz_traits::shell::Viewport::new(
                1280,
                1600,
                1.0,
                blitz_traits::shell::ColorScheme::Dark,
            )),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until("the locations", |window| {
        window.find_all("data-part", "root").len() == 2
    });
    window
}

fn card(window: &Window, name: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-part", "root")
        .into_iter()
        .find(|&card| window.text(card).contains(name))
        .unwrap_or_else(|| panic!("no location {name}"))
}

fn button(window: &Window, text: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| window.text(button).trim() == text)
        .unwrap_or_else(|| panic!("no {text} button"))
}

fn field(window: &Window, label: &str) -> blitz_dom::NodeId {
    let wrapper = window
        .find_all("data-cs", "text-field")
        .into_iter()
        .find(|&field| label_of(window, field) == label)
        .unwrap_or_else(|| panic!("no field {label}"));
    window
        .find_all("data-part", "input")
        .into_iter()
        .find(|&input| super::is_inside(window, input, wrapper))
        .unwrap()
}

fn press(window: &mut Window, node: blitz_dom::NodeId) {
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

fn clear_and_type(window: &mut Window, node: blitz_dom::NodeId, text: &str) {
    window.focus(node);
    window.key(Key::Named(NamedKey::End));
    for _ in 0..40 {
        window.key(Key::Named(NamedKey::Backspace));
    }
    window.type_text(text);
}

#[test]
fn lists_the_locations_selects_one_and_saves_its_label_and_template() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let locations = locations();
    let mut window = open();
    let text = window.text(window.get("data-cs", "storage-settings"));
    assert!(text.contains("2 registered locations"), "{text}");
    let archive = card(&window, "Archive");
    assert!(window.text(archive).contains("Default · online"));
    assert_eq!(
        window.attr(archive, "data-status").as_deref(),
        Some("online")
    );
    assert_eq!(
        window.attr(archive, "aria-pressed").as_deref(),
        Some("true")
    );
    // Enter on a card selects it, and its section shows.
    let fast = card(&window, "Fast SSD");
    press(&mut window, fast);
    window.settle_until("Fast SSD selected", |window| {
        window
            .attr(card(window, "Fast SSD"), "aria-pressed")
            .as_deref()
            == Some("true")
    });
    let label = field(&window, "Location label");
    assert_eq!(window.attr(label, "value").as_deref(), Some("Fast SSD"));
    clear_and_type(&mut window, label, "Games NVMe");
    window.settle_until("unsaved", |window| {
        window
            .text(window.get("data-part", "selected-root"))
            .contains("Unsaved changes")
    });
    let save = button(&window, "Save location");
    press(&mut window, save);
    window.settle_until("saved", |_| {
        roots(&locations.service).iter().any(|root| {
            root.root.id == locations.fast && root.root.label.as_deref() == Some("Games NVMe")
        })
    });
    window.settle_until("the card", |window| {
        window
            .find_all("data-part", "root")
            .iter()
            .any(|&card| window.text(card).contains("Games NVMe"))
    });
}

#[test]
fn adds_a_folder_and_makes_another_location_the_default() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let locations = locations();
    let mut window = open();
    let folder = locations.temp.path().join("three");
    std::fs::create_dir_all(&folder).unwrap();
    let path = field(&window, "Folder");
    window.focus(path);
    window.type_text(&folder.to_string_lossy());
    let name = field(&window, "Label");
    window.focus(name);
    window.type_text("USB");
    let add = button(&window, "Add location");
    press(&mut window, add);
    window.settle_until("added", |window| {
        window.find_all("data-part", "root").len() == 3
            && window.find_all("role", "status").iter().any(|&status| {
                window
                    .text(status)
                    .contains("Location added. Caskade looked there for games it installed before.")
            })
    });
    assert_eq!(roots(&locations.service).len(), 3);
    // The new one is selected: Make default makes it the default.
    let usb = card(&window, "USB");
    assert_eq!(window.attr(usb, "aria-pressed").as_deref(), Some("true"));
    let make = button(&window, "Make default");
    press(&mut window, make);
    window.settle_until("the default", |window| {
        roots(&locations.service)
            .iter()
            .any(|root| root.root.label.as_deref() == Some("USB") && root.root.is_default)
            && window.text(card(window, "USB")).contains("Default")
    });
    assert!(
        !roots(&locations.service)
            .iter()
            .any(|root| root.root.id == locations.archive && root.root.is_default)
    );
}

/// A right click on a location offers Make default and Remove location…;
/// removing keeps its files.
#[test]
fn a_right_click_removes_a_location_keeping_its_files() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let locations = locations();
    let mut window = open();
    let fast = card(&window, "Fast SSD");
    window.right_click(fast);
    window.settle_until("the menu", |window| window.find("role", "menu").is_some());
    let remove = window
        .find_all("role", "menuitem")
        .into_iter()
        .find(|&item| window.text(item).contains("Remove location…"))
        .expect("Remove location…");
    press(&mut window, remove);
    window.settle_until("the question", |window| {
        window
            .find("data-part", "remove-root")
            .is_some_and(|section| {
                window
                    .text(section)
                    .contains("Remove this location from the library? Files on disk are kept.")
            })
    });
    let keep = button(&window, "Keep files and remove from library");
    press(&mut window, keep);
    window.settle_until("removed", |window| {
        window.find_all("data-part", "root").len() == 1
    });
    assert!(
        roots(&locations.service)
            .iter()
            .all(|root| root.root.id != locations.fast)
    );
    assert!(locations.temp.path().join("two").exists(), "its files stay");
    // The default can't be removed: it says to choose another first.
    let archive = card(&window, "Archive");
    press(&mut window, archive);
    assert!(
        window
            .text(window.get("data-part", "selected-root"))
            .contains("Choose another default before removing this location.")
    );
}

#[test]
fn always_use_the_default_saves() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _locations = locations();
    let settings = source().settings.clone();
    let before = caskade_app::runtime::block_on(settings.get("core.storage"))
        .unwrap()
        .value;
    let mut window = open();
    let row = window
        .find_all("data-cs", "setting-row")
        .into_iter()
        .find(|&row| label_of(&window, row) == "Always use the default location")
        .unwrap();
    let switch = window
        .find_all("role", "switch")
        .into_iter()
        .find(|&switch| super::is_inside(&window, switch, row))
        .unwrap();
    window.focus(switch);
    window.key(Key::Character(" ".into()));
    window.settle_until("saved", |_| {
        caskade_app::runtime::block_on(settings.get("core.storage"))
            .unwrap()
            .value["alwaysUseDefault"]
            != before["alwaysUseDefault"]
    });
    let restore = before.clone();
    caskade_app::runtime::block_on(settings.update("core.storage", move |_| restore)).unwrap();
    assert_ne!(before["alwaysUseDefault"], json!(null));
}
