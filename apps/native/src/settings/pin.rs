//! An account's PIN row (#208, #105), as the webview's `PinSetting` and
//! `PinDialog`: set, change or remove it on the shared PIN pad, so a
//! controller can too. An account changing its own PIN enters the current
//! one first, then the new one twice; removing your own asks only for the
//! current one, and another account's asks first. A PIN lives only in the
//! dialog until it goes to the core.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use caskade_core::error::ErrorReport;
use caskade_ui_data::accounts::PinCheck;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use crate::flows::pin_pad::{Check, PinPad};
use crate::ui::icons::KEY_ROUND;
use crate::ui::{Button, ButtonSize, ButtonVariant, ConfirmDialog, Dialog, SettingRow};

/// The core's reason as a sentence: "Every admin needs a PIN…."
pub fn sentence(report: &ErrorReport) -> String {
    let text = report.summary.trim();
    let mut chars = text.chars();
    let capital: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    if capital.ends_with(['.', '!', '?']) {
        capital
    } else {
        format!("{capital}.")
    }
}

/// A PIN saved or removed, later; why it was refused.
pub type Saving = Pin<Box<dyn Future<Output = Result<(), String>>>>;

/// Stores a new PIN, or removes it with `None`; why it was refused.
#[derive(Clone)]
pub struct SetPin(pub Rc<dyn Fn(Option<String>) -> Saving>);

impl SetPin {
    pub fn new<F, Fut>(set: F) -> Self
    where
        F: Fn(Option<String>) -> Fut + 'static,
        Fut: Future<Output = Result<(), String>> + 'static,
    {
        Self(Rc::new(move |pin| Box::pin(set(pin))))
    }
}

impl PartialEq for SetPin {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Open {
    Choose,
    Remove,
    ConfirmRemove,
}

/// The PIN row of `name`'s account.
#[component]
pub fn PinSetting(
    name: String,
    has_pin: bool,
    #[props(default)] locked_until_ms: Option<i64>,
    /// Checks the current PIN, for an account changing its own.
    #[props(default)]
    own: Option<Check>,
    set: SetPin,
    #[props(default)] description: Option<String>,
) -> Element {
    let mut open = use_signal(|| None::<Open>);
    let mut error = use_signal(|| None::<String>);
    let mut removing = use_signal(|| false);
    let description = description.unwrap_or_else(|| {
        if has_pin {
            t!("A PIN locks this account.")
        } else {
            t!("No PIN: anyone can sign in to it.")
        }
    });
    // Your own PIN goes once you enter it; another account's asks first, as
    // it opens that account to anyone at once (#49).
    let own_check = own.clone();
    let remove = move |_| {
        error.set(None);
        open.set(Some(if own_check.is_some() {
            Open::Remove
        } else {
            Open::ConfirmRemove
        }));
    };
    let confirm_remove = {
        let set = set.clone();
        move |_| {
            removing.set(true);
            let set = set.clone();
            spawn(async move {
                match (set.0)(None).await {
                    Ok(()) => {
                        error.set(None);
                        open.set(None);
                    }
                    Err(reason) => error.set(Some(reason)),
                }
                removing.set(false);
            });
        }
    };
    let save = {
        let set = set.clone();
        SetPin::new(move |pin| {
            let set = set.clone();
            async move {
                let saved = (set.0)(pin).await;
                if saved.is_ok() {
                    error.set(None);
                }
                saved
            }
        })
    };
    let shown = open();
    rsx! {
        SettingRow { label: t!("PIN"), icon: Some(KEY_ROUND), description,
            div { class: "flex flex-wrap justify-end gap-2",
                Button {
                    size: ButtonSize::Sm,
                    on_press: move |_| open.set(Some(Open::Choose)),
                    if has_pin {
                        {t!("Change PIN…")}
                    } else {
                        {t!("Set PIN…")}
                    }
                }
                if has_pin {
                    Button {
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Ghost,
                        on_press: remove,
                        {t!("Remove PIN…")}
                    }
                }
            }
        }
        if let Some(reason) = error().filter(|_| shown != Some(Open::ConfirmRemove)) {
            p { role: "alert", class: "pb-2 text-sm text-danger", "{reason}" }
        }
        if shown == Some(Open::ConfirmRemove) {
            ConfirmDialog {
                title: t!("Remove {name}'s PIN?", name = name.clone()),
                confirm_label: t!("Remove PIN"),
                pending_label: Some(t!("Removing…")),
                pending: removing(),
                on_confirm: confirm_remove,
                on_close: move |_| {
                    error.set(None);
                    open.set(None);
                },
                p {
                    {t!(
                        "Anyone can then sign in to {name} without a PIN. You can set a new one later.",
                        name = name.clone()
                    )}
                }
                if let Some(reason) = error() {
                    p { role: "alert", class: "text-danger", "{reason}" }
                }
            }
        }
        if matches!(shown, Some(Open::Choose | Open::Remove)) {
            PinDialog {
                name: name.clone(),
                current: if has_pin { own.clone() } else { None },
                locked_until_ms,
                remove: shown == Some(Open::Remove),
                save,
                on_close: move |_| open.set(None),
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Current,
    New,
    Again,
}

/// Choosing a PIN on the pad: the current PIN first when `current` is
/// given, then the new one twice; with `remove`, only the current one.
#[component]
fn PinDialog(
    /// Whose PIN: "Ada".
    name: String,
    current: Option<Check>,
    locked_until_ms: Option<i64>,
    remove: bool,
    save: SetPin,
    on_close: EventHandler<()>,
) -> Element {
    let mut step = use_signal(|| {
        if current.is_some() {
            Step::Current
        } else {
            Step::New
        }
    });
    let mut first = use_signal(String::new);
    let title = match step() {
        Step::Current => t!("Enter {name}'s current PIN", name = name.clone()),
        Step::New => t!("Choose a new PIN for {name}", name = name.clone()),
        Step::Again => t!("Enter the new PIN again"),
    };
    let check = use_hook(|| {
        let current = current.clone();
        let save = save.clone();
        Check::new(move |pin: String| {
            let current = current.clone();
            let save = save.clone();
            async move {
                match *step.peek() {
                    Step::Current => {
                        let answer = match &current {
                            Some(current) => (current.0)(pin).await?,
                            None => PinCheck::Matched,
                        };
                        if answer == PinCheck::Matched && remove {
                            (save.0)(None).await?;
                        }
                        Ok(answer)
                    }
                    Step::New => {
                        first.set(pin);
                        Ok(PinCheck::Matched)
                    }
                    Step::Again => {
                        if pin != *first.peek() {
                            return Err(t!("The PINs don't match. Try again."));
                        }
                        (save.0)(Some(pin)).await?;
                        Ok(PinCheck::Matched)
                    }
                }
            }
        })
    });
    let next = move |_| match step() {
        Step::Current if !remove => step.set(Step::New),
        Step::New => step.set(Step::Again),
        _ => on_close.call(()),
    };
    let shown = step();
    let step_key = shown as u8;
    rsx! {
        Dialog {
            title: title.clone(),
            on_close: move |_| on_close.call(()),
            class: "max-w-sm",
            div { class: "grid place-items-center",
                PinPad {
                    key: "{step_key}",
                    title,
                    show_title: false,
                    check,
                    on_matched: next,
                    on_cancel: move |_| on_close.call(()),
                    locked_until_ms: if shown == Step::Current { locked_until_ms } else { None },
                }
            }
        }
    }
}
