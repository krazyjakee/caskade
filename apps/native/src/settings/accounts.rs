//! Settings → Accounts and My profile (#208, #105), as the webview's
//! `settings/accounts/`. Accounts, for admins: every account with Edit and
//! Delete, Add account, and the rating board every account's ratings and
//! age limits use; the editor sets an account's name, avatar, role, PIN,
//! what it may change and its age limit. My profile, for every account:
//! its own name, avatar and PIN. The core keeps every rule (an admin
//! remains; every admin has a PIN while standard accounts exist) and refuses
//! changes an account may not make whatever this shows; a refusal shows its
//! reason.

use caskade_core::error::ErrorReport;
use caskade_ui_data::accounts::{
    Account, AccountAccess, AccountRole, AccountUpdate, Accounts, NewAccount, Permissions,
};
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;
use serde_json::Value;
use std::rc::Rc;

use super::pin::{PinSetting, SetPin, sentence};
use super::state::use_settings_section;
use super::sync::SectionSync;
use super::unsaved::{UnsavedForm, UnsavedHint, use_unsaved_changes};
use crate::accounts::use_account_list;
use crate::data::{change, use_settings};
use crate::flows::pin_pad::Check;
use crate::game::{BOARDS, DEFAULT_BOARD, board_name};
use crate::ui::icons::{
    DOWNLOAD, PENCIL, PLUS, SHIELD_CHECK, SHIELD_QUESTION_MARK, TRASH, USER, USER_COG, USER_ROUND,
    USERS,
};
use crate::ui::{
    Avatar, AvatarPicker, AvatarSize, Button, ButtonSize, ButtonVariant, ConfirmDialog,
    DEFAULT_AVATAR, Dialog, FailureNotice, Segment, SegmentedControl, Select, SelectOption,
    SettingRow, SettingsSection, Switch, TextField,
};

/// The settings section that holds the preferred rating board.
const ACCOUNTS_SETTINGS: &str = "core.accounts";
const NO_LIMIT: &str = "none";

fn use_accounts() -> Accounts {
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

/// Each board's labels and the minimum age each allows, as the core maps
/// them (`caskade_core::igdb::age`); labels that come to the same age are
/// one step.
fn steps(board: &str) -> &'static [(&'static str, u8)] {
    match board {
        "pegi" => &[("3", 3), ("7", 7), ("12", 12), ("16", 16), ("18", 18)],
        "esrb" => &[
            ("EC", 3),
            ("E", 6),
            ("E10+", 10),
            ("T", 13),
            ("M", 17),
            ("AO", 18),
        ],
        "usk" => &[("0", 0), ("6", 6), ("12", 12), ("16", 16), ("18", 18)],
        "cero" => &[("A", 0), ("B", 12), ("C", 15), ("D", 17), ("Z", 18)],
        "acb" => &[("G", 0), ("PG", 8), ("M", 15), ("R18+", 18)],
        "grac" => &[("All", 0), ("12", 12), ("15", 15), ("18", 18)],
        "classind" => &[
            ("L", 0),
            ("10", 10),
            ("12", 12),
            ("14", 14),
            ("16", 16),
            ("18", 18),
        ],
        _ => &[],
    }
}

/// The age limit picker's options: no limit, then `board`'s steps, and
/// `current` when it is none of them (set under another board).
fn age_limit_options(board: &str, current: Option<u8>) -> Vec<SelectOption> {
    let known = if steps(board).is_empty() {
        DEFAULT_BOARD
    } else {
        board
    };
    let name = board_name(known);
    let mut options = vec![SelectOption::new(NO_LIMIT, tp!("age limit", "No limit"))];
    options.extend(steps(known).iter().map(|(label, age)| {
        let text = if label.bytes().all(|byte| byte.is_ascii_digit()) {
            format!("{name} {label}")
        } else {
            format!("{name} {label} ({age}+)")
        };
        SelectOption::new(age.to_string(), text)
    }));
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
fn own_check(accounts: &Accounts, id: &str) -> Check {
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
fn pin_setter(accounts: &Accounts, id: &str) -> SetPin {
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

/// The Settings sections a standard account may be allowed to change
/// (`caskade_core::accounts::SETTINGS_SECTIONS`), with the tabs' own
/// messages and what each covers.
fn permission_sections() -> [(&'static str, String, String); 8] {
    [
        (
            "general",
            tp!("settings tab", "General"),
            t!("Language, startup, closing and updates."),
        ),
        (
            "library-discovery",
            tp!("settings tab", "Library Discovery"),
            t!("What scans look for, their folders and ignored results."),
        ),
        (
            "launchers",
            tp!("settings tab", "Launchers"),
            t!("Which launchers' games join the library."),
        ),
        (
            "emulators",
            tp!("settings tab", "Emulation"),
            t!("Emulators, ROM scanners, BIOS files and RomM servers."),
        ),
        (
            "appearance",
            tp!("settings tab", "Appearance"),
            t!("Theme, look, motion and custom CSS."),
        ),
        (
            "storage",
            tp!("settings tab", "Storage"),
            t!("Where games install, and the default location."),
        ),
        (
            "installer",
            tp!("settings tab", "Installer"),
            t!("How games download and install, and torrent links."),
        ),
        (
            "plugins",
            tp!("settings tab", "Plugins"),
            t!("Plugins and add-on themes, and what they may do."),
        ),
    ]
}

fn access_of(account: &Account) -> AccountAccess {
    AccountAccess {
        role: account.role,
        permissions: account.permissions.clone(),
        max_age: account.max_age,
        unrated_needs_pin: account.unrated_needs_pin,
    }
}

fn new_access() -> AccountAccess {
    AccountAccess {
        role: AccountRole::Standard,
        permissions: Permissions {
            settings: Vec::new(),
            install: false,
        },
        max_age: None,
        unrated_needs_pin: false,
    }
}

/// Adds an account, or changes `account`.
#[component]
fn AccountEditor(
    account: Option<Account>,
    active_id: Option<String>,
    board: String,
    on_close: EventHandler<()>,
) -> Element {
    let accounts = use_accounts();
    let initial = use_hook({
        let account = account.clone();
        move || account.as_ref().map_or_else(new_access, access_of)
    });
    let mut name = use_signal(|| account.as_ref().map(|a| a.name.clone()).unwrap_or_default());
    let mut avatar = use_signal(|| {
        account
            .as_ref()
            .map_or_else(|| DEFAULT_AVATAR.to_owned(), |a| a.avatar.clone())
    });
    let mut access = use_signal(|| initial.clone());
    // A new account's PIN waits here until it is created.
    let draft_pin = use_signal(|| None::<String>);
    let draft_setter = use_hook(|| {
        SetPin::new(move |pin: Option<String>| {
            let mut draft = draft_pin;
            draft.set(pin);
            async { Ok(()) }
        })
    });
    let mut busy = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    // With changes, only Cancel or Save closes it: Escape or a click beside
    // it would drop them without a word (#55).
    let dirty = name() != account.as_ref().map(|a| a.name.clone()).unwrap_or_default()
        || avatar()
            != account
                .as_ref()
                .map_or_else(|| DEFAULT_AVATAR.to_owned(), |a| a.avatar.clone())
        || access() != initial
        || draft_pin().is_some();
    let named = !name().trim().is_empty();
    let current = access();
    let standard = current.role == AccountRole::Standard;
    let mut submit = {
        let accounts = accounts.clone();
        let account = account.clone();
        move || {
            if !named || busy() {
                return;
            }
            let mut sent = access();
            // An admin may change everything; age limits are for standard accounts.
            if sent.role == AccountRole::Admin {
                sent.max_age = None;
                sent.unrated_needs_pin = false;
            }
            let accounts = accounts.clone();
            let id = account.as_ref().map(|a| a.id.clone());
            let (name, avatar, pin) = (name().trim().to_owned(), avatar(), draft_pin());
            busy.set(true);
            failure.set(None);
            spawn(async move {
                let saved = change(async move {
                    match id {
                        Some(id) => accounts
                            .update(
                                &id,
                                AccountUpdate {
                                    name,
                                    avatar,
                                    access: Some(sent),
                                },
                            )
                            .await
                            .map(|_| ()),
                        None => accounts
                            .create(NewAccount {
                                name,
                                avatar,
                                access: sent,
                                pin,
                            })
                            .await
                            .map(|_| ()),
                    }
                })
                .await;
                busy.set(false);
                match saved {
                    Ok(()) => on_close.call(()),
                    Err(report) => failure.set(Some(report)),
                }
            });
        }
    };
    let title = match &account {
        Some(account) => t!("Edit {name}", name = account.name.clone()),
        None => t!("Add account"),
    };
    let roles = vec![
        Segment {
            icon: Some(USER),
            ..Segment::new("standard", tp!("account role", "Standard"))
        },
        Segment {
            icon: Some(USER_COG),
            ..Segment::new("admin", tp!("account role", "Admin"))
        },
    ];
    let pin_row = match &account {
        Some(account) => {
            let own = active_id.as_deref() == Some(account.id.as_str());
            rsx! {
                PinSetting {
                    name: account.name.clone(),
                    has_pin: account.has_pin,
                    locked_until_ms: account.locked_until_ms,
                    own: own.then(|| own_check(&accounts, &account.id)),
                    set: pin_setter(&accounts, &account.id),
                    description: if account.has_pin {
                        t!("A PIN locks this account. Changes at once.")
                    } else {
                        t!("No PIN: anyone can sign in to it. Changes at once.")
                    },
                }
            }
        }
        None => {
            let shown = name().trim().to_owned();
            rsx! {
                PinSetting {
                    name: if shown.is_empty() {
                        t!(
                            note = "Fills in for a new account's name: 'Choose a new PIN for the account'",
                            "the account"
                        )
                    } else {
                        shown
                    },
                    has_pin: draft_pin().is_some(),
                    set: draft_setter,
                }
            }
        }
    };
    let save_label = if busy() {
        t!("Saving…")
    } else if account.is_some() {
        t!("Save")
    } else {
        t!(note = "Adds the new account", "Add")
    };
    let mut submit_key = submit.clone();
    rsx! {
        Dialog {
            title,
            on_close: move |_| {
                if !busy() && !dirty {
                    on_close.call(());
                }
            },
            class: "max-h-[90vh] max-w-2xl overflow-auto",
            "data-cs": "account-editor",
            div { class: "flex flex-col gap-4",
                TextField {
                    id: "account-name",
                    label: tp!("account", "Name"),
                    value: name(),
                    required: true,
                    disabled: busy(),
                    "data-focus-default": "true",
                    on_input: move |next: String| name.set(next.chars().take(64).collect()),
                    on_submit: move |_| submit_key(),
                }
                div { "data-part": "avatars", class: "flex flex-col gap-2",
                    span { class: "text-sm font-medium", {t!("Avatar")} }
                    AvatarPicker {
                        value: Some(avatar()),
                        on_change: move |next| avatar.set(next),
                        size: AvatarSize::Md,
                        disabled: busy(),
                    }
                }
                div {
                    SettingRow {
                        icon: Some(USER_COG),
                        label: tp!("account", "Role"),
                        description: t!("An admin changes everything, other accounts included."),
                        SegmentedControl {
                            options: roles,
                            value: if standard { "standard" } else { "admin" },
                            disabled: busy(),
                            on_change: move |role: String| {
                                access.write().role = if role == "admin" { AccountRole::Admin } else { AccountRole::Standard };
                            },
                        }
                    }
                    {pin_row}
                }
                if standard {
                    fieldset { "data-part": "access", class: "flex flex-col",
                        legend { class: "mb-1 text-sm font-medium",
                            {t!(note = "Over a standard account's permissions", "What it can do")}
                        }
                        SettingRow {
                            icon: Some(DOWNLOAD),
                            label: t!("Install games"),
                            description: t!("Install, uninstall and move games. Updates run for everyone."),
                            Switch {
                                checked: current.permissions.install,
                                disabled: busy(),
                                on_change: move |install: bool| access.write().permissions.install = install,
                            }
                        }
                        p { class: "mt-3 text-sm font-medium",
                            {t!(note = "Over the Settings sections a standard account may change", "Settings sections it can change")}
                        }
                        for (section, label, note) in permission_sections() {
                            SettingRow { key: "{section}", label, description: note,
                                Switch {
                                    checked: current.permissions.settings.iter().any(|id| id == section),
                                    disabled: busy(),
                                    on_change: move |on: bool| {
                                        let mut access = access.write();
                                        access.permissions.settings.retain(|id| id != section);
                                        if on {
                                            access.permissions.settings.push(section.to_owned());
                                        }
                                    },
                                }
                            }
                        }
                        SettingRow {
                            icon: Some(SHIELD_CHECK),
                            label: t!("Maximum age rating"),
                            description: t!("Games rated above it need an admin's PIN."),
                            Select {
                                id: "account-max-age",
                                options: age_limit_options(&board, current.max_age),
                                value: Some(current.max_age.map_or_else(|| NO_LIMIT.to_owned(), |age| age.to_string())),
                                disabled: busy(),
                                class: "w-52",
                                on_change: move |id: String| access.write().max_age = id.parse().ok(),
                            }
                        }
                        SettingRow {
                            icon: Some(SHIELD_QUESTION_MARK),
                            label: t!("Unrated games need a PIN"),
                            description: t!("Under an age limit, games without a rating need an admin's PIN too."),
                            Switch {
                                checked: current.unrated_needs_pin,
                                disabled: busy() || current.max_age.is_none(),
                                on_change: move |on: bool| access.write().unrated_needs_pin = on,
                            }
                        }
                    }
                }
                if let Some(report) = failure() {
                    p { role: "alert", class: "text-sm text-danger", {sentence(&report)} }
                }
                div { class: "flex justify-end gap-2",
                    Button { disabled: busy(), on_press: move |_| on_close.call(()), {t!("Cancel")} }
                    Button {
                        id: "account-save",
                        variant: ButtonVariant::Primary,
                        disabled: !named || busy(),
                        on_press: move |_| submit(),
                        "{save_label}"
                    }
                }
            }
        }
    }
}

/// Settings → My profile.
#[component]
pub fn ProfileSettings() -> Element {
    let list = use_account_list();
    let account = match &*list.read() {
        None => None,
        Some(Ok(list)) => Some(
            list.active_id
                .as_ref()
                .and_then(|id| list.accounts.iter().find(|account| &account.id == id))
                .cloned(),
        ),
        Some(Err(_)) => Some(None),
    };
    match account {
        None => rsx! {
            p { class: "text-fg-muted", {t!("Loading your profile…")} }
        },
        Some(None) => rsx! {
            p { class: "text-fg-muted", {t!("Sign in to change your profile.")} }
        },
        Some(Some(account)) => rsx! {
            div { "data-settings": "profile", class: "max-w-3xl",
                Profile { key: "{account.id}", account }
            }
        },
    }
}

#[component]
fn Profile(account: Account) -> Element {
    let accounts = use_accounts();
    let mut name = use_signal(|| account.name.clone());
    let mut avatar = use_signal(|| account.avatar.clone());
    let mut saving = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    // Saved elsewhere (another window, an admin): show what was saved.
    let (saved_name, saved_avatar) = (account.name.clone(), account.avatar.clone());
    use_effect(use_reactive!(|saved_name| name.set(saved_name)));
    use_effect(use_reactive!(|saved_avatar| avatar.set(saved_avatar)));
    let named = !name().trim().is_empty();
    let changed = name().trim() != account.name || avatar() != account.avatar;
    let save = {
        let accounts = accounts.clone();
        let id = account.id.clone();
        Rc::new(move || {
            let accounts = accounts.clone();
            let id = id.clone();
            let (next, picked) = (name.peek().trim().to_owned(), avatar.peek().clone());
            Box::pin(async move {
                if next.is_empty() {
                    return false;
                }
                saving.set(true);
                failure.set(None);
                let saved = change(async move {
                    accounts
                        .update(
                            &id,
                            AccountUpdate {
                                name: next,
                                avatar: picked,
                                access: None,
                            },
                        )
                        .await
                })
                .await;
                saving.set(false);
                match saved {
                    Ok(_) => true,
                    Err(report) => {
                        failure.set(Some(report));
                        false
                    }
                }
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = bool>>>
        })
    };
    // Name and avatar wait for Save (#43): leaving the section asks first.
    use_unsaved_changes(
        changed,
        UnsavedForm {
            label: tp!("settings tab", "My profile"),
            save: save.clone(),
            discard: Rc::new({
                let (saved_name, saved_avatar) = (account.name.clone(), account.avatar.clone());
                move || {
                    let (mut name, mut avatar) = (name, avatar);
                    name.set(saved_name.clone());
                    avatar.set(saved_avatar.clone());
                }
            }),
        },
    );
    let submit = {
        let save = save.clone();
        move || {
            if named && changed && !saving() {
                // A failure shows below the fields.
                let save = save.clone();
                spawn(async move {
                    save().await;
                });
            }
        }
    };
    let submit_key = submit.clone();
    let submit_button = submit;
    let description = if account.role == AccountRole::Admin {
        t!(
            "Signed in as {name}, an admin.",
            name = account.name.clone()
        )
    } else {
        t!(
            "Signed in as {name}, a standard account.",
            name = account.name.clone()
        )
    };
    rsx! {
        SettingsSection {
            icon: Some(USER_ROUND),
            title: tp!("settings tab", "My profile"),
            description,
            div { "data-cs": "account-profile", class: "flex flex-col gap-4 pt-2",
                div { class: "flex flex-col gap-4",
                    TextField {
                        id: "profile-name",
                        label: tp!("account", "Name"),
                        value: name(),
                        required: true,
                        disabled: saving(),
                        on_input: move |next: String| name.set(next.chars().take(64).collect()),
                        on_submit: move |_| submit_key(),
                    }
                    div { "data-part": "avatars", class: "flex flex-col gap-2",
                        span { class: "text-sm font-medium", {t!("Avatar")} }
                        AvatarPicker {
                            value: Some(avatar()),
                            on_change: move |next| avatar.set(next),
                            size: AvatarSize::Md,
                            disabled: saving(),
                        }
                    }
                    if let Some(report) = failure() {
                        p { role: "alert", class: "text-sm text-danger", {sentence(&report)} }
                    }
                    div { class: "flex items-center justify-end gap-3",
                        UnsavedHint { dirty: changed }
                        Button {
                            id: "profile-save",
                            variant: ButtonVariant::Primary,
                            disabled: !named || !changed || saving(),
                            on_press: move |_| submit_button(),
                            if saving() {
                                {t!("Saving…")}
                            } else {
                                {t!("Save")}
                            }
                        }
                    }
                }
                div { "data-part": "pin",
                    PinSetting {
                        name: account.name.clone(),
                        has_pin: account.has_pin,
                        locked_until_ms: account.locked_until_ms,
                        own: Some(own_check(&accounts, &account.id)),
                        set: pin_setter(&accounts, &account.id),
                    }
                }
            }
        }
    }
}
