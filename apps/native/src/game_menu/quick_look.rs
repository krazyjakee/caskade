//! A game at a glance (`components/gameMenu/QuickLook.tsx`): a column on
//! the right that a click on a game opens, beside the view, with its art,
//! status, platform, Play (or Stop) and View more, then its playtime, last
//! play, size, release, genres and description. Choosing another game
//! shows that one; Escape, Close or a page change closes it. A pad's X in
//! it plays, so X then X plays a game.

use super::entries::{entries, playable};
use super::{GameMenus, Target, actions, use_game_menus, use_sessions};
use crate::data::use_game;
use crate::input::{Bind, use_input_action};
use crate::router::use_router;
use crate::ui::{
    Button, ButtonVariant, GameArt, GameStatus, StatusBadge, Svg, icons, platform_line,
};
use caskade_ui_i18n::{format, t, tp};
use caskade_ui_input::Action;
use dioxus_native::prelude::*;
use std::collections::HashSet;

#[component]
pub fn QuickLook(target: Target) -> Element {
    let menus: GameMenus = use_game_menus().expect("under GameMenuHost");
    let router = use_router();
    let game = use_game(target.game_id.clone());
    let sessions = use_sessions(target.game_id.clone());
    let mut panel = use_signal(|| None::<blitz_dom::NodeId>);
    let mut stopping = use_signal(|| false);
    let mut stop_error = use_signal(|| None::<String>);
    let game = game
        .read()
        .as_ref()
        .and_then(|game| game.as_ref().ok().cloned().flatten());
    let running = sessions
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok())
        .and_then(|list| {
            list.iter()
                .find(|session| session.ended_at_ms.is_none())
                .cloned()
        });
    let list = game.as_ref().map(entries).unwrap_or_default();
    let playable_entry = game.as_ref().and_then(|game| {
        playable(&list, &HashSet::new(), game.game.default_entry.as_deref())
            .map(|entry| entry.id.clone())
    });
    // Enabled while the details load; the launch picks the entry then.
    let can_play = game.is_none() || playable_entry.is_some();
    let starting = menus.starting(&target.game_id) && running.is_none();

    let start = {
        let target = target.clone();
        let running = running.clone();
        let playable_entry = playable_entry.clone();
        use_callback(move |()| {
            if let Some(session) = running.clone() {
                stopping.set(true);
                spawn(async move {
                    if let Err(report) = actions::stop(session.id).await {
                        stop_error.set(Some(report.summary));
                    }
                    stopping.set(false);
                });
                return;
            }
            if !can_play || starting {
                return;
            }
            menus.close_look();
            menus.dialog(
                target.clone(),
                super::MenuDialog::Play {
                    entry_id: playable_entry.clone(),
                    action: None,
                },
            );
        })
    };
    let play_label = if running.is_some() {
        if stopping() {
            t!("Stopping…")
        } else {
            tp!("game action", "Stop")
        }
    } else if starting {
        t!(note = "The Play button while the game starts", "Starting…")
    } else {
        tp!("game action", "Play")
    };
    {
        use_input_action(
            Action::Details,
            Bind {
                owner: panel(),
                label: Some(play_label.clone()),
                ..Bind::default()
            },
            move || {
                start(());
                true
            },
        );
    }
    let open = {
        let game_id = target.game_id.clone();
        move |_| {
            menus.close_look();
            router.open_game(&game_id);
        }
    };

    let (status, platform, cover, facts, genres, description) = match &game {
        Some(game) => {
            let installed = list.iter().any(|entry| entry.installed);
            let status = if installed {
                GameStatus::Installed
            } else {
                GameStatus::NotInstalled
            };
            let cover = game
                .artwork
                .iter()
                .find(|art| art.kind == caskade_core::model::ArtworkKind::Hero)
                .or_else(|| {
                    game.artwork
                        .iter()
                        .find(|art| art.kind == caskade_core::model::ArtworkKind::Cover)
                })
                .and_then(|art| crate::library::art_url(Some(art)));
            let size = game.entries.iter().filter_map(|entry| entry.size).max();
            let mut facts = vec![
                (
                    "playtime",
                    t!("Playtime"),
                    format::playtime(game.playtime_ms / 1000),
                ),
                (
                    "last-played",
                    t!("Last played"),
                    format::last_played(
                        game.last_played_at_ms.map(crate::library::local_date_time),
                    ),
                ),
                ("size", t!("Size"), format::size(size)),
            ];
            if let Some(released) = &game.game.release_date {
                facts.push(("released", t!("Released"), released.clone()));
            }
            let mut genres: Vec<String> = Vec::new();
            for genre in game.game.genres.iter().chain(game.game.tags.iter()) {
                if !genres.contains(genre) {
                    genres.push(genre.clone());
                }
            }
            (
                Some(status),
                platform_line(&game.game.platforms),
                cover,
                facts,
                genres,
                game.game.description.clone(),
            )
        }
        None => (None, None, None, Vec::new(), Vec::new(), String::new()),
    };
    let notice = game.as_ref().filter(|_| !can_play).map(|_| {
        if list.is_empty() {
            t!("Nothing to launch.")
        } else {
            t!("Not installed or unavailable.")
        }
    });
    rsx! {
        div {
            role: "dialog",
            "aria-label": "{target.title}",
            "data-cs": "quick-look",
            class: "fixed top-topbar right-0 bottom-9 z-40 flex w-[min(28rem,100%)] flex-col border-l border-border-strong bg-surface-overlay shadow-xl shadow-black/40 animate-enter",
            onmounted: move |event: Event<MountedData>| {
                panel.set(event.data().downcast::<dioxus_native::NodeHandle>().map(|handle| handle.node_id()));
            },
            onkeydown: move |event: KeyboardEvent| {
                if event.key() == Key::Named(NamedKey::Escape) {
                    event.stop_propagation();
                    menus.close_look();
                }
            },
            Button {
                variant: ButtonVariant::Ghost,
                "data-part": "close",
                "aria-label": tp!("dialog", "Close"),
                class: "absolute top-2 right-2 z-10",
                on_press: move |_| menus.close_look(),
                Svg { icon: icons::X, class: "size-4" }
            }
            div { class: "flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-5",
                GameArt { image: cover, part: Some("art"), class: "aspect-video w-full rounded-card" }
                div { class: "flex flex-wrap items-center gap-2 text-sm text-fg-muted",
                    if let Some(status) = status {
                        StatusBadge { status }
                    }
                    if let Some(platform) = platform {
                        span { "data-part": "platform", "{platform}" }
                    }
                    crate::accounts::AgeLock { game_id: target.game_id.clone() }
                }
                h2 { "data-part": "title", class: "font-display text-2xl font-semibold", "{target.title}" }
                div { "data-part": "actions", class: "flex flex-wrap gap-2",
                    Button {
                        variant: ButtonVariant::Primary,
                        "data-part": "play",
                        "data-focus-default": "true",
                        icon: Some(if running.is_some() { icons::SQUARE } else { icons::PLAY }),
                        disabled: !can_play || stopping() || starting,
                        on_press: move |_| start(()),
                        "{play_label}"
                    }
                    Button { "data-part": "open", icon: Some(icons::INFO), on_press: open, {t!("View more")} }
                }
                if let Some(error) = stop_error() {
                    p { role: "alert", class: "text-sm text-danger", "{error}" }
                }
                if let Some(notice) = notice {
                    p { "data-part": "notice", class: "text-sm text-fg-muted", "{notice}" }
                }
                if !facts.is_empty() {
                    dl { "data-part": "facts", class: "grid grid-cols-2 gap-x-4 gap-y-2 text-sm",
                        for (part, label, value) in facts {
                            div { key: "{part}", "data-part": part,
                                dt { class: "text-xs text-fg-subtle", "{label}" }
                                dd { "{value}" }
                            }
                        }
                    }
                }
                if !genres.is_empty() {
                    div { "data-part": "genres", class: "flex flex-wrap gap-1.5",
                        for genre in genres {
                            span { key: "{genre}", class: "rounded-full border border-border px-2 py-0.5 text-xs text-fg-muted", "{genre}" }
                        }
                    }
                }
                if !description.is_empty() {
                    p { "data-part": "description", class: "text-sm text-fg-muted", "{description}" }
                }
            }
        }
    }
}
