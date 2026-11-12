//! Accounts in the shell (#106), on the frame's fixture library, as the
//! webview's account tests: Switch account signs out to the picker, a
//! locked account asks for its PIN (a wrong one is refused) and signs in;
//! a standard account's age limit asks for an admin's PIN before Play, then
//! plays (the permission rules themselves are caskade-app's tests). Each
//! test puts the library's accounts back as they were
//! (one admin, no PIN, signed in): other frame tests share them.

use crate::frame::tests::{ONE, Window, source};
use crate::game::launch_tests::{Script, launches, script, started};
use caskade_core::accounts::{AccountAccess, AccountRole, Permissions};
use caskade_ui_data::accounts::{Account, Accounts, NewAccount};
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};

const PIN: &str = "2468";

fn accounts() -> Accounts {
    source().settings.accounts()
}

fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    caskade_app::runtime::block_on(future)
}

/// The shared library's admin gets a PIN and a standard account `Kit` joins
/// (with `max_age`, unrated games held back too); puts them back on drop.
struct Household {
    admin: Account,
    kit: Account,
}

fn household(max_age: Option<u8>) -> Household {
    let accounts = accounts();
    let list = block(accounts.list()).unwrap();
    let admin = list
        .accounts
        .iter()
        .find(|account| Some(&account.id) == list.active_id.as_ref())
        .cloned()
        .expect("the fixture admin is signed in");
    block(accounts.set_pin(&admin.id, Some(PIN.into()))).unwrap();
    let kit = block(accounts.create(NewAccount {
        name: "Kit".into(),
        avatar: crate::ui::DEFAULT_AVATAR.into(),
        access: AccountAccess {
            role: AccountRole::Standard,
            permissions: Permissions::default(),
            max_age,
            unrated_needs_pin: max_age.is_some(),
        },
        pin: None,
    }))
    .unwrap();
    Household { admin, kit }
}

impl Drop for Household {
    fn drop(&mut self) {
        let accounts = accounts();
        let _ = block(accounts.sign_in(&self.admin.id, Some(PIN.into())));
        let _ = block(accounts.delete(&self.kit.id));
        let _ = block(accounts.set_pin(&self.admin.id, None));
        let _ = block(accounts.sign_in(&self.admin.id, None));
    }
}

fn active() -> Option<String> {
    block(accounts().list()).unwrap().active_id
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

/// The picker's tile for `name`.
fn tile(window: &Window, name: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-part", "account")
        .into_iter()
        .find(|&node| window.text(node).contains(name))
        .unwrap_or_else(|| panic!("no tile for {name}"))
}

fn enter_pin(window: &mut Window, pin: &str) {
    let field = window.get("data-part", "field");
    window.focus(field);
    window.type_text(pin);
    window.key(Key::Named(NamedKey::Enter));
}

#[test]
fn switch_account_signs_out_to_the_picker_and_a_locked_account_asks_for_its_pin() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let home = household(None);
    let mut window = Window::open();
    // The top bar's avatar: who is signed in, and Switch account.
    window.settle_until("the account menu", |window| {
        window.find("aria-haspopup", "menu").is_some()
            && window.find("data-part", "avatar").is_some()
    });
    let avatar = window
        .find_all("data-part", "avatar")
        .into_iter()
        .find(|&node| window.attr(node, "aria-haspopup").as_deref() == Some("menu"))
        .expect("the account button");
    window.click(avatar);
    window.settle_until("the menu", |window| {
        window.find("data-cs", "account-menu").is_some()
    });
    let menu = window.text(window.get("data-cs", "account-menu"));
    assert!(
        menu.contains(&home.admin.name) && menu.contains("Admin"),
        "{menu}"
    );
    let switch = window
        .find_all("role", "menuitem")
        .into_iter()
        .find(|&node| window.text(node).contains("Switch account"))
        .expect("Switch account");
    window.click(switch);
    // Signed out: the picker, starting on the account that signed in last.
    window.settle_until("the picker", |window| {
        window.find("data-cs", "account-picker").is_some()
    });
    assert_eq!(active(), None);
    assert!(body(&window).contains("Who's playing?"));
    let admin = tile(&window, &home.admin.name);
    assert_eq!(window.attr(admin, "data-status").as_deref(), Some("locked"));
    // A locked account asks for its PIN; a wrong one is refused.
    window.click(admin);
    window.settle_until("the PIN pad", |window| {
        window.find("data-cs", "pin-pad").is_some()
    });
    assert_eq!(
        window
            .attr(window.get("data-cs", "account-picker"), "data-step")
            .as_deref(),
        Some("pin")
    );
    enter_pin(&mut window, "0000");
    window.settle_until("refused", |window| {
        body(window).contains("That PIN isn't right.")
    });
    assert_eq!(active(), None);
    enter_pin(&mut window, PIN);
    window.settle_until("signed in", |window| {
        window.find("data-cs", "account-picker").is_none()
            && window.find("data-cs", "app-shell").is_some()
    });
    assert_eq!(active(), Some(home.admin.id.clone()));
    // An account without a PIN signs in at once.
    block(accounts().sign_out()).unwrap();
    window.settle_until("the picker again", |window| {
        window.find("data-cs", "account-picker").is_some()
    });
    let kit = tile(&window, "Kit");
    window.click(kit);
    window.settle_until("Kit signed in", |window| {
        window.find("data-cs", "app-shell").is_some()
    });
    assert_eq!(active(), Some(home.kit.id.clone()));
}

#[test]
fn an_age_limit_asks_for_an_admins_pin_before_play_then_plays() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let home = household(Some(12));
    // The library's first game by title, which has a release Play starts.
    let first = block(source().library.page(&caskade_core::model::ListGames {
        query: None,
        filter: caskade_core::model::GameFilter::All,
        platform: None,
        sort: caskade_core::model::GameSort::Title,
        offset: 0,
        limit: 1,
    }))
    .unwrap()
    .items
    .remove(0);
    block(accounts().sign_in(&home.kit.id, None)).unwrap();
    let mut window = Window::open();
    // Held-back games leave Kit's library (search still finds them, marked);
    // the game page says why.
    let game = first.id.clone();
    window.go(&format!("/games/{game}"));
    window.settle_until("the lock", |window| {
        window.find("data-age-lock", "true").is_some()
    });
    let lock = window.get("data-age-lock", "true");
    assert!(window.text(lock).contains("Admin PIN needed"));
    let _script = script(Script {
        outcomes: [started(&game)].into(),
        ..Script::default()
    });
    // Searched for, it's listed, marked; its card starts Play.
    window.go(&format!(
        "/library?filter=all&sort=title&layout=grid&q={}",
        first.title.replace(' ', "%20")
    ));
    window.settle_until("its card", |window| {
        !window.find_all("data-index", "0").is_empty()
    });
    let card = window.get("data-index", "0");
    assert!(window.find_in(card, "data-age-lock", "true").is_some());
    window.double_click(card);
    window.settle_until("the age gate", |window| {
        body(window).contains("An admin PIN is needed to play")
    });
    assert!(body(&window).contains("Approve"), "{}", body(&window));
    assert_eq!(launches(), 0, "nothing starts before the PIN");
    // Kit's own PIN pad isn't an admin's: a wrong PIN is refused.
    enter_pin(&mut window, "0000");
    window.settle_until("refused", |window| {
        body(window).contains("That PIN isn't right.")
    });
    assert_eq!(launches(), 0);
    enter_pin(&mut window, PIN);
    window.settle_until("the launch", |_| launches() >= 1);
}
