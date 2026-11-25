//! Settings → My profile: the signed-in account's own name, avatar and PIN.

use std::rc::Rc;

use caskade_core::error::ErrorReport;
use caskade_ui_data::accounts::{Account, AccountRole, AccountUpdate};
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;

use super::super::pin::{PinSetting, sentence};
use super::super::unsaved::{UnsavedForm, UnsavedHint, use_unsaved_changes};
use super::{own_check, pin_setter, use_accounts};
use crate::accounts::use_account_list;
use crate::data::change;
use crate::ui::icons::USER_ROUND;
use crate::ui::{AvatarPicker, AvatarSize, Button, ButtonVariant, SettingsSection, TextField};

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
