//! Settings → Backup (#105), over a fixture library with its own settings
//! (`core.backups` registered, as the app does) and a backup folder in a
//! temporary directory, as the webview's `BackupSettings` tests: the parts
//! to back up and what a backup leaves out; Back up now writes a backup,
//! checked, and says so; a folder that isn't there blocks it with why;
//! choosing the backup shows what it holds and what restoring keeps; the
//! schedule saves.

use std::sync::Mutex;

use crate::data::{DataSource, use_data_source};
use crate::frame::tests::{ONE, Window};
use crate::input::InputRoot;
use blitz_dom::DocumentConfig;
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_ui_data::Settings;
use caskade_ui_data::backups::Backups;
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;
use serde_json::{Value, json};

struct Library {
    source: DataSource,
    settings: Settings,
    backups: Backups,
    folder: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

static SHOWN: Mutex<Option<(DataSource, Backups)>> = Mutex::new(None);

fn library() -> Library {
    caskade_app::runtime::block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let mut source = DataSource::fixtures(3).await.unwrap();
        // Settings of its own, with core.backups, over a library to back up.
        let paths = AppPaths::under(&dir.path().join("app"));
        paths.create().unwrap();
        // A library on disk, as a backup copies it.
        let store = caskade_core::db::LibraryStore::open_default(&paths)
            .await
            .unwrap();
        let accounts = caskade_core::accounts::AccountService::open(
            store.clone(),
            caskade_core::bus::Bus::default(),
        )
        .await
        .unwrap();
        let service = SettingsService::new(store.clone());
        service.register_core(&paths).unwrap();
        caskade_backup::register_settings(&service).unwrap();
        let settings = Settings::new(service.clone(), accounts);
        let folder = dir.path().join("backups");
        std::fs::create_dir_all(&folder).unwrap();
        let destination = folder.to_string_lossy().into_owned();
        settings
            .update(caskade_backup::NAMESPACE, move |mut value: Value| {
                value["destination"] = json!(destination);
                value
            })
            .await
            .unwrap();
        let backups = caskade_backup::Backups::new(store, paths);
        backups.set_settings(service);
        // The status follows jobs through the backups' watch, as the app runs it.
        drop(backups.watch());
        source.settings = settings.clone();
        Library {
            source,
            settings,
            backups: Backups::new(backups),
            folder,
            _dir: dir,
        }
    })
}

fn app() -> Element {
    let (source, backups) = SHOWN.lock().unwrap().clone().expect("a library");
    use_data_source(move || source.clone());
    use_context_provider(move || backups);
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            crate::settings::backup::BackupSettings {}
        }
    }
}

fn open(library: &Library) -> Window {
    *SHOWN.lock().unwrap() = Some((library.source.clone(), library.backups.clone()));
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        DocumentConfig {
            viewport: Some(Viewport::new(1280, 2400, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until("Back up now", |window| {
        window.find("id", "backup-now").is_some()
    });
    window
}

fn press(window: &mut Window, id: &str) {
    let node = window.get("id", id);
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

#[test]
fn back_up_now_writes_a_checked_backup_and_says_so() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let library = library();
    let mut window = open(&library);
    // Every part listed, the library always in.
    let parts = window.text(window.get("data-part", "components"));
    assert!(parts.contains("Library and settings"), "{parts}");
    assert!(parts.contains("Downloaded artwork"), "{parts}");
    window.settle_until("the plan", |window| {
        window
            .doc
            .inner
            .borrow()
            .find_body_node()
            .is_some_and(|body| {
                body.text_content()
                    .contains("before compression. Never included:")
            })
    });
    press(&mut window, "backup-now");
    window.settle_until("backed up", |window| {
        window
            .find("data-part", "last")
            .is_some_and(|node| window.text(node).starts_with("Backed up "))
    });
    let written: Vec<_> = std::fs::read_dir(&library.folder)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "zip"))
        .collect();
    assert_eq!(written.len(), 1, "one backup in the folder");
}

#[test]
fn a_folder_that_isnt_there_blocks_the_backup_with_why() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let library = library();
    let gone = library
        .folder
        .join("not-here")
        .to_string_lossy()
        .into_owned();
    caskade_app::runtime::block_on(library.settings.update(
        caskade_backup::NAMESPACE,
        move |mut value: Value| {
            value["destination"] = json!(gone);
            value
        },
    ))
    .unwrap();
    let mut window = open(&library);
    window.settle_until("why", |window| {
        window
            .doc
            .inner
            .borrow()
            .find_body_node()
            .is_some_and(|body| {
                body.text_content()
                    .contains("Can't back up: the backup folder isn't there.")
            })
    });
    assert_eq!(
        window
            .attr(window.get("id", "backup-now"), "aria-disabled")
            .as_deref(),
        Some("true")
    );
    // The schedule saves.
    let weekly = window
        .find_all("role", "radio")
        .into_iter()
        .chain(window.find_all("aria-pressed", "false"))
        .find(|&node| window.text(node).trim() == "Weekly")
        .expect("Weekly");
    window.focus(weekly);
    window.key(Key::Named(NamedKey::Enter));
    let settings = library.settings.clone();
    window.settle_until("weekly saved", |_| {
        caskade_app::runtime::block_on(settings.get(caskade_backup::NAMESPACE))
            .unwrap()
            .value["schedule"]
            == json!("weekly")
    });
}
