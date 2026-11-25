//! Settings → Accounts and My profile (#208, #105). Accounts, for admins:
//! every account with Edit and Delete, Add account, and the rating board
//! every account's ratings and age limits use; the editor sets an
//! account's name, avatar, role, PIN, what it may change and its age
//! limit. My profile, for every account: its own name, avatar and PIN. The
//! core keeps every rule (an admin remains; every admin has a PIN while
//! standard accounts exist) and refuses changes an account may not make
//! whatever this shows; a refusal shows its reason.

mod editor;
mod profile;

use caskade_core::error::ErrorReport;
use caskade_ui_data::accounts::{Account, AccountRole, Accounts};
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;
use serde_json::Value;

pub use self::editor::AccountEditor;
pub use self::profile::ProfileSettings;
use super::pin::{SetPin, sentence};
use super::state::use_settings_section;
use super::sync::SectionSync;
use crate::accounts::use_account_list;
use crate::data::{change, use_settings};
use crate::flows::pin_pad::Check;
use crate::game::{BOARDS, DEFAULT_BOARD, board_name};
use crate::ui::icons::{PENCIL, PLUS, SHIELD_CHECK, TRASH, USERS};
use crate::ui::{
    Avatar, AvatarSize, Button, ButtonSize, ButtonVariant, ConfirmDialog, FailureNotice, Select,
    SelectOption, SettingRow, SettingsSection,
};

/// The settings section that holds the preferred rating board.
const ACCOUNTS_SETTINGS: &str = "core.accounts";
pub(super) const NO_LIMIT: &str = "none";

pub(super) fn use_accounts() -> Accounts {
    use_settings().accounts()
}

/// The board the panels label ages with: the settings', else PEGI.
fn board_of(value: Option<Value>) -> String {
    value
        .as_ref()
        .and_then(|value| value.get("ratingBoard"))
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_BOARD)
        .to_owned()
}

/// The age limit picker's options: no limit, then `board`'s steps, and
/// `current` when it is none of them (set under another board).
pub(super) fn age_limit_options(board: &str, current: Option<u8>) -> Vec<SelectOption> {
    let known = if caskade_core::igdb::age::board_steps(board).is_empty() {
        DEFAULT_BOARD
    } else {
        board
    };
    let name = board_name(known);
    let mut options = vec![SelectOption::new(NO_LIMIT, tp!("age limit", "No limit"))];
    options.extend(
        caskade_core::igdb::age::board_steps(known)
            .iter()
            .map(|(label, age)| {
                let text = if label.bytes().all(|byte| byte.is_ascii_digit()) {
                    format!("{name} {label}")
                } else {
                    format!("{name} {label} ({age}+)")
                };
                SelectOption::new(age.to_string(), text)
            }),
    );
    if let Some(age) = current
        && !options.iter().any(|option| option.value == age.to_string())
    {
        options.push(SelectOption::new(
            age.to_string(),
            t!(
                note = "An age limit no step of the chosen rating board matches: 'Up to 13+'",
                "Up to {age}+",
                age = age
            ),
        ));
    }
    options
}

/// "PEGI 12", or "No limit".
fn age_limit_label(board: &str, age: Option<u8>) -> String {
    let wanted = age.map_or_else(|| NO_LIMIT.to_owned(), |age| age.to_string());
    age_limit_options(board, age)
        .into_iter()
        .find(|option| option.value == wanted)
        .map_or_else(|| tp!("age limit", "No limit"), |option| option.label)
}

/// "Standard · PIN · installs games · PEGI 12".
fn details(account: &Account, board: &str) -> String {
    let admin = account.role == AccountRole::Admin;
    let parts = [
        Some(if admin {
            tp!("account role", "Admin")
        } else {
            tp!("account role", "Standard")
        }),
        Some(if account.has_pin {
            tp!("account details", "PIN")
        } else {
            tp!("account details", "no PIN")
        }),
        (!admin && account.permissions.install).then(|| tp!("account details", "installs games")),
        account
            .max_age
            .filter(|_| !admin)
            .map(|age| age_limit_label(board, Some(age))),
    ];
    parts.into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

/// A PIN check through the account's own sign-in, for changing its own.
pub(super) fn own_check(accounts: &Accounts, id: &str) -> Check {
    let accounts = accounts.clone();
    let id = id.to_owned();
    Check::new(move |pin: String| {
        let accounts = accounts.clone();
        let id = id.clone();
        async move {
            change(async move { accounts.sign_in(&id, Some(pin)).await })
                .await
                .map_err(|report| sentence(&report))
        }
    })
}

/// Sets or removes `id`'s PIN.
pub(super) fn pin_setter(accounts: &Accounts, id: &str) -> SetPin {
    let accounts = accounts.clone();
    let id = id.to_owned();
    SetPin::new(move |pin: Option<String>| {
        let accounts = accounts.clone();
        let id = id.clone();
        async move {
            change(async move { accounts.set_pin(&id, pin).await })
                .await
                .map(|_| ())
                .map_err(|report| sentence(&report))
        }
    })
}

#[derive(Clone, PartialEq)]
enum Editing {
    Add,
    Edit(String),
    Delete(String),
}

/// Settings → Accounts.
#[component]
pub fn AccountsSettings() -> Element {
    let mut list = use_account_list();
    let ratings = use_settings_section(ACCOUNTS_SETTINGS);
    let mut editing = use_signal(|| None::<Editing>);
    let board = board_of(ratings.value());
    let loaded = match &*list.read() {
        None => None,
        Some(Ok(loaded)) => Some(Ok(loaded.clone())),
        Some(Err(report)) => Some(Err(report.clone())),
    };
    // A failed list says so with Try again; a loading one says it's loading (#57).
    let loaded = match loaded {
        None => {
            return rsx! {
                p { role: "status", class: "text-sm text-fg-muted", {t!("Loading accounts…")} }
            };
        }
        Some(Err(report)) => {
            return rsx! {
                FailureNotice {
                    error: report,
                    title: t!("Accounts couldn't be loaded"),
                    on_retry: move |_| list.restart(),
                }
            };
        }
        Some(Ok(loaded)) => loaded,
    };
    let active = loaded.active_id.clone();
    let admins = loaded
        .accounts
        .iter()
        .filter(|account| account.role == AccountRole::Admin)
        .count();
    // Deleting yourself or the last admin is refused: not offered.
    let deletable = |account: &Account| {
        active.as_deref() != Some(account.id.as_str())
            && !(account.role == AccountRole::Admin && admins == 1)
    };
    let target = match editing() {
        Some(Editing::Edit(id) | Editing::Delete(id)) => loaded
            .accounts
            .iter()
            .find(|account| account.id == id)
            .cloned(),
        _ => None,
    };
    let board_options: Vec<SelectOption> = BOARDS
        .iter()
        .map(|(id, name)| SelectOption::new(*id, *name))
        .collect();
    let close = move |_| editing.set(None);
    rsx! {
        div { "data-settings": "accounts", class: "flex flex-col gap-4",
            SettingsSection {
                icon: Some(USERS),
                title: tp!("settings tab", "Accounts"),
                description: t!("Who uses Caskade here, and what each can change."),
                aside: rsx! {
                    Button {
                        id: "account-add",
                        size: ButtonSize::Sm,
                        icon: Some(PLUS),
                        on_press: move |_| editing.set(Some(Editing::Add)),
                        {t!("Add account…")}
                    }
                },
                ul {
                    "data-cs": "account-list",
                    "aria-label": tp!("settings tab", "Accounts"),
                    class: "flex flex-col",
                    for account in loaded.accounts.iter().cloned() {
                        li {
                            key: "{account.id}",
                            "data-part": "account",
                            class: "flex items-center gap-3 border-t border-border py-3 first:border-t-0",
                            Avatar { part: "avatar", avatar: Some(account.avatar.clone()), name: account.name.clone(), size: AvatarSize::Md }
                            div { class: "min-w-0 flex-1",
                                div { "data-part": "name", class: "truncate text-sm font-medium",
                                    "{account.name}"
                                    if active.as_deref() == Some(account.id.as_str()) {
                                        span { class: "text-fg-subtle",
                                            " "
                                            {t!(note = "After the signed-in account's name", "(you)")}
                                        }
                                    }
                                }
                                div { "data-part": "details", class: "text-xs text-fg-muted", {details(&account, &board)} }
                            }
                            Button {
                                "data-part": "edit",
                                size: ButtonSize::Sm,
                                icon: Some(PENCIL),
                                "aria-label": t!("Edit {name}", name = account.name.clone()),
                                on_press: {
                                    let id = account.id.clone();
                                    move |_| editing.set(Some(Editing::Edit(id.clone())))
                                },
                                {t!("Edit…")}
                            }
                            if deletable(&account) {
                                Button {
                                    "data-part": "delete",
                                    size: ButtonSize::Sm,
                                    variant: ButtonVariant::Ghost,
                                    icon: Some(TRASH),
                                    "aria-label": t!("Delete {name}", name = account.name.clone()),
                                    on_press: {
                                        let id = account.id.clone();
                                        move |_| editing.set(Some(Editing::Delete(id.clone())))
                                    },
                                    {t!("Delete…")}
                                }
                            }
                        }
                    }
                }
            }
            SettingsSection {
                icon: Some(SHIELD_CHECK),
                title: t!("Age ratings"),
                description: t!("For every account. Age limits are set on each account."),
                if ratings.value().is_some() {
                    SettingRow {
                        label: t!("Rating board"),
                        description: t!("The ratings games show, and the steps age limits are picked from."),
                        Select {
                            id: "rating-board",
                            options: board_options,
                            value: Some(board.clone()),
                            disabled: ratings.saving(),
                            class: "w-44",
                            on_change: move |next: String| ratings.set_field("ratingBoard", Value::String(next)),
                        }
                    }
                }
                if let Some(report) = ratings.error() {
                    p { role: "alert", class: "text-sm text-danger", {sentence(&report)} }
                }
                SectionSync { namespace: ACCOUNTS_SETTINGS }
            }
            match (editing(), target) {
                (Some(Editing::Add), _) => rsx! {
                    AccountEditor { account: None, active_id: active.clone(), board: board.clone(), on_close: close }
                },
                (Some(Editing::Edit(_)), Some(account)) => rsx! {
                    AccountEditor { account: Some(account), active_id: active.clone(), board: board.clone(), on_close: close }
                },
                (Some(Editing::Delete(_)), Some(account)) => rsx! {
                    DeleteAccountDialog { account, on_close: close }
                },
                _ => rsx! {},
            }
        }
    }
}

#[component]
fn DeleteAccountDialog(account: Account, on_close: EventHandler<()>) -> Element {
    let accounts = use_accounts();
    let mut pending = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let id = account.id.clone();
    rsx! {
        ConfirmDialog {
            title: t!("Delete {name}?", name = account.name.clone()),
            confirm_label: t!("Delete"),
            pending_label: Some(t!("Deleting…")),
            pending: pending(),
            on_confirm: move |_| {
                let accounts = accounts.clone();
                let id = id.clone();
                pending.set(true);
                spawn(async move {
                    match change(async move { accounts.delete(&id).await }).await {
                        Ok(_) => on_close.call(()),
                        Err(report) => failure.set(Some(report)),
                    }
                    pending.set(false);
                });
            },
            on_close: move |_| on_close.call(()),
            p {
                {t!(
                    "{name}'s collections go with it. Games, installs and play history stay.",
                    name = account.name.clone()
                )}
            }
            if let Some(report) = failure() {
                p { role: "alert", class: "text-danger", {sentence(&report)} }
            }
        }
    }
}
