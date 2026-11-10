//! The library's Add game button, as the webview's `AddGame`: a menu of
//! the ways to add a game, each opening its own dialog. Installing from a
//! file, a folder or a link asks where the install goes first
//! (`InstallRootPicker`); adding by hand (#174) opens the manual dialog.
//! Receiving a shared game joins with the shares flow (#108).

use caskade_ui_data::storage::{MoveError, Storage};
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use crate::data::DataSource;
use crate::ui::icons::PLUS;
use crate::ui::{Button, ButtonVariant, Dialog, MenuButton, MenuEntry, MenuItem, TextField};

use super::manual_game::{ManualGameDialog, ManualMode};
use super::paths::{PathRequest, PickKind, use_path_picker};
use super::storage::{InstallRootPicker, use_storage};

/// The button and the dialogs it opens. `on_open_game` gets the id of a
/// game added by hand, to open its page.
#[component]
pub fn AddGame(on_open_game: EventHandler<String>) -> Element {
    // Adding by hand needs the library: not before the data source is
    // ready, as the webview leaves the items out without its API.
    let manual = try_use_context::<DataSource>().is_some();
    let storage = use_storage().filter(Storage::can_install);
    let picker = use_path_picker();
    let mut adding = use_signal(|| None::<ManualMode>);
    let mut url_open = use_signal(|| false);
    let mut url = use_signal(String::new);
    let mut pending = use_signal(|| None::<Vec<String>>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let mut items = Vec::new();
    if storage.is_some() {
        items.push(MenuEntry::Item(MenuItem {
            description: Some(t!("An installer, archive, disc image or .torrent file")),
            ..MenuItem::new("file", t!("Install from file…"))
        }));
        items.push(MenuEntry::Item(MenuItem::new(
            "folder",
            t!("Install from folder…"),
        )));
        items.push(MenuEntry::Item(MenuItem {
            description: Some(t!("A download link or a magnet link")),
            ..MenuItem::new("url", t!("Install from link…"))
        }));
    }
    if manual {
        items.push(MenuEntry::Item(MenuItem::new(
            "manual-target",
            t!("Add a game on this computer…"),
        )));
        items.push(MenuEntry::Item(MenuItem::new(
            "manual-title",
            t!("Add a game by title…"),
        )));
    }

    // Installs the inputs on the location chosen; the jobs go to Activity.
    let install = {
        let storage = storage.clone();
        move |inputs: Vec<String>, root: Option<String>| {
            let Some(storage) = storage.clone() else {
                return;
            };
            busy.set(true);
            spawn(async move {
                let mut failed = None;
                for input in inputs {
                    let storage = storage.clone();
                    let root = root.clone();
                    let started =
                        caskade_app::runtime::spawn(
                            async move { storage.install(input, root).await },
                        )
                        .await
                        .unwrap_or_else(|error| Err(MoveError::Failed(error.to_string())));
                    if let Err(error) = started {
                        failed = Some(match error {
                            MoveError::Failed(message) => message,
                            MoveError::Unavailable | MoveError::NeedsUser => {
                                t!("Storage service is unavailable")
                            }
                        });
                        break;
                    }
                }
                busy.set(false);
                match failed {
                    Some(message) => error.set(Some(message)),
                    None => {
                        url_open.set(false);
                        url.set(String::new());
                    }
                }
            });
        }
    };
    // What to install, chosen: then where it goes.
    let mut chosen = move |inputs: Vec<String>| {
        if !inputs.is_empty() {
            url_open.set(false);
            pending.set(Some(inputs));
        }
    };
    let mut choose = move |kind: PickKind| {
        error.set(None);
        spawn(async move {
            let paths = if picker.in_app() {
                // Console mode: one path from the app's own browser.
                let title = match kind {
                    PickKind::File => t!("Install from file"),
                    PickKind::Folder => t!("Install from folder"),
                };
                picker
                    .pick(PathRequest::new(kind, title))
                    .await
                    .into_iter()
                    .collect()
            } else {
                let picked = caskade_app::runtime::spawn(async move {
                    let dialogs = crate::dialogs::install_picker();
                    match kind {
                        PickKind::File => {
                            caskade_app::commands::install::choose_install_files(&dialogs).await
                        }
                        PickKind::Folder => {
                            caskade_app::commands::install::choose_install_folder(&dialogs)
                                .await
                                .map(|folder| folder.into_iter().collect())
                        }
                    }
                })
                .await
                .unwrap_or_else(|error| Err(error.to_string()));
                match picked {
                    Ok(paths) => paths,
                    Err(message) => {
                        error.set(Some(message));
                        Vec::new()
                    }
                }
            };
            chosen(paths);
        });
    };

    rsx! {
        div { "data-cs": "add-game",
            MenuButton {
                id: "add-game-button",
                label: t!("Add game"),
                icon: PLUS,
                variant: ButtonVariant::Primary,
                disabled: items.is_empty() || busy(),
                items,
                on_action: move |id: String| {
                    error.set(None);
                    match id.as_str() {
                        "file" => choose(PickKind::File),
                        "folder" => choose(PickKind::Folder),
                        "url" => url_open.set(true),
                        "manual-target" => adding.set(Some(ManualMode::Target)),
                        "manual-title" => adding.set(Some(ManualMode::Title)),
                        _ => {}
                    }
                },
            }
            if let Some(mode) = adding() {
                ManualGameDialog {
                    mode,
                    on_close: move |_| adding.set(None),
                    on_added: move |id| on_open_game.call(id),
                }
            }
            if let (Some(message), false) = (error(), url_open()) {
                p { role: "alert", class: "mt-2 max-w-sm text-sm text-danger", "{message}" }
            }
            if url_open() {
                Dialog {
                    title: t!("Install from link"),
                    on_close: (!busy()).then_some(EventHandler::new(move |_| url_open.set(false))),
                    class: "max-w-lg",
                    "data-cs": "install-url",
                    TextField {
                        id: "install-url-input",
                        label: t!("Download or magnet link"),
                        kind: "url",
                        value: url(),
                        required: true,
                        disabled: busy(),
                        description: t!("A download link or a magnet link. The installer resolves it before downloading, and you may be asked to choose what to download."),
                        on_input: move |value| url.set(value),
                        on_submit: move |value: String| {
                            if !value.trim().is_empty() {
                                chosen(vec![value.trim().to_owned()]);
                            }
                        },
                    }
                    if let Some(message) = error() {
                        p { role: "alert", class: "text-sm text-danger", "{message}" }
                    }
                    div { class: "flex justify-end gap-2",
                        Button { id: "install-url-cancel", disabled: busy(), on_press: move |_| url_open.set(false), {t!("Cancel")} }
                        Button {
                            id: "install-url-add",
                            variant: ButtonVariant::Primary,
                            disabled: url().trim().is_empty() || busy(),
                            on_press: move |_| chosen(vec![url.peek().trim().to_owned()]),
                            if busy() { {t!("Adding…")} } else { {t!("Add installation")} }
                        }
                    }
                }
            }
            if let (Some(inputs), Some(storage)) = (pending(), storage.clone()) {
                Dialog {
                    title: t!("Install location"),
                    on_close: move |_| pending.set(None),
                    class: "max-h-[85vh] max-w-xl overflow-auto",
                    InstallRootPicker {
                        storage,
                        heading: false,
                        on_cancel: move |_| pending.set(None),
                        on_select: {
                            let mut install = install.clone();
                            move |root: String| {
                                pending.set(None);
                                install(inputs.clone(), Some(root));
                            }
                        },
                    }
                }
            }
        }
    }
}
