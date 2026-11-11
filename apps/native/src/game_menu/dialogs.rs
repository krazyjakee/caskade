//! What a game's menu asks before it deletes anything
//! (`components/gameMenu/UninstallDialog.tsx`, `RemoveGameDialog.tsx`):
//! uninstalling an install's files, and removing a game Caskade added from
//! the library. Each runs as a job in Activity.

use super::actions;
use crate::router::use_router;
use crate::ui::{Button, ButtonVariant, Dialog};
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;

#[component]
pub fn UninstallDialog(
    title: String,
    entry_id: String,
    location: Option<String>,
    on_close: EventHandler<()>,
) -> Element {
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let body = match &location {
        Some(location) => t!(
            "Its installed files are deleted from {location}, with the play sessions recorded for this install. The game stays in your library. The uninstall shows in Activity.",
            location = location
        ),
        None => t!(
            "Its installed files are deleted, with the play sessions recorded for this install. The game stays in your library. The uninstall shows in Activity."
        ),
    };
    let uninstall = move |_| {
        let entry_id = entry_id.clone();
        pending.set(true);
        spawn(async move {
            match actions::uninstall(entry_id).await {
                Ok(_) => on_close.call(()),
                Err(report) => error.set(Some(report.summary)),
            }
            pending.set(false);
        });
    };
    rsx! {
        Dialog {
            title: t!("Uninstall {title}?", title = title),
            alert: true,
            on_close: move |_| {
                if !pending() {
                    on_close.call(());
                }
            },
            p { class: "text-sm text-fg-muted", "{body}" }
            if let Some(error) = error() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
            div { class: "mt-5 flex justify-end gap-2",
                Button {
                    disabled: pending(),
                    "data-focus-default": "true",
                    on_press: move |_| on_close.call(()),
                    {t!("Cancel")}
                }
                Button {
                    variant: ButtonVariant::Danger,
                    disabled: pending(),
                    on_press: uninstall,
                    if pending() { {t!("Uninstalling…")} } else { {t!("Uninstall")} }
                }
            }
        }
    }
}

#[component]
pub fn RemoveGameDialog(
    title: String,
    game_id: String,
    /// It has files Caskade installed, deleted first.
    uninstalls: bool,
    on_close: EventHandler<()>,
) -> Element {
    let router = use_router();
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let body = if uninstalls {
        t!(
            "The files Caskade installed for it are deleted, then it leaves your library with its play sessions. Files it didn't install stay where they are. The uninstall shows in Activity."
        )
    } else {
        t!(
            "It leaves your library with its play sessions. Its files, if it has any, stay where they are."
        )
    };
    let remove = move |_| {
        let game_id = game_id.clone();
        pending.set(true);
        spawn(async move {
            match actions::remove(game_id.clone()).await {
                Ok(_) => {
                    on_close.call(());
                    // Its page is gone: back where it was opened from.
                    if let crate::router::Route::Game { id, .. } = router.peek()
                        && id == game_id
                    {
                        router.back();
                    }
                }
                Err(report) => error.set(Some(report.summary)),
            }
            pending.set(false);
        });
    };
    rsx! {
        Dialog {
            title: t!("Remove {title} from your library?", title = title),
            alert: true,
            on_close: move |_| {
                if !pending() {
                    on_close.call(());
                }
            },
            p { class: "text-sm text-fg-muted", "{body}" }
            if let Some(error) = error() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
            div { class: "mt-5 flex justify-end gap-2",
                Button {
                    disabled: pending(),
                    "data-focus-default": "true",
                    on_press: move |_| on_close.call(()),
                    {t!("Cancel")}
                }
                Button {
                    variant: ButtonVariant::Danger,
                    disabled: pending(),
                    on_press: remove,
                    if pending() { {t!("Removing…")} } else { {tp!("library game", "Remove")} }
                }
            }
        }
    }
}
