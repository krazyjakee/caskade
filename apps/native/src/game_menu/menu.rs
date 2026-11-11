//! A game's menu items (`components/gameMenu/GameMenu.tsx`), worked out
//! from what the game has, without a window: [`items`] lists them in the
//! webview's order, with its conditions and wording; the host
//! (`super::GameMenuHost`) runs the one chosen by its id.

use super::entries::{EntryView, located, playable};
use crate::ui::{MenuEntry, MenuItem, icons};
use caskade_core::model::{ActionKind, GameAction, GameDetails, PlaySession};
use caskade_ui_i18n::{t, tp};
use std::collections::HashSet;

/// What a game's menu is for: the game, or one of its entries (a row on its
/// page).
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub game_id: String,
    pub title: String,
    pub entry_id: Option<String>,
}

/// What the menu reads.
#[derive(Default)]
pub struct Facts<'a> {
    pub game: Option<&'a GameDetails>,
    pub entries: Vec<EntryView>,
    pub sessions: &'a [PlaySession],
    /// Entries whose storage is out of reach.
    pub unavailable: HashSet<String>,
    /// Every release's actions.
    pub actions: Vec<GameAction>,
    /// Console mode: no "Open installation folder".
    pub console: bool,
    /// The app can move, uninstall and install (its storage service).
    pub storage: bool,
    /// The game's editor exists (#100).
    pub edits: bool,
    /// What failed last: shown last, under "That didn't work".
    pub failure: Option<String>,
    /// Hiding failed: shown on Hide itself.
    pub hide_failure: Option<String>,
    /// The app shares games through links (its share service, #247).
    pub shares: bool,
}

/// The release a share link would carry: installed here, with files here,
/// and not another device's or a store's copy (`GameMenu.tsx`'s
/// `copyable`).
pub fn copyable(entries: &[EntryView]) -> Option<&EntryView> {
    entries.iter().find(|entry| {
        !["peer:", "store:", "stream:"]
            .iter()
            .any(|prefix| entry.source.starts_with(prefix))
            && entry.installed
            && entry.location.is_some()
    })
}

/// The session running the target, if any.
pub fn running<'a>(target: &Target, facts: &'a Facts) -> Option<&'a PlaySession> {
    facts.sessions.iter().find(|session| {
        session.ended_at_ms.is_none()
            && target
                .entry_id
                .as_ref()
                .is_none_or(|entry| &session.entry_id == entry)
    })
}

fn item(id: impl Into<String>, label: String, icon: crate::ui::Icon) -> MenuItem {
    MenuItem {
        icon: Some(icon),
        ..MenuItem::new(id, label)
    }
}

fn quit_first() -> String {
    t!("Quit the game first")
}

/// The menu for `target`.
pub fn items(target: &Target, facts: &Facts) -> Vec<MenuEntry> {
    let scoped = target.entry_id.is_some();
    let entries: Vec<&EntryView> = facts
        .entries
        .iter()
        .filter(|entry| target.entry_id.as_ref().is_none_or(|id| &entry.id == id))
        .collect();
    let owned: Vec<EntryView> = entries.iter().map(|entry| (*entry).clone()).collect();
    let running = running(target, facts);
    let preferred = facts
        .game
        .and_then(|game| game.game.default_entry.as_deref());
    let playable = playable(&owned, &facts.unavailable, preferred);
    let which = |entry: &EntryView, list: usize| {
        (list > 1).then(|| {
            entry
                .location
                .clone()
                .unwrap_or_else(|| entry.source.clone())
        })
    };
    let mut first = Vec::new();

    // Stop, or Play.
    match running {
        Some(_) => first.push(MenuEntry::Item(MenuItem {
            danger: true,
            ..item("stop", tp!("game action", "Stop"), icons::SQUARE)
        })),
        None => {
            let loaded = facts.game.is_some();
            let description = (loaded && playable.is_none()).then(|| {
                if owned.is_empty() {
                    t!("Nothing to launch")
                } else {
                    t!("Not installed or unavailable")
                }
            });
            first.push(MenuEntry::Item(MenuItem {
                disabled: loaded && playable.is_none(),
                description,
                ..item("play", tp!("game action", "Play"), icons::PLAY)
            }));
            // Play with each of the release's launch actions.
            if let Some(playable) = playable {
                let plays: Vec<&GameAction> = facts
                    .actions
                    .iter()
                    .filter(|action| {
                        action.entry_id == playable.id && action.kind == ActionKind::Play
                    })
                    .collect();
                if plays.len() > 1 {
                    for action in plays {
                        first.push(MenuEntry::Item(item(
                            format!("play:{}", action.id),
                            t!(
                                note = "Starts the game with one of its launch actions",
                                "Play with {name}",
                                name = action.name
                            ),
                            icons::PLAY,
                        )));
                    }
                }
            }
        }
    }
    if !scoped {
        first.push(MenuEntry::Item(item(
            "details",
            t!("Open game details"),
            icons::INFO,
        )));
    }

    let mut second = Vec::new();
    if let Some(game) = facts.game.filter(|_| !scoped) {
        second.push(MenuEntry::Item(if game.game.favorite {
            item("favorite", t!("Remove from favourites"), icons::HEART_OFF)
        } else {
            item("favorite", t!("Add to favourites"), icons::HEART)
        }));
    }
    if !scoped && facts.edits {
        second.push(MenuEntry::Item(item("edit", t!("Edit…"), icons::PENCIL)));
    }
    if !scoped {
        second.push(MenuEntry::Item(item(
            "collections",
            t!("Add to collection…"),
            icons::LIST_PLUS,
        )));
    }
    let places = located(&owned);
    if !facts.console {
        for entry in &places {
            second.push(MenuEntry::Item(MenuItem {
                description: which(entry, places.len()),
                ..item(
                    format!("reveal:{}", entry.id),
                    t!("Open installation folder"),
                    icons::FOLDER_OPEN,
                )
            }));
        }
    }
    for entry in &places {
        second.push(MenuEntry::Item(MenuItem {
            description: which(entry, places.len()),
            ..item(
                format!("copy-path:{}", entry.id),
                tp!("game action", "Copy path"),
                icons::COPY,
            )
        }));
    }
    let managed: Vec<&EntryView> = owned.iter().filter(|entry| entry.managed()).collect();
    if !managed.is_empty() {
        second.push(MenuEntry::Item(MenuItem {
            disabled: !facts.storage || running.is_some(),
            description: running.is_some().then(quit_first),
            ..item("move", t!("Move installed files…"), icons::FOLDER_INPUT)
        }));
    }
    if facts.storage {
        let installed: Vec<&&EntryView> = managed.iter().filter(|entry| entry.installed).collect();
        for entry in &installed {
            second.push(MenuEntry::Item(MenuItem {
                danger: true,
                disabled: running.is_some(),
                description: if running.is_some() {
                    Some(quit_first())
                } else {
                    which(entry, installed.len())
                },
                ..item(
                    format!("uninstall:{}", entry.id),
                    t!("Uninstall…"),
                    icons::TRASH,
                )
            }));
        }
    }
    let all = &facts.entries;
    let scanned = all.iter().any(|entry| !entry.web());
    if !scoped && facts.storage && super::entries::added_by_caskade(all) {
        second.push(MenuEntry::Item(MenuItem {
            danger: true,
            disabled: running.is_some(),
            description: running.is_some().then(quit_first),
            ..item("remove", t!("Remove from library…"), icons::TRASH)
        }));
    }
    if !scoped && facts.game.is_some() && scanned {
        second.push(MenuEntry::Item(MenuItem {
            description: Some(facts.hide_failure.clone().unwrap_or_else(|| {
                t!("Scans won't bring it back. Show it again in Settings → Library Discovery.")
            })),
            ..item("hide", t!("Hide from library"), icons::EYE_OFF)
        }));
    }
    if let Some(failure) = &facts.failure {
        second.push(MenuEntry::Item(MenuItem {
            disabled: true,
            description: Some(failure.clone()),
            ..MenuItem::new(
                "failure",
                t!(
                    note = "A menu action failed; the reason is below",
                    "That didn't work"
                ),
            )
        }));
    }

    let mut menu = vec![MenuEntry::Section {
        title: None,
        items: first.into_iter().filter_map(only_item).collect(),
    }];
    menu.push(MenuEntry::Separator);
    menu.push(MenuEntry::Section {
        title: None,
        items: second.into_iter().filter_map(only_item).collect(),
    });

    // Your devices: Share with someone… (#247). Playing on and copying to
    // paired devices join it with sync's views.
    if !scoped && facts.shares && copyable(&facts.entries).is_some() {
        menu.push(MenuEntry::Separator);
        menu.push(MenuEntry::Section {
            title: Some(tp!("game menu section", "Your devices")),
            items: vec![MenuItem {
                description: Some(t!("A link for someone else's Caskade")),
                ..item("share", t!("Share with someone…"), icons::SHARE_2)
            }],
        });
    }

    // Tools: a release's utilities, which count no playtime.
    let ids: HashSet<&str> = owned.iter().map(|entry| entry.id.as_str()).collect();
    let tools: Vec<&GameAction> = facts
        .actions
        .iter()
        .filter(|action| {
            action.kind == ActionKind::Utility && ids.contains(action.entry_id.as_str())
        })
        .collect();
    if !tools.is_empty() {
        menu.push(MenuEntry::Separator);
        menu.push(MenuEntry::Section {
            title: Some(tp!("game menu section", "Tools")),
            items: tools
                .into_iter()
                .map(|tool| MenuItem {
                    description: Some(t!("No playtime is counted")),
                    ..item(
                        format!("tool:{}:{}", tool.entry_id, tool.id),
                        tool.name.clone(),
                        icons::WRENCH,
                    )
                })
                .collect(),
        });
    }
    menu
}

fn only_item(entry: MenuEntry) -> Option<MenuItem> {
    match entry {
        MenuEntry::Item(item) => Some(item),
        _ => None,
    }
}
