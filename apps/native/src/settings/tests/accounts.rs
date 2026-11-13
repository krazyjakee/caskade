//! Settings → Accounts and My profile (#105), over a fixture library whose
//! one admin is signed in, as the webview's `accounts.test.tsx`: adding a
//! standard account is refused with the core's reason until the admin has a
//! PIN, then made; an account's access saves; deleting asks first and
//! takes it away; the rating board labels age limits. My profile saves its
//! name, and its PIN is set on the pad, entered twice, and a wrong current
//! PIN is refused.

use std::sync::Mutex;

use crate::data::{DataSource, use_data_source};
use crate::frame::tests::{ONE, Window};
use crate::input::InputRoot;
use blitz_dom::{DocumentConfig, NodeId};
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_ui_data::accounts::{AccountAccess, AccountRole, Accounts, NewAccount, Permissions};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Key, Modifiers, NamedKey};
use dioxus_native::prelude::*;
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq)]
enum Panel {
    Accounts,
    Profile,
}

static SHOWN: Mutex<Option<(DataSource, Panel)>> = Mutex::new(None);

fn app() -> Element {
    let (source, panel) = SHOWN.lock().unwrap().clone().expect("a library");
    use_data_source(move || source.clone());
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            if panel == Panel::Accounts {
                crate::settings::accounts::AccountsSettings {}
            } else {
                crate::settings::accounts::ProfileSettings {}
            }
        }
    }
}

fn library() -> (DataSource, Accounts) {
    caskade_app::runtime::block_on(async {
        let source = DataSource::fixtures(3).await.unwrap();
        let accounts = source.settings.accounts();
        (source, accounts)
    })
}

fn open(source: &DataSource, panel: Panel, ready: &str) -> Window {
    *SHOWN.lock().unwrap() = Some((source.clone(), panel));
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        DocumentConfig {
            viewport: Some(Viewport::new(1280, 2400, 1.0, ColorScheme::Dark)),
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

fn body(window: &Window) -> String {
    window
        .doc
        .inner
        .borrow()
        .find_body_node()
        .map(|body| body.text_content())
        .unwrap_or_default()
}

fn button(window: &Window, text: &str) -> NodeId {
    window
        .find_all("role", "button")
        .into_iter()
        .chain(window.find_all("data-cs", "button"))
        .find(|&node| window.text(node).trim() == text)
        .unwrap_or_else(|| panic!("no button {text}"))
}

fn row_switch(window: &Window, label: &str) -> NodeId {
    let row = window
        .find_all("data-cs", "setting-row")
        .into_iter()
        .find(|&row| super::label_of(window, row) == label)
        .unwrap_or_else(|| panic!("no row {label}"));
    window
        .find_all("role", "switch")
        .into_iter()
        .find(|&switch| super::is_inside(window, switch, row))
        .unwrap_or_else(|| panic!("no switch in {label}"))
}

fn retype(window: &mut Window, id: &str, text: &str) {
    let field = window.get("id", id);
    window.focus(field);
    window.key_with(Key::Character("a".into()), Modifiers::CONTROL);
    window.key(Key::Named(NamedKey::Backspace));
    window.type_text(text);
}

/// Types `pin` on the pad's field and confirms it.
fn enter_pin(window: &mut Window, pin: &str) {
    let field = window.get("data-part", "field");
    window.focus(field);
    window.type_text(pin);
    window.key(Key::Named(NamedKey::Enter));
}

fn list(accounts: &Accounts) -> caskade_ui_data::accounts::AccountList {
    caskade_app::runtime::block_on(accounts.list()).unwrap()
}

#[test]
fn an_admin_adds_changes_and_deletes_accounts() {
    let _one = caskade_core::lock(&ONE);
    let (source, accounts) = library();
    let mut window = open(&source, Panel::Accounts, "account-add");
    let me = window.text(window.get("data-part", "name"));
    assert!(me.contains("(you)"), "{me}");
    let details = window.text(window.get("data-part", "details"));
    assert!(details.starts_with("Admin · no PIN"), "{details}");
    // The only admin can't delete itself.
    assert!(window.find("data-part", "delete").is_none());

    // A standard account needs every admin to have a PIN: the core says so.
    let add = window.get("id", "account-add");
    press(&mut window, add);
    window.settle_until("the editor", |window| {
        window.find("data-cs", "account-editor").is_some()
    });
    retype(&mut window, "account-name", "Kit");
    let install = row_switch(&window, "Install games");
    window.focus(install);
    window.key(Key::Character(" ".into()));
    let save = window.get("id", "account-save");
    press(&mut window, save);
    window.settle_until("refused", |window| window.find("role", "alert").is_some());
    let reason = window.text(window.get("role", "alert"));
    assert!(reason.ends_with('.'), "{reason}");
    assert_eq!(list(&accounts).accounts.len(), 1);

    // With the admin's PIN set, it's made, with what it may do.
    let admin = list(&accounts).active_id.unwrap();
    caskade_app::runtime::block_on(accounts.set_pin(&admin, Some("2468".into()))).unwrap();
    let save = window.get("id", "account-save");
    press(&mut window, save);
    window.settle_until("made", |window| {
        window.find("data-cs", "account-editor").is_none()
            && window.find_all("data-part", "account").len() == 2
    });
    let kit = list(&accounts)
        .accounts
        .into_iter()
        .find(|account| account.name == "Kit")
        .expect("Kit");
    assert_eq!(kit.role, AccountRole::Standard);
    assert!(kit.permissions.install);
    assert!(body(&window).contains("Standard · no PIN · installs games"));

    // Edit: a settings section it may change saves.
    let edit = window
        .find_all("data-part", "edit")
        .into_iter()
        .find(|&node| window.attr(node, "aria-label").as_deref() == Some("Edit Kit"))
        .expect("Edit Kit");
    press(&mut window, edit);
    window.settle_until("the editor", |window| {
        window.find("data-cs", "account-editor").is_some()
    });
    let appearance = row_switch(&window, "Appearance");
    window.focus(appearance);
    window.key(Key::Character(" ".into()));
    let save = window.get("id", "account-save");
    press(&mut window, save);
    let check = accounts.clone();
    window.settle_until("saved", |_| {
        list(&check)
            .accounts
            .iter()
            .any(|account| account.permissions.settings == ["appearance"])
    });

    // Delete asks first, then takes it away.
    window.settle_until("closed", |window| {
        window.find("data-cs", "account-editor").is_none()
    });
    let delete = window.get("data-part", "delete");
    press(&mut window, delete);
    window.settle_until("asked", |window| {
        window.find("role", "alertdialog").is_some()
    });
    assert!(body(&window).contains("Kit's collections go with it."));
    let confirm = button(&window, "Delete");
    press(&mut window, confirm);
    window.settle_until("deleted", |window| {
        window.find_all("data-part", "account").len() == 1
    });
    assert_eq!(list(&accounts).accounts.len(), 1);
}

#[test]
fn age_limits_are_labelled_with_the_rating_board() {
    let _one = caskade_core::lock(&ONE);
    let (source, accounts) = library();
    caskade_app::runtime::block_on(async {
        let admin = accounts.list().await.unwrap().active_id.unwrap();
        accounts.set_pin(&admin, Some("2468".into())).await.unwrap();
        accounts
            .create(NewAccount {
                name: "Kit".into(),
                avatar: crate::ui::DEFAULT_AVATAR.into(),
                access: AccountAccess {
                    role: AccountRole::Standard,
                    permissions: Permissions::default(),
                    max_age: Some(13),
                    unrated_needs_pin: false,
                },
                pin: None,
            })
            .await
            .unwrap();
    });
    let mut window = open(&source, Panel::Accounts, "rating-board");
    window.settle_until("PEGI", |window| body(window).contains("Up to 13+"));
    let settings = source.settings.clone();
    caskade_app::runtime::block_on(settings.update("core.accounts", |mut value: Value| {
        value["ratingBoard"] = json!("esrb");
        value
    }))
    .unwrap();
    window.settle_until("ESRB", |window| body(window).contains("ESRB T (13+)"));
}

#[test]
fn my_profile_saves_its_name_and_sets_its_pin() {
    let _one = caskade_core::lock(&ONE);
    let (source, accounts) = library();
    let mut window = open(&source, Panel::Profile, "profile-name");
    assert!(body(&window).contains("an admin."));
    // The name waits for Save; Enter saves it.
    retype(&mut window, "profile-name", "Robin");
    window.key(Key::Named(NamedKey::Enter));
    let check = accounts.clone();
    window.settle_until("renamed", |_| {
        list(&check)
            .accounts
            .iter()
            .any(|account| account.name == "Robin")
    });

    // Set PIN…: the new PIN twice; a mismatch says so.
    let set = button(&window, "Set PIN…");
    press(&mut window, set);
    window.settle_until("the pad", |window| {
        window.find("data-cs", "pin-pad").is_some()
    });
    enter_pin(&mut window, "1357");
    window.settle_until("again", |window| {
        body(window).contains("Enter the new PIN again")
    });
    enter_pin(&mut window, "1358");
    window.settle_until("mismatch", |window| {
        body(window).contains("The PINs don't match. Try again.")
    });
    enter_pin(&mut window, "1357");
    let check = accounts.clone();
    window.settle_until("set", |window| {
        window.find("data-cs", "pin-pad").is_none()
            && list(&check).accounts.iter().all(|account| account.has_pin)
    });

    // Change PIN… asks for the current one first, and refuses a wrong one.
    window.settle_until("Change PIN…", |window| {
        body(window).contains("Change PIN…")
    });
    let change = button(&window, "Change PIN…");
    press(&mut window, change);
    window.settle_until("current", |window| {
        body(window).contains("Enter Robin's current PIN")
    });
    enter_pin(&mut window, "0000");
    window.settle_until("wrong", |window| {
        body(window).contains("That PIN isn't right.")
    });
    enter_pin(&mut window, "1357");
    window.settle_until("new", |window| {
        body(window).contains("Choose a new PIN for Robin")
    });
}
