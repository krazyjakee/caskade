//! Who is playing (#207), as the webview's `AccountPicker`: the accounts to
//! sign in as, in place of the shell at start (when there's more than one
//! account, or the only one has a PIN) and after Switch account. A locked
//! account asks for its PIN on the PIN pad. Focus starts on the account
//! that signed in last here.

use caskade_ui_data::accounts::{Account, AccountList, PinCheck};
use caskade_ui_i18n::{t, tp};
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use super::{last, use_accounts};
use crate::data::change;
use crate::flows::pin_pad::{Check, PinPad};
use crate::input::{Bind, use_input_action};
use crate::ui::icons::{ARROW_LEFT, LOCK};
use crate::ui::interact::{Keys, flag, use_interaction};
use crate::ui::{Avatar, AvatarSize, Button, Svg};

#[component]
pub fn AccountPicker(list: AccountList, on_signed_in: EventHandler<Account>) -> Element {
    let accounts = use_accounts();
    let mut chosen = use_signal(|| None::<String>);
    let mut error = use_signal(|| None::<String>);
    let mut pending = use_signal(|| false);
    let mut root = use_signal(|| None::<blitz_dom::NodeId>);
    // The list refreshes as accounts change (a lock starts, one is deleted).
    let asking = chosen().and_then(|id| {
        list.accounts
            .iter()
            .find(|account| account.id == id)
            .cloned()
    });
    let last = use_hook(last::last_account);
    let start = list
        .accounts
        .iter()
        .find(|account| Some(&account.id) == last.as_ref())
        .or(list.accounts.first())
        .map(|account| account.id.clone());
    let signed_in = move |account: Account| {
        last::remember(&account.id);
        on_signed_in.call(account);
    };
    let choose = {
        let accounts = accounts.clone();
        move |account: Account| {
            error.set(None);
            if account.has_pin {
                chosen.set(Some(account.id.clone()));
                return;
            }
            let accounts = accounts.clone();
            pending.set(true);
            spawn(async move {
                let id = account.id.clone();
                let answer = change(async move { accounts.sign_in(&id, None).await }).await;
                pending.set(false);
                match answer {
                    Ok(PinCheck::Matched) => signed_in(account),
                    Ok(_) => {}
                    Err(report) => error.set(Some(report.summary)),
                }
            });
        }
    };
    // B leaves the PIN pad (which deletes digits first); on the accounts it
    // does nothing: there's nowhere behind them.
    use_input_action(
        Action::Back,
        Bind {
            label: asking.as_ref().map(|_| t!("All accounts")),
            owner: root(),
            ..Bind::default()
        },
        move || {
            chosen.set(None);
            true
        },
    );
    use_input_action(
        Action::Confirm,
        Bind {
            label: Some(tp!(
                note = "Hint bar: confirm the focused item",
                "pad action",
                "Select"
            )),
            owner: root(),
            ..Bind::default()
        },
        || false,
    );
    rsx! {
        div {
            "data-cs": "account-picker",
            "data-step": if asking.is_some() { "pin" } else { "accounts" },
            class: "relative flex h-screen flex-col overflow-hidden",
            onmounted: move |event| {
                if let Some(handle) = event.data().downcast::<NodeHandle>() {
                    root.set(Some(handle.node_id()));
                }
            },
            div { "data-cs": "backdrop", "aria-hidden": "true" }
            header {
                "data-part": "titlebar",
                "data-window-drag": "true",
                class: "relative z-10 flex h-topbar shrink-0 items-center justify-end gap-2 pr-4 pl-6",
                crate::frame::window::WindowControls {}
            }
            main {
                "data-focus-section": "main",
                class: "relative z-10 flex min-h-0 flex-1 flex-col items-center overflow-y-auto px-6 pb-10",
                if let Some(account) = asking.clone() {
                    section { key: "{account.id}", class: "my-auto flex w-full animate-enter flex-col items-center gap-6",
                        Button {
                            "data-part": "back",
                            class: "self-center",
                            icon: ARROW_LEFT,
                            on_press: move |_| chosen.set(None),
                            {t!("All accounts")}
                        }
                        Avatar { part: "avatar", avatar: Some(account.avatar.clone()), name: account.name.clone(), size: AvatarSize::Lg }
                        PinPad {
                            title: t!("Enter {name}'s PIN", name = account.name.clone()),
                            locked_until_ms: account.locked_until_ms,
                            check: {
                                let accounts = accounts.clone();
                                let id = account.id.clone();
                                Check::new(move |pin: String| {
                                    let accounts = accounts.clone();
                                    let id = id.clone();
                                    async move {
                                        change(async move { accounts.sign_in(&id, Some(pin)).await })
                                            .await
                                            .map_err(|report| report.summary)
                                    }
                                })
                            },
                            on_matched: {
                                let account = account.clone();
                                move |_| signed_in(account.clone())
                            },
                            on_cancel: move |_| chosen.set(None),
                        }
                    }
                } else {
                    section { class: "my-auto flex w-full max-w-5xl flex-col items-center gap-10",
                        h1 {
                            "data-part": "heading",
                            class: "animate-enter font-display text-3xl font-semibold tracking-wide",
                            {t!("Who's playing?")}
                        }
                        ul {
                            "data-part": "accounts",
                            "aria-label": t!("Accounts"),
                            class: "flex flex-wrap justify-center gap-6",
                            for (index, account) in list.accounts.iter().cloned().enumerate() {
                                li { key: "{account.id}", class: "animate-enter", style: "--i: {index}",
                                    AccountTile {
                                        preferred: Some(&account.id) == start.as_ref(),
                                        disabled: pending(),
                                        on_press: {
                                            let mut choose = choose.clone();
                                            let account = account.clone();
                                            move |_| choose(account.clone())
                                        },
                                        account,
                                    }
                                }
                            }
                        }
                        if let Some(reason) = error() {
                            p { role: "alert", class: "text-sm text-danger", "{reason}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn AccountTile(
    account: Account,
    preferred: bool,
    disabled: bool,
    on_press: EventHandler<()>,
) -> Element {
    let label = if account.has_pin {
        t!("{name}, PIN locked", name = account.name.clone())
    } else {
        account.name.clone()
    };
    let interaction = use_interaction();
    let press = move || {
        if !disabled {
            on_press.call(());
        }
    };
    rsx! {
        button {
            r#type: "button",
            "data-part": "account",
            "data-status": account.has_pin.then_some("locked"),
            "data-focus-default": preferred.then_some("true"),
            "data-spatial": "focusable",
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-pressed": interaction.pressed(),
            "data-disabled": flag(disabled),
            "aria-disabled": flag(disabled),
            "aria-label": label,
            class: "group relative flex w-36 flex-col items-center gap-3 rounded-card p-3 outline-none transition-[background-color,scale] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:bg-surface-hover data-pressed:scale-[0.97]",
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onclick: move |_| press(),
            onkeydown: move |event: KeyboardEvent| {
                interaction.key_down(&event, Keys::EnterAndSpace, press);
            },
            onkeyup: move |event| interaction.key_up(&event, press),
            span { class: "relative",
                Avatar { part: "avatar", avatar: Some(account.avatar.clone()), name: account.name.clone(), size: AvatarSize::Lg }
                if account.has_pin {
                    span {
                        "data-part": "lock",
                        "aria-hidden": "true",
                        class: "absolute right-0 bottom-0 grid size-7 place-items-center rounded-full border border-border-strong bg-surface-overlay text-fg-muted",
                        Svg { icon: LOCK, class: "size-3.5" }
                    }
                }
            }
            span { "data-part": "name", class: "max-w-full truncate font-medium text-fg-muted group-data-focus-visible:text-fg group-data-hovered:text-fg",
                "{account.name}"
            }
        }
    }
}
