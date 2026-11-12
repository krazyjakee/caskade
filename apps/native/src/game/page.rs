//! A game's page (#100, the webview's `views/game/GameView.tsx`): the way
//! back, the hero (art, status, title, what it is, playtime, description,
//! tags, its site), Play or Stop with the release Play starts, the game's
//! releases, each with its own Play when there are several, and its
//! session history. Right clicks on the hero and on a release open the
//! game's menu. Play goes through `game_menu::play::PlayRequest`, so it
//! asks what every Play asks first.

use super::back::GameBackLink;
use super::present;
use crate::data::{change, use_game, use_library};
use crate::game_menu::entries::{EntryView, entries, playable};
use crate::game_menu::{Target, use_game_menus, use_sessions};
use crate::ui::{
    Button, ButtonVariant, GameArt, GameStatus, Select, SelectOption, StatusBadge, Svg, icons,
};
use caskade_core::model::{GameDetails, InstallState, LaunchSpec, PlaySession};
use caskade_ui_i18n::{format, t, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;
use std::collections::HashSet;

/// The game's own site: its web release's page.
fn website(game: &GameDetails) -> Option<String> {
    game.entries.iter().find_map(|entry| match &entry.launch {
        Some(LaunchSpec::Web { url, .. }) => Some(url.clone()),
        _ => None,
    })
}

/// A URL's host, without `www.`.
fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .map(|host| host.trim_start_matches("www.").to_owned())
        .unwrap_or_default()
}

/// The page's status: running, emulated, installed or not.
fn status(game: &GameDetails, list: &[EntryView]) -> GameStatus {
    if list.iter().any(|entry| entry.emulated) {
        GameStatus::Emulated
    } else if game.entries.iter().any(|entry| {
        matches!(
            entry.state,
            InstallState::Installed | InstallState::Updating | InstallState::Broken { .. }
        )
    }) {
        GameStatus::Installed
    } else {
        GameStatus::NotInstalled
    }
}

/// What each release is called: by its source; names two share add it.
fn release_names(list: &[EntryView]) -> Vec<(String, String)> {
    let base: Vec<String> = list
        .iter()
        .map(|entry| present::source(&entry.source))
        .collect();
    list.iter()
        .zip(&base)
        .map(|(entry, name)| (entry.id.clone(), name.clone()))
        .collect()
}

#[component]
pub fn GameView(id: String, from: Option<String>) -> Element {
    let game = use_game(id.clone());
    let read = game.read();
    match &*read {
        None => rsx! {
            p { role: "status", class: "py-12 text-fg-muted", {t!("Loading game…")} }
        },
        Some(Err(error)) => {
            let summary = error.summary.clone();
            rsx! {
                div { role: "alert", class: "py-12",
                    h1 { {t!("Could not load this game")} }
                    p { class: "text-fg-muted", "{summary}" }
                    Button {
                        on_press: move |_| {
                            let mut game = game;
                            game.restart();
                        },
                        {t!("Try again")}
                    }
                }
            }
        }
        Some(Ok(None)) => rsx! {
            section { class: "py-12",
                h1 { class: "font-display text-2xl", {t!("Game not found")} }
                GameBackLink { game_id: id.clone(), from: from.clone() }
            }
        },
        Some(Ok(Some(details))) => rsx! {
            Page { key: "{id}", game: details.clone(), from: from.clone() }
        },
    }
}

#[component]
fn Page(game: GameDetails, from: Option<String>) -> Element {
    let menus = use_game_menus();
    let edits = super::editor::use_edits();
    let library = use_library();
    let sessions_query = use_sessions(game.game.id.clone());
    let sessions: Option<Result<Vec<PlaySession>, String>> = sessions_query
        .read()
        .as_ref()
        .map(|list| list.clone().map_err(|error| error.summary));
    let running = sessions
        .as_ref()
        .and_then(|list| list.as_ref().ok())
        .and_then(|list| list.iter().find(|session| session.ended_at_ms.is_none()))
        .cloned();
    let mut hero = use_signal(|| None::<NodeHandle>);
    let mut default_error = use_signal(|| None::<String>);
    let mut stopping = use_signal(|| false);
    let mut stop_error = use_signal(|| None::<String>);

    let data = super::data::use_page_data();
    // The hero's achievement summary moves to the achievements' heading.
    use_context_provider(|| super::achievements::AchievementsHeading(Signal::new(None)));
    let mut moved = use_context_provider(|| super::achievements::PageMoved(Signal::new(0))).0;
    let updates = {
        let (data, game_id) = (data.clone(), game.game.id.clone());
        super::data::use_answer(caskade_ui_data::Key::Game(game_id.clone()), move || {
            (data.updates)(game_id.clone())
        })
    };
    let updates: Vec<caskade_core::updates::GameUpdate> = updates
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok().cloned())
        .unwrap_or_default();
    let router = crate::router::use_router();
    // IGDB's videos, from the game's match.
    let igdb = {
        let (data, game_id) = (data.clone(), game.game.id.clone());
        super::data::use_answer(caskade_ui_data::Key::Game(game_id.clone()), move || {
            (data.igdb)(game_id.clone())
        })
    };
    let igdb_videos: Vec<caskade_core::igdb::IgdbVideo> = igdb
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok().cloned().flatten())
        .and_then(|matched| matched.game)
        .map(|game| game.videos)
        .unwrap_or_default();
    let list = entries(&game);
    // Other devices' copies are on their own panel, not releases here.
    let releases: Vec<EntryView> = list
        .iter()
        .filter(|entry| !entry.source.starts_with("peer:"))
        .cloned()
        .collect();
    let several = releases.len() > 1;
    let names = release_names(&list);
    let name_of = |id: &str| {
        names
            .iter()
            .find(|(entry, _)| entry == id)
            .map(|(_, name)| name.clone())
            .unwrap_or_default()
    };
    // Play starts the release the user chose, else the first that can.
    let none = HashSet::new();
    let chosen: Option<EntryView> = game
        .game
        .default_entry
        .as_ref()
        .and_then(|id| list.iter().find(|entry| &entry.id == id))
        .or_else(|| playable(&list, &none, None))
        .cloned();
    // An emulated release starts only with its emulator ready (or asking).
    let mut chosen_emulated = use_signal(|| None::<String>);
    let wanted = chosen
        .as_ref()
        .filter(|entry| entry.emulated)
        .map(|entry| entry.id.clone());
    if *chosen_emulated.peek() != wanted {
        chosen_emulated.set(wanted);
    }
    let emulation = {
        let data = data.clone();
        super::data::use_answer(
            caskade_ui_data::Key::Game(game.game.id.clone()),
            move || {
                let (data, entry) = (data.clone(), chosen_emulated());
                Box::pin(async move {
                    match entry {
                        Some(entry) => (data.emulation)(entry).await.map(Some),
                        None => Ok(None),
                    }
                })
            },
        )
    };
    let emulator_ready = chosen.as_ref().is_none_or(|entry| !entry.emulated)
        || matches!(&*emulation.read(), Some(Ok(Some(game))) if super::emulation::can_start(Some(game)));
    let starting = menus.is_some_and(|menus| menus.starting(&game.game.id));
    let busy = starting || running.is_some();
    let target = {
        let (game_id, title) = (game.game.id.clone(), game.game.title.clone());
        move |entry_id: Option<String>| Target {
            game_id: game_id.clone(),
            title: title.clone(),
            entry_id,
        }
    };
    let play_label = if running.is_some() {
        t!(note = "The Play button while the game runs", "Running")
    } else if starting {
        t!("Starting…")
    } else {
        t!(note = "Button: starts the game", "Play")
    };
    let label_for = |entry: Option<&EntryView>| {
        if entry.is_some_and(|entry| entry.stream) && !busy {
            t!(
                note = "Button: streams the game from another device",
                "Stream"
            )
        } else {
            play_label.clone()
        }
    };
    let launchable = |entry: &EntryView| entry.can_launch;
    let state_of = |id: &str| {
        game.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.state.clone())
    };
    let release_label = |entry: &EntryView| {
        let name = name_of(&entry.id);
        if launchable(entry) {
            name
        } else {
            let state = state_of(&entry.id)
                .map(|state| present::install_state(&state))
                .unwrap_or_default();
            t!(
                note = "A release and its install state",
                "{name} ({state})",
                name = name,
                state = state
            )
        }
    };
    let status = status(&game, &list);
    let meta: Vec<String> = [
        game.game.genres.join(" · "),
        game.game.platforms.join(" · "),
        game.entries
            .iter()
            .filter_map(|entry| entry.size)
            .max()
            .map(|size| format::size(Some(size)))
            .unwrap_or_default(),
        game.game
            .release_date
            .as_deref()
            .map(|date| date.chars().take(4).collect())
            .unwrap_or_default(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect();
    let meta = meta.join(" · ");
    let playtime = format::playtime(game.playtime_ms / 1000);
    let site = website(&game);
    let hero_art = crate::library::art_url_hero(&game.artwork);
    let open_menu = {
        let game_id = game.game.id.clone();
        let title = game.game.title.clone();
        move |entry_id: Option<String>, event: MouseEvent| {
            event.stop_propagation();
            if let Some(menus) = menus {
                let anchor = crate::frame::pointer_anchor(hero.peek().as_ref(), &event);
                menus.open(
                    Target {
                        game_id: game_id.clone(),
                        title: title.clone(),
                        entry_id,
                    },
                    anchor,
                );
            }
        }
    };
    let play = {
        let target = target.clone();
        move |entry_id: String| {
            if let Some(menus) = menus {
                menus.play(target(Some(entry_id)));
            }
        }
    };
    let mut stop = move |session: String| {
        stopping.set(true);
        stop_error.set(None);
        spawn(async move {
            if let Err(report) = crate::game_menu::actions::stop(session).await {
                stop_error.set(Some(report.summary));
            }
            stopping.set(false);
        });
    };
    let set_default = {
        let library = library.clone();
        let game_id = game.game.id.clone();
        move |entry_id: String| {
            let (library, game_id) = (library.clone(), game_id.clone());
            default_error.set(None);
            spawn(async move {
                if let Err(report) =
                    change(
                        async move { library.set_default_entry(&game_id, Some(&entry_id)).await },
                    )
                    .await
                {
                    default_error.set(Some(report.summary));
                }
            });
        }
    };
    let release_options: Vec<SelectOption> = releases
        .iter()
        .map(|entry| SelectOption::new(entry.id.clone(), release_label(entry)))
        .collect();
    let updating = chosen
        .as_ref()
        .and_then(|entry| state_of(&entry.id))
        .is_some_and(|state| state == InstallState::Updating);

    rsx! {
        article {
            "data-view": "game",
            "data-status": if running.is_some() { "running" } else { status.name() },
            class: "mt-4",
            onwheel: move |_| moved += 1,
            GameBackLink { game_id: game.game.id.clone(), from: from.clone(), class: "text-sm text-fg-muted" }
            section {
                "data-cs": "hero",
                class: "relative mt-4 overflow-hidden rounded-panel border border-border bg-surface-raised",
                onmounted: move |event: Event<MountedData>| hero.set(event.data().downcast::<NodeHandle>().cloned()),
                oncontextmenu: {
                    let open_menu = open_menu.clone();
                    move |event: MouseEvent| open_menu(None, event)
                },
                GameArt { part: "art", image: hero_art, class: "h-56 w-full" }
                div { "data-part": "content", class: "flex flex-wrap items-start gap-6 p-6",
                    div { class: "min-w-0 flex-[1_1_32rem]",
                        StatusBadge { part: "badge", status }
                        h1 { "data-part": "title", class: "mt-3 font-display text-4xl font-semibold", "{game.game.title}" }
                        p { "data-part": "meta", class: "mt-2 text-sm text-fg-muted",
                            "{meta}"
                            super::media::AgeRating { game_id: game.game.id.clone() }
                            crate::accounts::AgeLock { game_id: game.game.id.clone(), separated: true }
                        }
                        p { "data-part": "meta", class: "mt-3 flex items-center gap-2 text-fg-muted",
                            Svg { icon: icons::CLOCK, class: "size-4" }
                            span { "{playtime}" }
                            super::elsewhere::PlaytimeSources { game_id: game.game.id.clone() }
                            super::achievements::AchievementSummary { game_id: game.game.id.clone() }
                        }
                        super::elsewhere::PlayingElsewhereLine { game_id: game.game.id.clone() }
                        p { "data-part": "description", class: "mt-4 max-w-3xl text-sm leading-relaxed text-fg-muted",
                            if game.game.description.is_empty() { {t!("No description yet.")} } else { "{game.game.description}" }
                        }
                        div { "data-part": "tags", class: "mt-4 flex flex-wrap gap-2",
                            for tag in game.game.tags.clone() {
                                span { key: "{tag}", class: "rounded-full border border-border px-3 py-1 text-xs text-fg-muted", "{tag}" }
                            }
                        }
                        if let Some(site) = site {
                            super::ExternalLink {
                                href: site.clone(),
                                "data-part": "website",
                                class: "mt-4 inline-flex items-center gap-1.5 text-sm text-accent-alt",
                                {t!(note = "A link to a game's website", "Official site")}
                                span { class: "text-fg-subtle", {host_of(&site)} }
                            }
                        }
                    }
                    div { "data-part": "actions", class: "flex w-full flex-col gap-3 sm:w-80",
                        if let Some(session) = running.clone() {
                            div { class: "flex flex-col items-stretch gap-1",
                                span { "data-part": "playing", class: "font-mono text-xs tracking-widest text-accent-alt uppercase",
                                    if list.iter().any(|entry| entry.id == session.entry_id && entry.stream) {
                                        {t!(note = "The game is streaming from another device", "Streaming")}
                                    } else {
                                        {t!(note = "The game is running", "Playing")}
                                    }
                                }
                                Button {
                                    "data-part": "stop",
                                    "data-focus-default": "true",
                                    variant: ButtonVariant::Danger,
                                    icon: Some(icons::SQUARE),
                                    disabled: stopping(),
                                    on_press: {
                                        let id = session.id.clone();
                                        move |_| stop(id.clone())
                                    },
                                    if stopping() { {t!("Stopping…")} } else { {tp!("game action", "Stop")} }
                                }
                            }
                        } else {
                            Button {
                                "data-part": "play",
                                "data-focus-default": "true",
                                variant: ButtonVariant::Primary,
                                icon: Some(icons::PLAY),
                                disabled: chosen.as_ref().is_none_or(|entry| !launchable(entry)) || busy || !emulator_ready,
                                on_press: {
                                    let chosen = chosen.clone();
                                    let play = play.clone();
                                    move |_| {
                                        if let Some(entry) = &chosen {
                                            play(entry.id.clone());
                                        }
                                    }
                                },
                                {label_for(chosen.as_ref())}
                            }
                        }
                        if running.is_none() && updating {
                            p { role: "status", class: "text-sm text-fg-muted",
                                {t!("This release is being updated. Play is available again when the update finishes.")}
                            }
                        }
                        if several && let Some(entry) = chosen.clone() {
                            Select {
                                label: t!(note = "Picks which of the game's releases Play starts", "Release"),
                                "data-part": "release",
                                options: release_options,
                                value: Some(entry.id.clone()),
                                on_change: {
                                    let mut set_default = set_default.clone();
                                    move |id: String| set_default(id)
                                },
                            }
                        }
                        if running.is_none() && let Some(entry) = chosen.clone() {
                            super::play_choice::PlayChoice {
                                game_id: game.game.id.clone(),
                                title: game.game.title.clone(),
                                entry_id: entry.id.clone(),
                                disabled: busy,
                            }
                        }
                        if running.is_none() {
                            super::elsewhere::OwnedReleases {
                                game_id: game.game.id.clone(),
                                primary: !list.iter().any(|entry| entry.can_launch),
                            }
                            super::elsewhere::OtherDevices {
                                game_id: game.game.id.clone(),
                                primary: !list.iter().any(|entry| entry.can_launch),
                            }
                        }
                        if edits && let Some(menus) = menus {
                            Button {
                                "data-part": "edit",
                                "data-spatial": "focusable",
                                icon: Some(icons::PENCIL),
                                on_press: {
                                    let target = target.clone();
                                    move |_| menus.dialog(target(None), crate::game_menu::MenuDialog::Edit)
                                },
                                {t!("Edit details")}
                            }
                        }
                        if let Some(error) = default_error() {
                            p { role: "alert", class: "text-sm text-danger", "{error}" }
                        }
                        if let Some(error) = stop_error() {
                            p { role: "alert", class: "text-sm text-danger", "{error}" }
                        }
                    }
                }
            }
            super::media::Screenshots { title: game.game.title.clone(), shots: super::media::shots(&game.artwork) }
            super::media::Videos { title: game.game.title.clone(), videos: igdb_videos }
            section { class: "mt-8", "aria-labelledby": "entries-heading",
                h2 { id: "entries-heading", class: "font-display text-xl font-semibold", {t!("Play from your library")} }
                if releases.is_empty() {
                    p { class: "mt-4 text-fg-muted",
                        {t!("Caskade can't start this game: it has nothing to launch. Add a program or URI in Edit details → Actions.")}
                    }
                }
                div { class: "mt-4 grid grid-cols-[repeat(auto-fit,minmax(min(100%,34rem),1fr))] gap-3",
                    for entry in releases.clone() {
                        Release {
                            key: "{entry.id}",
                            name: name_of(&entry.id),
                            state: state_of(&entry.id),
                            is_default: several && chosen.as_ref().is_some_and(|chosen| chosen.id == entry.id),
                            several,
                            playing: running.as_ref().is_some_and(|session| session.entry_id == entry.id),
                            running: running.is_some(),
                            busy,
                            play_label: label_for(Some(&entry)),
                            game_id: game.game.id.clone(),
                            update: updates.iter().find(|update| update.entry_id == entry.id).cloned(),
                            on_play: {
                                let play = play.clone();
                                move |id: String| play(id)
                            },
                            on_menu: {
                                let open_menu = open_menu.clone();
                                move |(id, event): (String, MouseEvent)| open_menu(Some(id), event)
                            },
                            entry,
                        }
                    }
                }
            }
            super::saves::SavesPanel {
                game_id: game.game.id.clone(),
                title: game.game.title.clone(),
                entry_id: chosen.as_ref().map(|entry| entry.id.clone()),
                running: running.is_some(),
            }
            super::ports::PortsPanel { game_id: game.game.id.clone() }
            super::achievements::GameAchievements { game_id: game.game.id.clone() }
            Sessions { sessions }
            super::igdb::IgdbPanel {
                game_id: game.game.id.clone(),
                title: game.game.title.clone(),
                on_merged: {
                    let from = from.clone();
                    move |merged: String| {
                        router.replace(&crate::router::Route::Game { id: merged, from: from.clone() }.href())
                    }
                },
            }
        }
    }
}

/// One of the game's releases: its name, source and state, where it is,
/// moving its files, and its own Play when there are several.
#[component]
fn Release(
    entry: EntryView,
    name: String,
    state: Option<InstallState>,
    is_default: bool,
    several: bool,
    /// It's the one running.
    playing: bool,
    /// One of the game's releases is running.
    running: bool,
    busy: bool,
    play_label: String,
    game_id: String,
    /// Its updates, when a provider follows it.
    update: Option<caskade_core::updates::GameUpdate>,
    on_play: EventHandler<String>,
    on_menu: EventHandler<(String, MouseEvent)>,
) -> Element {
    let source = present::source(&entry.source);
    let state_name = state
        .as_ref()
        .map(present::install_state)
        .unwrap_or_default();
    let line = if name == source {
        state_name
    } else {
        format!("{source} · {state_name}")
    };
    let menu_id = entry.id.clone();
    let play_id = entry.id.clone();
    rsx! {
        div {
            "data-part": "entry",
            "data-status": state.as_ref().map(present::state_id).unwrap_or("installed"),
            class: "flex min-w-0 flex-col gap-3 rounded-panel border border-border bg-surface-raised p-4",
            oncontextmenu: move |event: MouseEvent| on_menu.call((menu_id.clone(), event)),
            div { class: "flex flex-wrap items-start justify-between gap-3",
                div { class: "min-w-0 flex-[1_1_16rem]",
                    h3 { class: "flex flex-wrap items-center gap-2 font-semibold",
                        "{name}"
                        if is_default {
                            span { "data-part": "default", class: "rounded-full border border-accent px-2 py-0.5 text-xs font-normal text-accent-alt",
                                {tp!(note = "The release the game's Play starts", "release", "Default")}
                            }
                        }
                    }
                    if entry.stream {
                        p { "data-part": "device", class: "text-xs text-fg-subtle", {tp!("streaming device", "Ready")} }
                    } else {
                        p { class: "text-xs text-fg-subtle", "{line}" }
                        p { class: "mt-1 break-all font-mono text-xs text-fg-muted",
                            {entry.location.clone().unwrap_or_else(|| t!("Install location unknown"))}
                        }
                    }
                }
                div { class: "flex flex-wrap items-start justify-end gap-2",
                    if entry.managed() {
                        crate::flows::storage::MoveGameAction {
                            game_id: game_id.clone(),
                            entry_id: entry.id.clone(),
                            is_running: running,
                        }
                    }
                    if several {
                        Separate { entry_id: entry.id.clone(), running: playing }
                    }
                    if several {
                        if playing {
                            span { "data-part": "playing", class: "self-center font-mono text-xs tracking-widest text-accent-alt uppercase",
                                {t!(note = "The game is running", "Playing")}
                            }
                        } else {
                            Button {
                                "data-part": "play",
                                variant: ButtonVariant::Primary,
                                icon: Some(icons::PLAY),
                                disabled: !entry.can_launch || busy,
                                on_press: move |_| on_play.call(play_id.clone()),
                                "{play_label}"
                            }
                        }
                    }
                }
            }
            if entry.emulated {
                super::emulation::EmulatedEntry { game_id: game_id.clone(), entry_id: entry.id.clone() }
            }
            if let Some(update) = update {
                super::entry_update::EntryUpdate { update, is_running: running }
            }
        }
    }
}

/// On a game with several releases (merged because IGDB counts them as one
/// game): moves this one out into a game of its own. Matching never merges
/// it back.
#[component]
fn Separate(entry_id: String, running: bool) -> Element {
    let data = super::data::use_page_data();
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    rsx! {
        div { class: "flex shrink-0 flex-col items-end gap-1",
            Button {
                "data-part": "separate",
                disabled: running || pending(),
                on_press: move |_| {
                    let (data, entry_id) = (data.clone(), entry_id.clone());
                    pending.set(true);
                    error.set(None);
                    spawn(async move {
                        if let Err(report) = (data.separate)(entry_id).await {
                            error.set(Some(report.summary));
                        }
                        pending.set(false);
                    });
                },
                if pending() {
                    {t!("Separating…")}
                } else {
                    {t!(note = "Button: moves this release out of the merged game into a game of its own", "Separate")}
                }
            }
            if let Some(error) = error() {
                p { role: "alert", class: "text-xs text-danger", "{error}" }
            }
        }
    }
}

/// The game's play sessions, newest first.
#[component]
fn Sessions(sessions: Option<Result<Vec<PlaySession>, String>>) -> Element {
    rsx! {
        section { class: "mt-8", "aria-labelledby": "sessions-heading",
            h2 { id: "sessions-heading", class: "font-display text-xl font-semibold", {t!("Session history")} }
            match sessions {
                None => rsx! { p { role: "status", class: "mt-3 text-fg-muted", {t!("Loading sessions…")} } },
                Some(Err(_)) => rsx! { p { role: "alert", class: "mt-3 text-fg-muted", {t!("Could not load session history.")} } },
                Some(Ok(list)) if list.is_empty() => rsx! {
                    p { class: "mt-3 text-fg-muted", {t!("Your first session will appear here.")} }
                },
                Some(Ok(list)) => rsx! {
                    ol { "data-part": "sessions", class: "mt-4 space-y-2",
                        for session in list {
                            li { key: "{session.id}", class: "flex justify-between gap-4 rounded-control border border-border px-4 py-3 text-sm",
                                span { {format::day(crate::library::local_date_time(session.started_at_ms))} }
                                span { class: "font-mono text-fg-muted",
                                    if session.ended_at_ms.is_some() {
                                        {format::duration(session.duration_ms / 1000)}
                                    } else {
                                        {t!("Playing now")}
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}
