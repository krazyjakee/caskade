//! First-run setup's last question (#207), as the webview's
//! `AccountStep.tsx`: the admin's name, avatar and an optional PIN. The PIN
//! can stay empty while the admin is the only account; it goes straight to
//! the core and is never kept here.

use caskade_core::error::ErrorReport;
use caskade_ui_data::accounts::{Account, AccountUpdate};
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;

use super::{Continue, StepHeading};
use crate::data::change;
use crate::flows::pin_pad::valid_pin;
use crate::ui::{AvatarPicker, AvatarSize, TextField};

/// What the library's first admin is called until setup names them.
const DEFAULT_NAME: &str = "Player";

#[component]
pub fn AccountStep(account: Account, on_next: EventHandler<()>) -> Element {
    let accounts = crate::accounts::use_accounts();
    // The first admin's stored name is English; offer it in the user's
    // language until they change it.
    let mut name = use_signal(|| {
        if account.name == DEFAULT_NAME {
            tp!("account name", "Player")
        } else {
            account.name.clone()
        }
    });
    let mut avatar = use_signal(|| account.avatar.clone());
    let mut pin = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut failed = use_signal(|| None::<ErrorReport>);
    let named = !name().trim().is_empty();
    let pin_ok = pin().is_empty() || valid_pin(&pin());
    let save = {
        let id = account.id.clone();
        move |_| {
            if !named || !pin_ok || pending() {
                return;
            }
            let accounts = accounts.clone();
            let id = id.clone();
            let update = AccountUpdate {
                name: name.peek().trim().to_owned(),
                avatar: avatar.peek().clone(),
                access: None,
            };
            let typed = Some(pin.peek().clone()).filter(|pin| !pin.is_empty());
            pending.set(true);
            failed.set(None);
            spawn(async move {
                let saved = change(async move {
                    accounts.update(&id, update).await?;
                    if let Some(pin) = typed {
                        accounts.set_pin(&id, Some(pin)).await?;
                    }
                    Ok(())
                })
                .await;
                pending.set(false);
                match saved {
                    Ok(()) => on_next.call(()),
                    Err(error) => failed.set(Some(error)),
                }
            });
        }
    };
    let pin_help = if account.has_pin {
        t!("Your account has a PIN. Type a new one to change it.")
    } else {
        t!("Optional while you're the only account: 4 to 8 digits that lock it.")
    };
    rsx! {
        section { "data-part": "question", "data-question": "account",
            StepHeading {
                title: t!("Who are you?"),
                description: t!("You're the admin: you'll add the others' accounts and choose what they can do."),
            }
            div { class: "flex animate-rise flex-col gap-6 [--i:2]",
                TextField {
                    id: "onboarding-name",
                    label: tp!("account", "Name"),
                    value: name(),
                    // Marked invalid while empty, without a message, as the
                    // webview's field.
                    error: (!named).then(String::new),
                    on_input: move |next: String| name.set(next.chars().take(64).collect()),
                }
                div { class: "flex flex-col gap-2",
                    span { id: "onboarding-avatar", class: "text-sm font-medium", {t!("Avatar")} }
                    AvatarPicker {
                        "aria-labelledby": "onboarding-avatar",
                        value: Some(avatar()),
                        on_change: move |next: String| avatar.set(next),
                        size: AvatarSize::Md,
                    }
                }
                TextField {
                    id: "onboarding-pin",
                    label: t!("PIN"),
                    kind: "password",
                    value: pin(),
                    description: pin_help,
                    error: (!pin_ok).then(String::new),
                    on_input: move |next: String| {
                        pin.set(next.chars().filter(char::is_ascii_digit).take(8).collect())
                    },
                }
                if let Some(error) = failed() {
                    p { role: "alert", class: "text-center text-sm text-danger", "{error.summary}" }
                }
            }
            Continue {
                disabled: !named || !pin_ok || pending(),
                on_press: save,
                label: t!(
                    note = "Finishes first-run setup's questions and starts the work",
                    "Set up"
                ),
            }
        }
    }
}
