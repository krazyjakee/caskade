//! A collection's commands (the webview's
//! `components/collections/useCollectionActions.tsx`): edit, add games,
//! pin, share and delete, as menu items, and the dialogs they open. A view
//! takes them from [`use_collection_actions`] and renders
//! [`CollectionActions::dialogs`] once.

use super::add_games::AddGamesDialog;
use super::delete::DeleteCollectionDialog;
use super::{PIN, PIN_OFF};
use crate::data::{change, use_library};
use crate::flows::share_game::{ShareGameDialog, ShareTarget};
use crate::library::collection_dialog::CollectionDialog;
use crate::ui::{MenuEntry, MenuItem, icons};
use caskade_core::model::{CollectionKind, CollectionSummary};
use caskade_ui_data::Library;
use caskade_ui_data::shares::{ShareItem, Shares};
use caskade_ui_i18n::t;
use dioxus_native::prelude::dioxus_core::spawn_forever;
use dioxus_native::prelude::*;

/// A dialog a command opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    Edit,
    Add,
    Share,
    Delete,
}

/// One view's collection commands and the dialog open, if any.
#[derive(Clone, Copy, PartialEq)]
pub struct CollectionActions {
    library: Signal<Library>,
    /// Share with someone…, where this app shares (#247, #248).
    shares: Signal<Option<Shares>>,
    open: Signal<Option<(Open, CollectionSummary)>>,
    pinning: Signal<bool>,
    error: Signal<Option<String>>,
}

pub fn use_collection_actions() -> CollectionActions {
    let library = use_library();
    let shares = crate::flows::receive_share::use_shares();
    CollectionActions {
        library: use_signal(move || library),
        shares: use_signal(move || shares),
        open: use_signal(|| None),
        pinning: use_signal(|| false),
        error: use_signal(|| None),
    }
}

impl CollectionActions {
    /// `collection`'s commands, as its menus list them.
    pub fn items(&self, collection: &CollectionSummary) -> Vec<MenuEntry> {
        let smart = matches!(collection.kind, CollectionKind::Smart { .. });
        let mut items = vec![MenuItem {
            icon: Some(icons::PENCIL),
            ..MenuItem::new(
                "edit",
                if smart {
                    t!("Edit rules…")
                } else {
                    t!("Edit…")
                },
            )
        }];
        if !smart {
            items.push(MenuItem {
                icon: Some(icons::PLUS),
                ..MenuItem::new("add", t!("Add games…"))
            });
        }
        items.push(MenuItem {
            icon: Some(if collection.pinned { PIN_OFF } else { PIN }),
            disabled: (self.pinning)(),
            ..MenuItem::new(
                "pin",
                if collection.pinned {
                    t!("Unpin")
                } else {
                    t!("Pin to the top")
                },
            )
        });
        // As a list of games, without files (#248).
        if self.shares.read().is_some() {
            items.push(MenuItem {
                icon: Some(icons::SHARE_2),
                ..MenuItem::new("share", t!("Share with someone…"))
            });
        }
        items.push(MenuItem {
            icon: Some(icons::TRASH),
            danger: true,
            ..MenuItem::new("delete", t!("Delete…"))
        });
        items.into_iter().map(MenuEntry::Item).collect()
    }

    /// Runs command `id` (a menu item's) on `collection`.
    pub fn run(&self, id: &str, collection: &CollectionSummary) {
        match id {
            "edit" => self.open(Open::Edit, collection.clone()),
            "add" => self.open(Open::Add, collection.clone()),
            "share" => self.open(Open::Share, collection.clone()),
            "delete" => self.open(Open::Delete, collection.clone()),
            "pin" => self.pin(collection),
            _ => {}
        }
    }

    /// Opens one of the commands' dialogs.
    pub fn open(&self, open: Open, collection: CollectionSummary) {
        let mut slot = self.open;
        slot.set(Some((open, collection)));
    }

    /// Why the last pin or unpin failed.
    pub fn error(&self) -> Option<String> {
        (self.error)()
    }

    /// Pins `collection` to the top of the list, or unpins it. It runs
    /// beyond the menu that asked, which closes first.
    fn pin(&self, collection: &CollectionSummary) {
        let mut pinning = self.pinning;
        let mut error = self.error;
        if *pinning.peek() {
            return;
        }
        let library = self.library.peek().clone();
        let id = collection.id.clone();
        let pinned = !collection.pinned;
        pinning.set(true);
        error.set(None);
        spawn_forever(async move {
            let result =
                change(async move { library.set_collection_pinned(&id, pinned).await }).await;
            // The view may have gone meanwhile.
            if let Ok(mut error) = error.try_write() {
                *error = result.err().map(|report| report.summary);
            }
            if let Ok(mut pinning) = pinning.try_write() {
                *pinning = false;
            }
            super::changed();
        });
    }

    /// The dialog a command opened; render it once. `on_deleted` follows a
    /// deletion (a collection's page goes back to the list).
    pub fn dialogs(&self, on_deleted: Option<EventHandler<()>>) -> Element {
        let mut slot = self.open;
        let close = move |_| slot.set(None);
        let Some((open, collection)) = slot.read().clone() else {
            return rsx! {};
        };
        match open {
            Open::Edit => rsx! {
                CollectionDialog {
                    collection,
                    on_close: close,
                    on_saved: move |_: String| super::changed(),
                }
            },
            Open::Add => rsx! {
                AddGamesDialog { collection, on_close: close }
            },
            Open::Share => match self.shares.peek().clone() {
                Some(shares) => rsx! {
                    ShareGameDialog {
                        shares,
                        target: ShareTarget::Item {
                            item: ShareItem::Collection {
                                collection_id: collection.id.clone(),
                            },
                            title: collection.name.clone(),
                        },
                        on_close: close,
                    }
                },
                None => rsx! {},
            },
            Open::Delete => rsx! {
                DeleteCollectionDialog { collection, on_close: close, on_deleted }
            },
        }
    }
}
