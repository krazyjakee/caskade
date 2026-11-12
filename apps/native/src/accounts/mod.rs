//! Local accounts in the shell (#106, #207), as the webview's
//! `shell/accounts`: the account picker in place of the shell while nobody
//! is signed in, the signed-in account's menu in the top bar, and the admin
//! PIN an age limit asks for before a game or a release's tool starts
//! (#209). The core keeps the active account and every rule; a single
//! account without a PIN is signed in at start, so a library that never set
//! accounts up opens as before.

mod age_gate;
mod last;
mod menu;
mod picker;
#[cfg(test)]
mod tests;

pub use age_gate::{AgeGateDialog, AgeLock};
pub use menu::AccountMenu;

use caskade_ui_data::Key as DataKey;
use caskade_ui_data::accounts::{AccountList, Accounts};
use caskade_ui_i18n::t;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use crate::data::{Query, use_settings, use_settings_query};
use crate::router::{Route, use_router};
use crate::ui::FailureNotice;

/// The accounts' catalogued roots.
pub const ANATOMY: &[Entry] = &[ACCOUNT_PICKER, ACCOUNT_MENU];

pub const ACCOUNT_PICKER: Entry = Entry {
    parts: &[
        (
            "titlebar",
            "The bar along the top: it drags the window, and holds the window controls.",
        ),
        ("heading", "The title."),
        ("accounts", "The account tiles."),
        (
            "account",
            "An account's tile; `data-status=\"locked\"` when a PIN locks it. The last account to sign in has `data-focus-default`.",
        ),
        ("avatar", "An account's picture (an `avatar`)."),
        ("name", "An account's name."),
        ("lock", "The lock on a PIN-locked account's tile."),
        ("back", "Back from the PIN pad to the accounts."),
    ],
    states: &[("data-step", &["accounts", "pin"])],
    contains: &["avatar", "pin-pad", "window-controls", "hint-bar"],
    ..Entry::new(
        "account-picker",
        "AccountPicker (apps/native/src/accounts/picker.rs)",
        Source::App,
        "Who is playing: the accounts to sign in as, at start and after Switch account, and the PIN pad for a locked one. `data-step` is `accounts` or `pin`.",
    )
};

pub const ACCOUNT_MENU: Entry = Entry {
    parts: &[
        ("avatar", "The account's picture (an `avatar`)."),
        ("name", "The account's name."),
        ("role", "Admin or Standard."),
    ],
    contains: &["avatar"],
    ..Entry::new(
        "account-menu",
        "AccountMenu (apps/native/src/accounts/menu.rs)",
        Source::App,
        "The head of the menu the topbar's avatar opens: who is signed in. Its actions are the `menu`'s items.",
    )
};

/// The app's accounts: a test's, else the library's.
pub fn use_accounts() -> Accounts {
    let provided = try_use_context::<Accounts>();
    let settings = use_settings();
    provided.unwrap_or_else(|| settings.accounts())
}

/// The accounts, and who is signed in; again after every account change
/// (a sign-in, a lock, an edit).
pub fn use_account_list() -> Query<AccountList> {
    use_settings_query(
        DataKey::Settings("core.accounts".to_owned()),
        |settings| async move { settings.accounts().list().await },
    )
}

/// `children` (the shell) while someone is signed in; the account picker
/// otherwise. Nothing shows while the accounts load, so a locked library
/// never flashes up.
#[component]
pub fn AccountGate(children: Element) -> Element {
    let mut list = use_account_list();
    let router = use_router();
    let loaded = list.read().clone();
    match loaded {
        None => rsx! {},
        // Not knowing who is signed in must not open the shell, where every
        // change would be refused (#160): say why, and offer to ask again.
        Some(Err(error)) => rsx! {
            AccountsUnavailable { error, on_retry: move |_| list.restart() }
        },
        Some(Ok(accounts))
            if accounts.accounts.is_empty()
                || accounts
                    .accounts
                    .iter()
                    .any(|account| Some(&account.id) == accounts.active_id.as_ref()) =>
        {
            children
        }
        Some(Ok(accounts)) => rsx! {
            picker::AccountPicker {
                list: accounts,
                on_signed_in: move |_| {
                    // A collection page belonged to whoever was signed in
                    // before.
                    if matches!(router.peek(), Route::Collection { .. }) {
                        router.navigate("/collections?filter=all");
                    }
                },
            }
        },
    }
}

#[component]
fn AccountsUnavailable(
    error: caskade_core::error::ErrorReport,
    on_retry: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "flex h-screen flex-col",
            header {
                "data-window-drag": "true",
                class: "flex h-topbar shrink-0 items-center justify-end pr-4",
                crate::frame::window::WindowControls {}
            }
            main { "data-focus-section": "main", class: "grid flex-1 place-items-center px-6",
                FailureNotice {
                    class: "w-full max-w-lg",
                    error,
                    title: t!("Caskade couldn't check who is signed in"),
                    on_retry: move |_| on_retry.call(()),
                }
            }
        }
    }
}
