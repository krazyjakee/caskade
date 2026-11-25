//! The dialog that adds or edits an account: name, avatar, role, PIN,
//! permissions and age limit.

use caskade_core::error::ErrorReport;
use caskade_ui_data::accounts::{
    Account, AccountAccess, AccountRole, AccountUpdate, NewAccount, Permissions,
};
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;

use super::super::pin::{PinSetting, SetPin, sentence};
use super::{NO_LIMIT, age_limit_options, own_check, pin_setter, use_accounts};
use crate::data::change;
use crate::ui::icons::{DOWNLOAD, SHIELD_CHECK, SHIELD_QUESTION_MARK, USER, USER_COG};
use crate::ui::{
    AvatarPicker, AvatarSize, Button, ButtonVariant, DEFAULT_AVATAR, Dialog, Segment,
    SegmentedControl, Select, SettingRow, Switch, TextField,
};

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
            tp!("settings tab", "Discovery"),
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
pub fn AccountEditor(
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
