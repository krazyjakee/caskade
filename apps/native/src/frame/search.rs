//! The top bar's search (`shell/SearchBox.tsx`): games suggested as you
//! type. Enter, or "See all results", opens the library on the text;
//! choosing a suggestion opens its game. Ctrl K (⌘ K) focuses it from
//! anywhere, and the library's search shows in it.

use crate::data::sleep;
use crate::data::use_library_search;
use crate::input::{Bind, use_input, use_input_action};
use crate::router::{Route, href::Href, use_router};
use crate::ui::fold;
use crate::ui::{Anchor, GameArt, GameStatus, Popover, StatusBadge, Svg, icons, use_active_option};
use caskade_core::model::GameSummary;
use caskade_ui_i18n::{t, tn, tp};
use caskade_ui_input::{Action, InputSource};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;
use std::time::Duration;

/// How long typing pauses before the suggestions are fetched.
const SUGGEST_DELAY: Duration = Duration::from_millis(150);

/// Where `query` first occurs in `text`, case and accents aside, as byte
/// offsets into `text` (`findFolded`).
pub fn find_folded(text: &str, query: &str) -> Option<(usize, usize)> {
    let wanted = fold(query.trim());
    if wanted.is_empty() {
        return None;
    }
    // Each folded character's place in `text`.
    let mut folded = String::new();
    let mut places: Vec<(usize, usize)> = Vec::new();
    for (at, c) in text.char_indices() {
        for part in fold(&c.to_string()).chars() {
            folded.push(part);
            places.push((at, at + c.len_utf8()));
        }
    }
    let found = folded.find(&wanted)?;
    let first = folded[..found].chars().count();
    let last = first + wanted.chars().count() - 1;
    Some((places.get(first)?.0, places.get(last)?.1))
}

/// `text` with its first match of `query` marked.
#[component]
fn Highlight(text: String, query: String) -> Element {
    match find_folded(&text, &query) {
        Some((start, end)) => rsx! {
            "{&text[..start]}"
            mark { class: "bg-transparent text-accent-alt", "{&text[start..end]}" }
            "{&text[end..]}"
        },
        None => rsx! { "{text}" },
    }
}

#[component]
pub fn SearchBox() -> Element {
    // The pad family for the shortcut in console mode (#107).
    let family = crate::console::use_family();
    let router = use_router();
    let input = use_input();
    let list_id = "search-suggestions";
    // The library's search shows here, so it says what the results are for.
    let route = router.route();
    let library_q = match &route {
        Route::Library(library) => library.q.clone(),
        _ => None,
    };
    let mut text = use_signal(|| library_q.clone().unwrap_or_default());
    let mut synced = use_signal(|| library_q.clone());
    if *synced.peek() != library_q {
        synced.set(library_q.clone());
        text.set(library_q.clone().unwrap_or_default());
    }
    // Suggestions follow the first keystroke, then the typing once it
    // pauses.
    let mut typed = use_signal(String::new);
    let mut typing = use_signal(|| 0u64);
    let query = use_memo(move || {
        let text = text();
        if text.trim().is_empty() {
            String::new()
        } else {
            typed()
        }
    });
    let suggestions = use_library_search(query.into());
    let mut field = use_signal(|| None::<NodeHandle>);
    let mut open = use_signal(|| None::<Anchor>);
    // An arrow made a suggestion active: Enter chooses it, not the search.
    let mut arrowed = use_signal(|| false);

    let current = text();
    let has_query = !current.trim().is_empty();
    let page = suggestions.read().clone();
    let (games, total): (Vec<GameSummary>, u64) = match (&page, has_query) {
        (Some(Ok(page)), true) => (page.items.clone(), page.total),
        _ => (Vec::new(), 0),
    };

    let search = use_callback(move |q: String| {
        let q = q.trim().to_owned();
        open.set(None);
        if q.is_empty() {
            return;
        }
        text.set(q.clone());
        let href = Href::new("/library")
            .text("filter", Some("all"))
            .text("q", Some(&q))
            .to_string();
        router.navigate(&href);
    });
    let show = use_callback(move |()| {
        if open.peek().is_none()
            && let Some(anchor) = field.peek().as_ref().and_then(Anchor::of)
        {
            open.set(Some(anchor));
        }
    });
    let ids: Vec<String> = games.iter().map(|game| game.id.clone()).collect();
    // A suggestion opens its game; the last row searches.
    let choose = use_callback(move |index: usize| match ids.get(index) {
        Some(id) => {
            open.set(None);
            router.open_game(id);
        }
        None => {
            let q = text.peek().clone();
            search(q);
        }
    });
    let options = games.len() + usize::from(has_query);
    let active = use_active_option(
        options,
        open().is_some(),
        field.peek().as_ref().map(NodeHandle::node_id),
        move || show(()),
        move |index| choose.call(index),
        move || open.set(None),
    );
    let active_index = active.index().filter(|_| arrowed());

    // Search (a pad's Y, `/` or F3 in the keyboard map): a pad types here
    // on the on-screen keyboard; keys open the search overlay.
    let open_overlay = try_use_context::<super::type_search::OpenTypeSearch>();
    let searcher = input.clone();
    use_input_action(
        Action::Search,
        Bind {
            label: Some(tp!("action", "Search")),
            ..Bind::default()
        },
        move || {
            let pad = searcher.source() == Some(InputSource::Pad);
            if !pad && let Some(open) = open_overlay {
                open.0.call(String::new());
                return true;
            }
            let Some(handle) = field.peek().clone() else {
                return false;
            };
            searcher.focus(handle.node_id());
            if pad {
                searcher.open_keyboard(handle.node_id());
            }
            true
        },
    );

    // Ctrl K focuses it, its text selected.
    use_hook(move || {
        input.on_search_key(move || {
            if let Some(handle) = field.peek().as_ref() {
                let mut doc = handle.doc_mut();
                doc.set_focus_to(handle.node_id());
            }
        });
    });

    rsx! {
        div {
            "data-cs": "search",
            // The padding around the input belongs to the search, not the
            // title bar.
            "data-window-drag": "false",
            "data-open": open().filter(|_| has_query).map(|_| "true"),
            class: "relative w-full max-w-xl",
            div {
                "data-part": "control",
                class: "flex h-11 items-center gap-3 rounded-control border border-border bg-surface-raised px-4 text-sm transition-[border-color,box-shadow] duration-(--cs-motion-duration-base) focus-within:border-accent-alt/50 focus-within:shadow-[0_0_20px_-8px] focus-within:shadow-accent-alt",
                Svg { icon: icons::SEARCH, part: "icon", class: "size-4 shrink-0 text-fg-subtle" }
                input {
                    "data-part": "input",
                    r#type: "text",
                    role: "combobox",
                    "aria-label": t!("Search games"),
                    "aria-autocomplete": "list",
                    "aria-expanded": if open().is_some() { "true" } else { "false" },
                    "aria-controls": list_id,
                    "aria-activedescendant": active_index.map(|index| format!("{list_id}-{index}")),
                    placeholder: t!("Search any game… (classic, modern, or something obscure)"),
                    value: "{current}",
                    class: "w-full bg-transparent placeholder:text-fg-subtle focus:outline-none",
                    onmounted: move |event: Event<MountedData>| {
                        field.set(event.data().downcast::<NodeHandle>().cloned());
                    },
                    oninput: move |event: FormEvent| {
                        let value = event.value();
                        let first = text.peek().trim().is_empty();
                        text.set(value.clone());
                        arrowed.set(false);
                        active.reset();
                        if value.trim().is_empty() {
                            open.set(None);
                            return;
                        }
                        if first {
                            typed.set(value.clone());
                        }
                        show(());
                        let generation = *typing.peek() + 1;
                        typing.set(generation);
                        spawn(async move {
                            sleep(SUGGEST_DELAY).await;
                            if *typing.peek() == generation {
                                typed.set(text.peek().clone());
                            }
                        });
                    },
                    onkeydown: move |event: KeyboardEvent| {
                        match event.key() {
                            Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp) if has_query => {
                                arrowed.set(true);
                                active.key_down(&event);
                            }
                            Key::Named(NamedKey::Enter) if !event.is_composing() => {
                                event.prevent_default();
                                event.stop_propagation();
                                if arrowed() && active.choose() {
                                    return;
                                }
                                let q = text.peek().clone();
                                search(q);
                            }
                            _ => active.key_down(&event),
                        }
                    },
                }
                kbd {
                    "data-part": "shortcut",
                    class: "whitespace-nowrap rounded border border-border-strong px-1.5 py-0.5 font-mono text-[10px] text-fg-subtle",
                    // Console mode names the pad's search button (Y), or `/`
                    // on the keyboard (#107).
                    if crate::console::is_console() {
                        {crate::console::glyph(caskade_ui_input::Action::Search, family).unwrap_or_default()}
                    } else {
                        {tp!(note = "The keyboard shortcut that focuses the search", "keyboard key",
                            "CTRL K"
                        )}
                    }
                }
            }
            if let Some(anchor) = open().filter(|_| has_query) {
                Popover {
                    anchor,
                    on_close: move |_| open.set(None),
                    data_cs: "search-popover",
                    role: "listbox",
                    keep_focus: true,
                    match_width: true,
                    id: list_id,
                    "aria-label": t!("Search games"),
                    class: "z-40 mt-1.5 max-h-[28rem] overflow-y-auto rounded-control border border-border-strong bg-surface-overlay p-1 shadow-xl shadow-black/40 outline-none",
                    for (index, game) in games.iter().enumerate() {
                        Suggestion {
                            key: "{game.id}",
                            id: "{list_id}-{index}",
                            game: game.clone(),
                            query: current.clone(),
                            focused: active_index == Some(index),
                            on_choose: move |_| choose(index),
                        }
                    }
                    div {
                        id: "{list_id}-{games.len()}",
                        role: "option",
                        "data-part": "see-all",
                        "data-focused": (active_index == Some(games.len())).then_some("true"),
                        class: "mt-1 flex cursor-default items-center gap-2 rounded-[6px] px-2 py-2 text-sm text-fg-muted data-focused:bg-surface-hover data-focused:text-fg",
                        onclick: move |_| {
                            let q = text.peek().clone();
                            search(q);
                        },
                        Svg { icon: icons::SEARCH, class: "size-4 text-fg-subtle" }
                        span { class: "flex-1 truncate",
                            if total > 0 {
                                {tn!(
                                    "See # result for “{query}”",
                                    "See all # results for “{query}”",
                                    total,
                                    query = current.trim().to_owned()
                                )}
                            } else {
                                {t!(
                                    "No matches. Search the library for “{query}”",
                                    query = current.trim().to_owned()
                                )}
                            }
                        }
                        Svg { icon: icons::CORNER_DOWN_LEFT, class: "size-3.5 text-fg-subtle" }
                    }
                }
            }
        }
    }
}

/// A suggested game: its art, its title and genre and platform with the
/// match marked, and its status.
#[component]
fn Suggestion(
    id: String,
    game: GameSummary,
    query: String,
    focused: bool,
    on_choose: EventHandler<()>,
) -> Element {
    let genre = game.genres.first().cloned().unwrap_or_default();
    let platform = game.platforms.join(", ");
    let cover = crate::library::art_url(game.cover.as_ref());
    let status = if game.installed {
        GameStatus::Installed
    } else {
        GameStatus::NotInstalled
    };
    rsx! {
        div {
            id: "{id}",
            role: "option",
            "data-part": "suggestion",
            "data-focused": focused.then_some("true"),
            class: "flex cursor-default items-center gap-3 rounded-[6px] px-2 py-1.5 data-focused:bg-surface-hover",
            onclick: move |_| on_choose.call(()),
            GameArt { image: cover, class: "size-9 shrink-0 rounded-[4px]" }
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "truncate text-sm",
                    Highlight { text: game.title.clone(), query: query.clone() }
                }
                span { class: "truncate text-xs text-fg-subtle",
                    Highlight { text: genre, query: query.clone() }
                    " · "
                    span { class: "font-mono",
                        Highlight { text: platform, query: query.clone() }
                    }
                }
            }
            if game.admin_pin_needed {
                crate::ui::AgeLockLabel {}
            }
            StatusBadge { status }
        }
    }
}
