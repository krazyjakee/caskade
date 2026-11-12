//! Games' menus, quick looks and the dialogs they open (#99,
//! `components/gameMenu/GameMenuHost.tsx`): one host around the views, so a
//! card, a row, a search suggestion or a game's page opens the same menu.
//! A right click, the menu key, Shift F10 or a pad's Start opens a game's
//! menu; a click opens its quick look beside the view (a pad's X, too);
//! a double click plays it. Only one menu is open at a time, and a game
//! starting shows as starting until it runs or doesn't.

pub mod actions;
mod anatomy;
mod collections;
mod dialogs;
pub mod entries;
pub mod menu;
pub(crate) mod play;
mod quick_look;

#[cfg(test)]
mod tests;

pub use anatomy::ANATOMY;
pub use menu::Target;

use crate::data::{Query, use_game, use_library, use_query};
use crate::router::use_router;
use crate::ui::{Anchor, Menu};
use caskade_core::model::{GameAction, PlaySession};
use caskade_ui_data::Key as DataKey;
use dioxus_native::prelude::dioxus_core::spawn_forever;
use dioxus_native::prelude::*;
use entries::EntryView;

/// What a menu opened, which outlives the menu itself.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuDialog {
    Play {
        entry_id: Option<String>,
        action: Option<String>,
    },
    Collections,
    Move {
        entry_id: Option<String>,
        running: bool,
    },
    Uninstall {
        entry_id: String,
        location: Option<String>,
    },
    Remove {
        uninstalls: bool,
    },
    /// A web game's Play offline…: where to keep it.
    Offline,
    /// A web game's Remove…; `keeps_game` when it has other entries.
    RemoveWeb {
        keeps_game: bool,
    },
    /// Share with someone… (#247): a release, or a web game's offline copy
    /// or a ROM rather than the folder they're in (#248).
    Share {
        entry_id: String,
        item: Option<caskade_ui_data::shares::ShareItem>,
    },
    /// Edit…: the game's editor (#100).
    Edit,
    /// One of a release's tools, run once (#108's `ToolRequest`).
    Tool {
        entry_id: String,
        action_id: String,
        name: String,
    },
}

/// The games' menus, from any view under [`GameMenuHost`].
#[derive(Clone, Copy, PartialEq)]
pub struct GameMenus {
    menu: Signal<Option<(Target, Anchor, Option<String>)>>,
    look: Signal<Option<Target>>,
    dialog: Signal<Option<(Target, MenuDialog)>>,
    starting: Signal<Option<String>>,
}

impl GameMenus {
    /// Opens `target`'s menu at `anchor`.
    pub fn open(&self, target: Target, anchor: Anchor) {
        let mut menu = self.menu;
        menu.set(Some((target, anchor, None)));
    }

    /// Opens `target`'s quick look beside the view.
    pub fn look(&self, target: Target) {
        let mut look = self.look;
        look.set(Some(target));
    }

    pub fn close_look(&self) {
        let mut look = self.look;
        look.set(None);
    }

    /// Plays `target` as its menu's Play does; not again while it starts.
    pub fn play(&self, target: Target) {
        if self.starting.peek().as_deref() == Some(target.game_id.as_str()) {
            return;
        }
        let entry_id = target.entry_id.clone();
        self.dialog(
            target,
            MenuDialog::Play {
                entry_id,
                action: None,
            },
        );
    }

    /// Opens one of the menu's dialogs without the menu.
    pub fn dialog(&self, target: Target, dialog: MenuDialog) {
        let mut slot = self.dialog;
        slot.set(Some((target, dialog)));
    }

    /// Whether `game_id` is starting.
    pub fn starting(&self, game_id: &str) -> bool {
        self.starting.read().as_deref() == Some(game_id)
    }
}

pub fn use_game_menus() -> Option<GameMenus> {
    try_use_context::<GameMenus>()
}

/// Holds the menus, quick look and dialogs for `children`.
#[component]
pub fn GameMenuHost(children: Element) -> Element {
    let menus = use_context_provider(|| GameMenus {
        menu: Signal::new(None),
        look: Signal::new(None),
        dialog: Signal::new(None),
        starting: Signal::new(None),
    });
    let router = use_router();
    // A page change closes the quick look.
    let path = router.route().path();
    let mut seen_path = use_signal(|| path.clone());
    if *seen_path.peek() != path {
        seen_path.set(path);
        if menus.look.peek().is_some() {
            menus.close_look();
        }
    }
    let menu = menus.menu;
    let mut dialog = menus.dialog;
    // Moving a game's files (#108's dialog), where the app has storage.
    let storage = crate::flows::storage::use_storage();
    let shares = crate::flows::receive_share::use_shares();
    rsx! {
        {children}
        // Keyed in lists, so another game's menu or quick look is one of
        // its own (Dioxus keys only list items): never the last game's.
        for (target, anchor, failure) in menu() {
            OpenMenu { key: "{target.game_id}:{target.entry_id:?}", target, anchor, failure }
        }
        for target in (menus.look)() {
            quick_look::QuickLook { key: "{target.game_id}", target }
        }
        if let Some((target, open)) = dialog() {
            {
                let close = move |_| dialog.set(None);
                match open {
                    MenuDialog::Play { entry_id, action } => rsx! {
                        play::PlayRequest {
                            game_id: target.game_id.clone(),
                            title: target.title.clone(),
                            entry_id,
                            action,
                            starting: menus.starting,
                            on_close: close,
                        }
                    },
                    MenuDialog::Collections => rsx! {
                        collections::CollectionPicker {
                            game_id: target.game_id.clone(),
                            title: target.title.clone(),
                            on_close: close,
                        }
                    },
                    MenuDialog::Move { entry_id, running } => match storage.clone() {
                        Some(storage) => rsx! {
                            crate::flows::storage::MoveGameDialog {
                                storage,
                                game_id: target.game_id.clone(),
                                entry_id,
                                is_running: running,
                                on_close: close,
                            }
                        },
                        None => rsx! {},
                    },
                    MenuDialog::Uninstall { entry_id, location } => rsx! {
                        dialogs::UninstallDialog {
                            title: target.title.clone(),
                            entry_id,
                            location,
                            on_close: close,
                        }
                    },
                    MenuDialog::Offline => rsx! {
                        crate::library::web::SaveOfflineDialog {
                            game_id: target.game_id.clone(),
                            title: target.title.clone(),
                            on_close: close,
                        }
                    },
                    MenuDialog::RemoveWeb { keeps_game } => rsx! {
                        crate::library::web::RemoveWebGameDialog {
                            game_id: target.game_id.clone(),
                            title: target.title.clone(),
                            keeps_game,
                            on_close: close,
                        }
                    },
                    MenuDialog::Remove { uninstalls } => rsx! {
                        dialogs::RemoveGameDialog {
                            title: target.title.clone(),
                            game_id: target.game_id.clone(),
                            uninstalls,
                            on_close: close,
                        }
                    },
                    MenuDialog::Share { entry_id, item } => match shares.clone() {
                        Some(shares) => rsx! {
                            crate::flows::share_game::ShareGameDialog {
                                shares,
                                target: match item {
                                    Some(item) => crate::flows::share_game::ShareTarget::Item {
                                        item,
                                        title: target.title.clone(),
                                    },
                                    None => crate::flows::share_game::ShareTarget::Game {
                                        entry_id,
                                        title: target.title.clone(),
                                    },
                                },
                                on_close: close,
                            }
                        },
                        None => rsx! {},
                    },
                    MenuDialog::Edit => rsx! {
                        crate::game::editor::GameEditor {
                            game_id: target.game_id.clone(),
                            title: target.title.clone(),
                            on_close: close,
                        }
                    },
                    MenuDialog::Tool { entry_id, action_id, name } => rsx! {
                        crate::flows::tool_request::ToolRequest {
                            entry_id,
                            action_id,
                            name,
                            on_close: close,
                        }
                    },
                }
            }
        }
    }
}

/// A game's play sessions, the latest first.
pub fn use_sessions(game_id: String) -> Query<Vec<PlaySession>> {
    use_query(DataKey::Sessions(Some(game_id.clone())), move |library| {
        let game_id = game_id.clone();
        async move { library.sessions(Some(&game_id), 1000).await }
    })
}

/// Every release's launch actions and tools.
fn use_actions(game_id: String) -> Query<Vec<GameAction>> {
    use_query(DataKey::Game(game_id.clone()), move |_| {
        let game_id = game_id.clone();
        async move {
            let Some(services) = crate::shell::services() else {
                return Ok(Vec::new());
            };
            caskade_app::commands::actions::game_actions(&services.store, &game_id)
                .await
                .map(|releases| {
                    releases
                        .into_iter()
                        .flat_map(|release| release.actions)
                        .collect()
                })
        }
    })
}

/// The menu open now: it reads the game, its sessions and its actions.
#[component]
fn OpenMenu(target: Target, anchor: Anchor, failure: Option<String>) -> Element {
    let menus = use_game_menus().expect("under GameMenuHost");
    let router = use_router();
    let library = use_library();
    let game = use_game(target.game_id.clone());
    let sessions = use_sessions(target.game_id.clone());
    let actions_query = use_actions(target.game_id.clone());
    // The game on the user's other devices, and where a copy could go
    // (#232, #235), read as the menu opens.
    let devices = crate::flows::devices::use_other_devices();
    let elsewhere = {
        let devices = devices.clone();
        let game_id = target.game_id.clone();
        use_resource(move || {
            let devices = devices.clone();
            let game_id = game_id.clone();
            async move {
                let devices = devices?;
                crate::data::change(async move {
                    let copies = devices.copies(&game_id).await?;
                    let paired = devices.paired().await?;
                    Ok((copies, paired))
                })
                .await
                .ok()
            }
        })
    };
    let (peers, paired) = elsewhere.read().clone().flatten().unwrap_or_default();
    let game = game
        .read()
        .as_ref()
        .and_then(|game| game.as_ref().ok().cloned().flatten());
    let sessions = sessions
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok().cloned())
        .unwrap_or_default();
    let game_actions = actions_query
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok().cloned())
        .unwrap_or_default();
    let entries: Vec<EntryView> = game.as_ref().map(entries::entries).unwrap_or_default();
    let tools: Vec<(String, String, String)> = game_actions
        .iter()
        .map(|action| {
            (
                action.entry_id.clone(),
                action.id.clone(),
                action.name.clone(),
            )
        })
        .collect();
    // Console mode's shell (#107) has no "Open installation folder"; the
    // native app has no console mode yet.
    let console = false;
    let edits = crate::game::editor::use_edits();
    let facts = menu::Facts {
        game: game.as_ref(),
        entries: entries.clone(),
        sessions: &sessions,
        actions: game_actions,
        console,
        storage: crate::shell::services().is_some_and(|services| services.install.is_some()),
        edits,
        failure: failure.clone(),
        shares: crate::flows::receive_share::use_shares().is_some(),
        peers,
        paired,
        ..menu::Facts::default()
    };
    let mut items = menu::items(&target, &facts);
    // Plugins' game actions (#109), for the whole game only.
    let plugin_actions = crate::plugins::use_slot("game.actions");
    let plugin_ui = crate::data::use_plugin_ui();
    let toasts = crate::ui::use_toasts();
    if target.entry_id.is_none() && !plugin_actions.is_empty() {
        items.push(crate::ui::MenuEntry::Separator);
        items.push(crate::ui::MenuEntry::Section {
            title: Some(caskade_ui_i18n::tp!("game menu section", "Plugins")),
            items: crate::plugins::game_actions(&plugin_actions),
        });
    }
    let running = menu::running(&target, &facts).map(|session| session.id.clone());
    let favorite = game.as_ref().map(|game| game.game.favorite);
    let uninstalls = entries.iter().any(EntryView::managed);
    // A web game's other entries stay when its web version goes.
    let keeps_game = entries.iter().any(|entry| !entry.web());
    let mut slot = menus.menu;
    // A failed action keeps the menu open with why.
    let fail = {
        let target = target.clone();
        use_callback(move |report: caskade_core::error::ErrorReport| {
            slot.set(Some((target.clone(), anchor, Some(report.summary))));
        })
    };
    let on_action = {
        let target = target.clone();
        let entries = entries.clone();
        move |id: String| {
            // The menu closes first, so its tasks run on the root's scope,
            // beyond it.
            slot.set(None);
            let target = target.clone();
            let open_dialog = |dialog: MenuDialog| menus.dialog(target.clone(), dialog);
            match id.as_str() {
                "stop" => {
                    if let Some(session) = running.clone() {
                        spawn_forever(async move {
                            if let Err(report) = actions::stop(session).await {
                                fail(report);
                            }
                        });
                    }
                }
                "play" => open_dialog(MenuDialog::Play {
                    entry_id: target.entry_id.clone(),
                    action: None,
                }),
                "details" => router.open_game(&target.game_id),
                "favorite" => {
                    let library = library.clone();
                    let game_id = target.game_id.clone();
                    let next = !favorite.unwrap_or(false);
                    spawn_forever(async move {
                        if let Err(report) = crate::data::change(async move {
                            library.set_favorite(&game_id, next).await
                        })
                        .await
                        {
                            fail(report);
                        }
                    });
                }
                "collections" => open_dialog(MenuDialog::Collections),
                "edit" => open_dialog(MenuDialog::Edit),
                "move" => open_dialog(MenuDialog::Move {
                    entry_id: target.entry_id.clone(),
                    running: running.is_some(),
                }),
                "remove" => open_dialog(MenuDialog::Remove { uninstalls }),
                "offline" => open_dialog(MenuDialog::Offline),
                "remove-web" => open_dialog(MenuDialog::RemoveWeb { keeps_game }),
                "online" => {
                    let game_id = target.game_id.clone();
                    spawn_forever(async move {
                        if let Err(report) = crate::library::web::remove_offline(game_id).await {
                            fail(report);
                        }
                    });
                }
                "share" => {
                    if let Some(entry) = menu::copyable(&entries) {
                        use caskade_ui_data::shares::ShareItem;
                        let item = if entry.web() {
                            Some(ShareItem::Web {
                                game_id: target.game_id.clone(),
                            })
                        } else if entry.emulated {
                            Some(ShareItem::Rom {
                                entry_id: entry.id.clone(),
                            })
                        } else {
                            None
                        };
                        open_dialog(MenuDialog::Share {
                            entry_id: entry.id.clone(),
                            item,
                        });
                    }
                }
                "hide" => {
                    let library = library.clone();
                    let game_id = target.game_id.clone();
                    spawn_forever(async move {
                        if let Err(report) =
                            crate::data::change(
                                async move { library.set_hidden(&game_id, true).await },
                            )
                            .await
                        {
                            fail(report);
                        }
                    });
                }
                other => {
                    let game_id = target.game_id.clone();
                    if let Some(device) = other.strip_prefix("play-on:") {
                        let (devices, device) = (devices.clone(), device.to_owned());
                        spawn_forever(async move {
                            let Some(devices) = devices else { return };
                            if let Err(report) = crate::data::change(async move {
                                devices.play_on(&game_id, &device).await
                            })
                            .await
                            {
                                fail(report);
                            }
                        });
                    } else if let Some(device) = other.strip_prefix("send:") {
                        let (devices, device) = (devices.clone(), device.to_owned());
                        spawn_forever(async move {
                            let Some(devices) = devices else { return };
                            if let Err(report) = crate::data::change(async move {
                                devices.send_to(&game_id, &device).await
                            })
                            .await
                            {
                                fail(report);
                            }
                        });
                    } else if other.starts_with("device:") {
                        // Its page lists the copies on other devices.
                        router.open_game(&game_id);
                    } else if let Some(action) = other.strip_prefix("play:") {
                        let entry = entries::playable(&entries, &Default::default(), None)
                            .map(|entry| entry.id.clone());
                        open_dialog(MenuDialog::Play {
                            entry_id: entry,
                            action: Some(action.to_owned()),
                        });
                    } else if let Some((entry_id, action_id, name)) = tools
                        .iter()
                        .find(|(entry, action, _)| other == format!("tool:{entry}:{action}"))
                        .cloned()
                    {
                        open_dialog(MenuDialog::Tool {
                            entry_id,
                            action_id,
                            name,
                        });
                    } else if let Some(entry) = other.strip_prefix("reveal:") {
                        let entry = entry.to_owned();
                        spawn_forever(async move {
                            if let Err(report) = actions::reveal(entry).await {
                                fail(report);
                            }
                        });
                    } else if let Some(entry) = other.strip_prefix("copy-path:") {
                        let entry = entry.to_owned();
                        spawn_forever(async move {
                            if let Err(report) = actions::copy_path(entry).await {
                                fail(report);
                            }
                        });
                    } else if let Some(action) = plugin_actions
                        .iter()
                        .find(|action| other == format!("plugin:{}/{}", action.plugin, action.id))
                    {
                        crate::plugins::run_action(
                            plugin_ui.clone(),
                            action.clone(),
                            Some(target.game_id.clone()),
                            toasts,
                        );
                    } else if let Some(entry_id) = other.strip_prefix("uninstall:") {
                        let location = entries
                            .iter()
                            .find(|entry| entry.id == entry_id)
                            .and_then(|entry| entry.location.clone());
                        open_dialog(MenuDialog::Uninstall {
                            entry_id: entry_id.to_owned(),
                            location,
                        });
                    }
                }
            }
        }
    };
    rsx! {
        Menu {
            anchor,
            items,
            label: target.title.clone(),
            on_close: move |_| slot.set(None),
            on_action,
        }
    }
}
