//! Settings → Emulation (#105), over a fixture library with Caskade's
//! shipped emulator definitions and a temporary app folder, as the
//! webview's `emulation.test.tsx`, `scanners.test.tsx` and
//! `RommPanel.test.tsx`: an emulator added by hand is listed and removed
//! after asking; a custom profile previews its command line and saves; a
//! platform's default emulator saves; a ROM scanner's scan is reviewed and
//! imported; and a RomM server is added with its password kept out of
//! everything but the keychain.

use std::sync::{Arc, Mutex};

use crate::data::{DataSource, JobFakes, use_data_source};
use crate::frame::tests::{ONE, Window};
use crate::input::InputRoot;
use blitz_dom::{DocumentConfig, NodeId};
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::emulation::{
    Emulator, EmulatorInstall, EmulatorProfile, EmulatorTarget, ProfileHooks, ProfileRef,
    RomScanner, Tracking,
};
use caskade_core::paths::AppPaths;
use caskade_core::romm::{MemorySecrets, RommService};
use caskade_ui_data::emulation::Emulation;
use caskade_ui_data::romm::Romm;
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Key, Modifiers, NamedKey};
use dioxus_native::prelude::*;

struct Library {
    source: DataSource,
    emulation: Emulation,
    romm: Option<Romm>,
    secrets: Arc<MemorySecrets>,
    dir: tempfile::TempDir,
}

#[derive(Clone)]
struct Shown {
    source: DataSource,
    emulation: Emulation,
    romm: Option<Romm>,
    section: Option<String>,
}

static SHOWN: Mutex<Option<Shown>> = Mutex::new(None);

fn app() -> Element {
    let shown = SHOWN.lock().unwrap().clone().expect("a library");
    let source = shown.source.clone();
    use_data_source(move || source.clone());
    let emulation = shown.emulation.clone();
    use_context_provider(move || emulation);
    if let Some(romm) = shown.romm.clone() {
        use_context_provider(move || romm);
    }
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            crate::settings::emulation::EmulationSettings { section: shown.section.clone() }
        }
    }
}

/// The fixture library, its emulation over a temporary app folder, and
/// (`with_romm`) RomM with its secrets in memory.
fn library(with_romm: bool) -> Library {
    caskade_app::runtime::block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let fixtures = caskade_ui_data::fixtures::open(3).await.unwrap();
        let paths = AppPaths::under(&dir.path().join("app"));
        paths.create().unwrap();
        let (store, bus): (LibraryStore, Bus) = (fixtures.store.clone(), fixtures.bus.clone());
        let emulation = Emulation::shipped(store.clone(), bus.clone(), &paths);
        let secrets = Arc::new(MemorySecrets::default());
        let romm = with_romm.then(|| {
            let settings = caskade_core::settings::SettingsService::new(store.clone());
            settings.register_core(&paths).unwrap();
            caskade_core::romm::register_settings(&settings).unwrap();
            let service = caskade_core::emulation::EmulationService::new(
                store.clone(),
                bus.clone(),
                Default::default(),
            );
            Romm::new(
                RommService::new(
                    store.clone(),
                    bus.clone(),
                    settings,
                    service,
                    dir.path().join("romm-cache"),
                )
                .with_secrets(secrets.clone()),
            )
        });
        let source = DataSource {
            library: fixtures.library,
            settings: fixtures.settings,
            themes: fixtures.themes,
            discovery: fixtures.discovery,
            stores: fixtures.stores,
            plugins: fixtures.plugins,
            addons: fixtures.addons,
            jobs: fixtures.jobs,
            job_fakes: Some(JobFakes {
                queue: fixtures.job_queue,
                refresh: fixtures.refresh,
                requests: fixtures.requests,
            }),
            events: fixtures.events,
        };
        Library {
            source,
            emulation,
            romm,
            secrets,
            dir,
        }
    })
}

fn open(library: &Library, section: Option<&str>, ready: &str) -> Window {
    *SHOWN.lock().unwrap() = Some(Shown {
        source: library.source.clone(),
        emulation: library.emulation.clone(),
        romm: library.romm.clone(),
        section: section.map(str::to_owned),
    });
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        DocumentConfig {
            viewport: Some(Viewport::new(1280, 4000, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until(ready, |window| window.find("id", ready).is_some());
    window
}

fn press(window: &mut Window, node: NodeId) {
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

fn press_id(window: &mut Window, id: &str) {
    let node = window.get("id", id);
    press(window, node);
}

fn body(window: &Window) -> String {
    window
        .doc
        .inner
        .borrow()
        .find_body_node()
        .map(|body| body.text_content())
        .unwrap_or_default()
}

fn retype(window: &mut Window, id: &str, text: &str) {
    let field = window.get("id", id);
    window.focus(field);
    window.key_with(Key::Character("a".into()), Modifiers::CONTROL);
    window.key(Key::Named(NamedKey::Backspace));
    window.type_text(text);
}

fn button(window: &Window, text: &str) -> NodeId {
    window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&node| window.text(node).trim() == text)
        .unwrap_or_else(|| panic!("no button {text}"))
}

/// Opens the select `id` and clicks its option `label`.
fn pick(window: &mut Window, id: &str, label: &str) {
    let trigger = window
        .find("id", id)
        .unwrap_or_else(|| panic!("no select {id}"));
    window.focus(trigger);
    window.key(Key::Named(NamedKey::ArrowDown));
    window.settle_until("the list", |window| {
        window.find("role", "listbox").is_some()
    });
    let option = window
        .find_all("role", "option")
        .into_iter()
        .find(|&node| window.text(node).trim() == label)
        .unwrap_or_else(|| panic!("no option {label}"));
    window.click(option);
    window.settle_until("chosen", |window| window.find("role", "listbox").is_none());
}

/// An emulator that runs `sh` from `$PATH`, with one custom profile for
/// `platform` files ending in `extension`.
fn add_emulator(library: &Library, platform: &str, extension: &str) -> (String, String) {
    let view = caskade_app::runtime::block_on(library.emulation.save_emulator(Emulator {
        id: String::new(),
        name: "Shell".into(),
        definition: None,
        install: EmulatorInstall::Path {
            binary: "sh".into(),
        },
        profiles: vec![EmulatorProfile::Custom {
            id: String::new(),
            name: "Plain".into(),
            executable: None,
            args: vec!["{RomPath}".into()],
            working_dir: None,
            platforms: vec![platform.into()],
            extensions: vec![extension.into()],
            env: Vec::new(),
            tracking: Tracking::Default,
            hooks: ProfileHooks::default(),
        }],
    }))
    .unwrap();
    let profile = view.emulator.profiles[0].id().to_owned();
    (view.emulator.id, profile)
}

#[test]
fn an_emulator_added_by_hand_is_listed_and_removed_after_asking() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let library = library(false);
    let mut window = open(&library, None, "emulation-add");
    assert!(body(&window).contains("No emulators yet."));
    // Add emulator: a folder with its executable, checked when saved.
    let folder = library.dir.path().join("emu");
    std::fs::create_dir_all(&folder).unwrap();
    let executable = folder.join("run");
    std::fs::write(&executable, "#!/bin/sh\n").unwrap();
    press_id(&mut window, "emulation-add");
    window.settle_until("the form", |window| {
        window.find("id", "emulator-name").is_some()
    });
    retype(&mut window, "emulator-name", "Portable");
    retype(&mut window, "emulator-folder", &folder.to_string_lossy());
    retype(
        &mut window,
        "emulator-executable",
        &executable.to_string_lossy(),
    );
    press_id(&mut window, "emulator-save");
    let emulation = library.emulation.clone();
    window.settle_until("saved", |_| {
        caskade_app::runtime::block_on(emulation.emulators())
            .unwrap()
            .iter()
            .any(|row| row.emulator.name == "Portable")
    });
    window.settle_until("listed", |window| {
        window
            .find_all("role", "option")
            .into_iter()
            .any(|node| window.text(node).contains("Portable"))
    });
    // Remove asks first, then takes it away.
    window.settle_until("its settings", |window| {
        window.find("id", "emulator-remove").is_some()
    });
    press_id(&mut window, "emulator-remove");
    window.settle_until("asked", |window| {
        window.find("role", "alertdialog").is_some()
    });
    assert!(body(&window).contains("Its profiles go with it"));
    let confirm = button(&window, "Remove");
    press(&mut window, confirm);
    let emulation = library.emulation.clone();
    window.settle_until("removed", |_| {
        caskade_app::runtime::block_on(emulation.emulators())
            .unwrap()
            .is_empty()
    });
}

#[test]
fn a_custom_profile_previews_its_command_and_saves() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let library = library(false);
    let (emulator, _) = add_emulator(&library, "nintendo_nes", "nes");
    let mut window = open(&library, None, "profile-add-custom");
    press_id(&mut window, "profile-add-custom");
    window.settle_until("the editor", |window| {
        window.find("id", "profile-name").is_some()
    });
    retype(&mut window, "profile-name", "Fast");
    retype(&mut window, "profile-args", "--fast \"{RomPath}\"");
    // The command line follows the arguments.
    window.settle_until("the preview", |window| {
        window
            .find_all("data-part", "token")
            .into_iter()
            .any(|node| window.text(node) == "--fast")
    });
    press_id(&mut window, "profile-save");
    let emulation = library.emulation.clone();
    window.settle_until("saved", |_| {
        caskade_app::runtime::block_on(emulation.emulators())
            .unwrap()
            .iter()
            .find(|row| row.emulator.id == emulator)
            .is_some_and(|row| {
                row.emulator.profiles.iter().any(|profile| {
                    matches!(profile, EmulatorProfile::Custom { name, args, .. }
                        if name == "Fast" && args.first().map(String::as_str) == Some("--fast"))
                })
            })
    });
}

#[test]
fn a_platforms_default_emulator_saves() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let library = library(false);
    let (emulator, profile) = add_emulator(&library, "nintendo_nes", "nes");
    let mut window = open(&library, Some("platforms"), "platform-default-nintendo_nes");
    pick(
        &mut window,
        "platform-default-nintendo_nes",
        "Shell · Plain",
    );
    let emulation = library.emulation.clone();
    window.settle_until("saved", |_| {
        caskade_app::runtime::block_on(emulation.platforms())
            .unwrap()
            .iter()
            .find(|row| row.platform.id == "nintendo_nes")
            .is_some_and(|row| {
                row.settings.default
                    == Some(ProfileRef {
                        emulator_id: emulator.clone(),
                        profile_id: profile.clone(),
                    })
            })
    });
}

#[test]
fn a_rom_scan_is_reviewed_and_imported() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let library = library(false);
    let (emulator, profile) = add_emulator(&library, "nintendo_nes", "nes");
    let roms = library.dir.path().join("roms");
    std::fs::create_dir_all(&roms).unwrap();
    std::fs::write(roms.join("Tidewater Quest.nes"), b"NES\x1a").unwrap();
    let scanner = caskade_app::runtime::block_on(library.emulation.save_scanner(RomScanner {
        id: String::new(),
        name: "Cartridges".into(),
        folder: roms.clone(),
        platform: Some("nintendo_nes".into()),
        profile: ProfileRef {
            emulator_id: emulator,
            profile_id: profile,
        },
        launch_with: EmulatorTarget::PlatformDefault,
        recursive: true,
        scan_archives: false,
        skip_cloud_placeholders: true,
        exclusions: Vec::new(),
        in_library_update: false,
    }))
    .unwrap();
    let scan = format!("scanner-scan-{}", scanner.id);
    let mut window = open(&library, Some("scanners"), &scan);
    press_id(&mut window, &scan);
    window.settle_until("the review", |window| {
        window.find("id", "review-import").is_some() && body(window).contains("Tidewater Quest")
    });
    press_id(&mut window, "review-import");
    let source = library.source.clone();
    window.settle_until("imported", |_| {
        caskade_app::runtime::block_on(source.library.search("Tidewater Quest"))
            .is_ok_and(|page| !page.items.is_empty())
    });
}

#[test]
fn a_romm_server_is_added_with_its_password_in_the_keychain_only() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let library = library(true);
    let mut window = open(&library, Some("romm"), "romm-add");
    press_id(&mut window, "romm-add");
    window.settle_until("the editor", |window| {
        window.find("id", "romm-url").is_some()
    });
    retype(&mut window, "romm-name", "Attic");
    // Nothing answers here: tests never reach a network.
    retype(&mut window, "romm-url", "http://127.0.0.1:1");
    retype(&mut window, "romm-username", "ada");
    retype(&mut window, "romm-secret", "hunter22");
    press_id(&mut window, "romm-save");
    let romm = library.romm.clone().unwrap();
    window.settle_until("saved", |_| {
        caskade_app::runtime::block_on(romm.overview()).is_ok_and(|overview| {
            overview
                .settings
                .servers
                .iter()
                .any(|server| server.name == "Attic")
        })
    });
    let overview = caskade_app::runtime::block_on(romm.overview()).unwrap();
    let server = overview
        .settings
        .servers
        .iter()
        .find(|server| server.name == "Attic")
        .unwrap();
    // The password is in the keychain (sealed), and nowhere in the
    // settings or on screen.
    use caskade_core::romm::SecretStore;
    assert!(
        caskade_app::runtime::block_on(library.secrets.get(&server.id))
            .unwrap()
            .is_some()
    );
    assert!(!format!("{:?}", overview.settings).contains("hunter22"));
    assert!(!body(&window).contains("hunter22"));
}

#[test]
fn without_romm_the_section_says_why() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let library = library(false);
    let window = open(&library, Some("romm"), "emulation-add");
    let mut window = window;
    window.settle_until("why", |window| {
        body(window).contains("RomM servers couldn't be loaded")
    });
}
