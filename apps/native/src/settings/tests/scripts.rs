//! Settings → Scripts (#105), over a library in memory, as the webview's
//! `scripts` tests: a script added, named and saved shows in the list and
//! in the order at launch; a duplicate starts off; the order changes with
//! Move down; Delete asks first; Test runs it through the launch's runner
//! and shows its output; an account that isn't an admin is refused.

use crate::data::use_data_source;
use crate::frame::tests::{ONE, Window, source};
use crate::input::InputRoot;
use blitz_dom::DocumentConfig;
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_core::accounts::AccountService;
use caskade_core::db::LibraryStore;
use caskade_ui_data::scripts::{ScriptScope, Scripts};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;
use std::sync::Mutex;

static SCRIPTS: Mutex<Option<Scripts>> = Mutex::new(None);

fn app() -> Element {
    use_data_source(|| source().clone());
    let scripts = SCRIPTS.lock().unwrap().clone().expect("scripts");
    use_context_provider(move || scripts);
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            div { id: "panel",
                crate::settings::scripts::ScriptsSettings {}
            }
        }
    }
}

/// A library in memory, and its accounts (the first, signed in, an
/// admin).
fn library() -> (LibraryStore, AccountService) {
    caskade_app::runtime::block_on(async {
        let store = LibraryStore::in_memory().await.unwrap();
        let accounts = AccountService::open(store.clone(), caskade_core::bus::Bus::default())
            .await
            .unwrap();
        (store, accounts)
    })
}

fn open(scripts: Scripts) -> Window {
    source();
    *SCRIPTS.lock().unwrap() = Some(scripts);
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        DocumentConfig {
            viewport: Some(Viewport::new(1280, 1600, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until("the scripts", |window| {
        window.find("data-cs", "scripts").is_some()
    });
    window
}

fn press(window: &mut Window, id: &str) {
    let node = window.get("id", id);
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

fn names(window: &Window) -> Vec<String> {
    window
        .find_all("data-part", "script")
        .into_iter()
        .map(|node| {
            window
                .text(node)
                .split("Before launch")
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned()
        })
        .collect()
}

fn type_into(window: &mut Window, id: &str, text: &str) {
    let node = window.get("id", id);
    window.focus(node);
    window.key_with(
        Key::Character("a".into()),
        dioxus_native::prelude::keyboard_types::Modifiers::CONTROL,
    );
    window.key(Key::Named(NamedKey::Backspace));
    window.type_text(text);
}

#[test]
fn a_script_is_added_ordered_duplicated_and_deleted() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let (store, accounts) = library();
    let mut window = open(Scripts::new(store.clone(), accounts));
    assert!(
        window
            .text(window.get("data-cs", "scripts"))
            .contains("No scripts here yet.")
    );
    assert!(
        window
            .text(window.get("data-part", "order"))
            .contains("No scripts run at launch.")
    );
    press(&mut window, "scripts-add");
    window.settle_until("the form", |window| {
        window.find("data-part", "form").is_some()
    });
    type_into(&mut window, "script-name", "Mount saves");
    let text = window.get("id", "script-text");
    window.focus(text);
    window.type_text("echo mounted");
    press(&mut window, "script-save");
    window.settle_until("saved", |window| names(window) == ["Mount saves"]);
    let order = window.text(window.get("data-part", "order"));
    assert!(order.contains("Mount saves"), "{order}");
    assert!(order.contains("Global"), "{order}");
    let saved = caskade_app::runtime::block_on(async {
        store.lifecycle_scripts(&ScriptScope::Global).await
    })
    .unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].name, "Mount saves");
    // A duplicate starts off, named as a copy.
    press(&mut window, "script-duplicate");
    window.settle_until("the copy", |window| {
        window
            .attr(window.get("id", "script-name"), "value")
            .as_deref()
            == Some("Mount saves (copy)")
    });
    press(&mut window, "script-save");
    window.settle_until("two", |window| {
        names(window) == ["Mount saves", "Mount saves (copy)"]
    });
    let copy = caskade_app::runtime::block_on(async {
        store.lifecycle_scripts(&ScriptScope::Global).await
    })
    .unwrap();
    assert!(!copy[1].enabled, "a copy starts off");
    // The copy is shown: up puts it first.
    press(&mut window, "script-up");
    window.settle_until("reordered", |window| {
        names(window) == ["Mount saves (copy)", "Mount saves"]
    });
    // Delete asks, then deletes.
    press(&mut window, "script-delete");
    window.settle_until("the question", |window| {
        window.text(window.get("id", "script-delete")) == "Delete “Mount saves (copy)”?"
    });
    press(&mut window, "script-delete");
    window.settle_until("deleted", |window| names(window) == ["Mount saves"]);
}

#[test]
fn a_test_runs_the_script_once_and_shows_its_output() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let (store, accounts) = library();
    let mut window = open(Scripts::new(store, accounts));
    press(&mut window, "scripts-add");
    window.settle_until("the form", |window| {
        window.find("data-part", "form").is_some()
    });
    let text = window.get("id", "script-text");
    window.focus(text);
    window.type_text("echo \"hello from $CASKADE_PHASE\"");
    press(&mut window, "script-test");
    window.settle_until("the result", |window| {
        window.find("data-part", "result").is_some()
    });
    let result = window.get("data-part", "result");
    assert_eq!(
        window.attr(result, "data-status").as_deref(),
        Some("succeeded")
    );
    assert!(
        window.text(result).starts_with("Finished · "),
        "{}",
        window.text(result)
    );
    let output = window.text(window.get("data-part", "output"));
    assert!(output.starts_with("hello from"), "{output}");
}

#[test]
fn an_account_that_isnt_an_admin_cant_change_global_scripts() {
    use caskade_core::accounts::{AccountAccess, AccountRole, NewAccount, Permissions};
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let (store, accounts) = library();
    let kid = accounts.clone();
    caskade_app::runtime::block_on(async move {
        let admin = kid.active().await.unwrap().expect("the first account");
        // An admin needs a PIN once a standard account exists.
        kid.set_pin(&admin.id, Some("2468".into())).await.unwrap();
        let made = kid
            .create(NewAccount {
                name: "Kid".into(),
                avatar: admin.avatar.clone(),
                access: AccountAccess {
                    role: AccountRole::Standard,
                    permissions: Permissions::default(),
                    max_age: None,
                    unrated_needs_pin: false,
                },
                pin: None,
            })
            .await
            .unwrap();
        kid.sign_in(&made.id, None).await.unwrap();
    });
    let mut window = open(Scripts::new(store.clone(), accounts));
    press(&mut window, "scripts-add");
    window.settle_until("the form", |window| {
        window.find("data-part", "form").is_some()
    });
    press(&mut window, "script-save");
    window.settle_until("refused", |window| {
        window.find("data-part", "error").is_some()
    });
    let saved = caskade_app::runtime::block_on(async {
        store.lifecycle_scripts(&ScriptScope::Global).await
    })
    .unwrap();
    assert!(saved.is_empty());
}
