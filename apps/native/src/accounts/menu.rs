//! The signed-in account's avatar in the top bar (#207), as the webview's
//! `AccountMenu`: it opens a menu that says who is signed in and offers My
//! profile (Settings → My profile) and Switch account, which signs out to
//! the account picker after asking about unsaved Settings edits. Not while
//! a game Caskade launched is running: the session belongs to the account
//! that started it.

use caskade_ui_data::accounts::AccountRole;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::{use_account_list, use_accounts};
use crate::data::change;
use crate::router::{Route, use_router};
use crate::settings::unsaved::{Leaving, after_unsaved_changes};
use crate::ui::icons::{USER_ROUND, USERS};
use crate::ui::interact::{Keys, use_interaction};
use crate::ui::popover::use_trigger;
use crate::ui::{Avatar, AvatarSize, Menu, MenuEntry, MenuItem};

#[component]
pub fn AccountMenu() -> Element {
    let list = use_account_list();
    let accounts = use_accounts();
    let router = use_router();
    let running = crate::data::use_running_count();
    let running = match &*running.read() {
        Some(Ok(count)) => *count,
        _ => 0,
    };
    let trigger = use_trigger();
    let interaction = use_interaction();
    let mut open = use_signal(|| None::<crate::ui::Anchor>);
    let account = match &*list.read() {
        Some(Ok(list)) => list
            .active_id
            .as_ref()
            .and_then(|id| list.accounts.iter().find(|account| &account.id == id))
            .cloned(),
        _ => None,
    };
    let Some(account) = account else {
        return rsx! {};
    };
    let playing = running > 0;
    let mut show = move || {
        if let Some(anchor) = trigger.anchor() {
            open.set(Some(anchor));
        }
    };
    let items = vec![
        MenuEntry::Item(MenuItem {
            icon: Some(USER_ROUND),
            ..MenuItem::new("profile", tp!("settings tab", "My profile"))
        }),
        MenuEntry::Item(MenuItem {
            icon: Some(USERS),
            disabled: playing,
            description: playing.then(|| t!("Not while a game is running.")),
            ..MenuItem::new("switch", t!("Switch account"))
        }),
    ];
    let role = if account.role == AccountRole::Admin {
        tp!("account role", "Admin")
    } else {
        tp!("account role", "Standard")
    };
    let header = rsx! {
        div {
            "data-cs": "account-menu",
            class: "mb-1 flex items-center gap-3 border-b border-border px-2.5 pt-1.5 pb-2.5",
            Avatar { part: "avatar", avatar: Some(account.avatar.clone()), name: account.name.clone(), size: AvatarSize::Md }
            div { class: "flex min-w-0 flex-col",
                span { "data-part": "name", class: "truncate font-medium text-fg", "{account.name}" }
                span { "data-part": "role", class: "text-xs text-fg-subtle", "{role}" }
            }
        }
    };
    rsx! {
        button {
            r#type: "button",
            "data-part": "avatar",
            "data-spatial": "focusable",
            "aria-haspopup": "menu",
            "aria-expanded": if open().is_some() { "true" } else { "false" },
            "aria-label": t!("{name}, account", name = account.name.clone()),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            class: "flex rounded-full outline-none transition-[filter] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:brightness-115",
            onmounted: move |event| trigger.mounted(event),
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onclick: move |_| show(),
            onkeydown: move |event: KeyboardEvent| {
                if event.key() == Key::Named(NamedKey::ArrowDown) {
                    event.prevent_default();
                    show();
                } else {
                    interaction.key_down(&event, Keys::EnterAndSpace, show);
                }
            },
            onkeyup: move |event| interaction.key_up(&event, show),
            Avatar { avatar: Some(account.avatar.clone()), name: account.name.clone(), size: AvatarSize::Sm }
        }
        if let Some(anchor) = open() {
            Menu {
                anchor,
                items,
                header,
                label: t!("{name}'s account", name = account.name.clone()),
                return_to: trigger.node(),
                on_close: move |_| open.set(None),
                on_action: move |id: String| match id.as_str() {
                    "profile" => router.navigate(
                        &Route::Settings {
                            tab: "profile",
                            section: None,
                        }
                        .href(),
                    ),
                    "switch" => {
                        let accounts = accounts.clone();
                        after_unsaved_changes(Leaving::Account, move || {
                            let accounts = accounts.clone();
                            spawn(async move {
                                let _ = change(async move { accounts.sign_out().await }).await;
                            });
                        });
                    }
                    _ => {}
                },
            }
        }
    }
}
