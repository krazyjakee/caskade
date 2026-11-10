//! The virtual grid and list (#94): only the rows on screen (and one or a
//! few more each way) are mounted, and only the pages of games they need
//! are loaded, a few kept each side; scrolling 10,000 games mounts the same
//! few dozen cards as 100. Rows have fixed heights and the grid's columns
//! follow its width ([`window`](super::window)).
//!
//! Focus survives scrolling: the focused game's row stays mounted wherever
//! the viewport goes, and LT/RT (Page Up/Down) page through the games, or
//! letter by letter when they're sorted by title. Blitz sends no scroll
//! event for a scroll made from code, so this reads the scroller's offset
//! from the document after its own scrolls and the input layer's
//! (`Input::scrolled`).

use super::art::{self, Variant};
use super::window::{Frame, Layout, PAGE};
use crate::data::{DataSource, change};
use crate::input::{Bind, use_input, use_input_action};
use blitz_dom::{BaseDocument, NodeId, ScrollBehavior};
use caskade_core::model::{GameSummary, ListGames, TitleLetter};
use caskade_ui_data::{Changed, Key as DataKey};
use caskade_ui_i18n::{t, tp};
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use tokio::sync::broadcast;

/// The games loaded so far, by page.
#[derive(Default)]
struct Pages {
    pages: HashMap<usize, Rc<Vec<GameSummary>>>,
    pending: HashSet<usize>,
    /// Bumped when the query changes, so a late page of the old one is
    /// dropped.
    generation: u64,
    query: Option<ListGames>,
}

/// What a view hears from the grid.
#[derive(Clone, PartialEq, Default)]
pub struct Events {
    /// A game was chosen: clicked, or A pressed on it.
    pub open: Option<EventHandler<String>>,
}

#[component]
pub fn VirtualLibrary(
    /// The games: the filter, sort and search. Its offset and limit are the
    /// grid's.
    query: ListGames,
    #[props(default)] layout: Layout,
    /// Where each letter starts, when sorted by title: LT/RT go letter by
    /// letter, and the bar beside the grid jumps to them.
    #[props(default)]
    letters: Vec<TitleLetter>,
    #[props(default)] events: Events,
) -> Element {
    let input = use_input();
    let source = use_context::<DataSource>();
    let pages: Rc<RefCell<Pages>> = use_hook(Rc::default);
    // Bumped when a page arrives or the library changes: renders again.
    let mut loaded = use_signal(|| 0u64);
    let mut total = use_signal(|| None::<usize>);
    let mut scroller = use_signal(|| None::<NodeHandle>);
    let mut focused = use_signal(|| None::<usize>);
    // Bumped by scroll events and window resizes.
    let mut moved = use_signal(|| 0u64);
    // A game to focus once its card is mounted.
    let pending_focus: Rc<RefCell<Option<usize>>> = use_hook(Rc::default);

    // The library's changes load the shown pages again.
    use_hook({
        let pages = pages.clone();
        let mut events = source.events.subscribe();
        move || {
            spawn(async move {
                loop {
                    let stale = match events.recv().await {
                        Ok(event) => Changed::from_event(&event).affects(&DataKey::Games),
                        Err(broadcast::error::RecvError::Lagged(_)) => true,
                        Err(broadcast::error::RecvError::Closed) => break,
                    };
                    if stale {
                        let mut state = pages.borrow_mut();
                        state.pages.clear();
                        state.generation += 1;
                        state.pending.clear();
                        drop(state);
                        loaded += 1;
                    }
                }
            });
        }
    });

    // The scroller's size and offset, as last laid out. Programmatic
    // scrolls (the input layer's, this grid's) subscribe through
    // `scrolled`; the others through `moved`.
    let _ = input.scrolled();
    let _ = moved();
    let _ = loaded();
    let (width, height, scroll_top) = scroller
        .read()
        .as_ref()
        .and_then(|handle| {
            let doc = handle.doc();
            let node = doc.get_node(handle.node_id())?;
            let rect = doc.get_client_bounding_rect(handle.node_id())?;
            Some((rect.width, rect.height, node.scroll_offset().y))
        })
        .unwrap_or((0.0, 0.0, 0.0));

    // A new query starts again from the top.
    {
        let mut state = pages.borrow_mut();
        if state.query.as_ref() != Some(&query) {
            state.query = Some(query.clone());
            state.pages.clear();
            state.pending.clear();
            state.generation += 1;
            pending_focus.borrow_mut().take();
            drop(state);
            if total.peek().is_some() {
                total.set(None);
            }
            if focused.peek().is_some() {
                focused.set(None);
            }
            if let Some(handle) = scroller.peek().as_ref() {
                scroll_to(handle, 0.0, &input);
            }
        }
    }

    let frame = Frame {
        layout,
        total: total().unwrap_or(0),
        width,
        height,
        scroll_top,
    };
    let focus = focused();

    // The pages the mounted rows need (the first one to learn the total);
    // the far ones are let go.
    {
        let mut wanted = frame.needed_pages(focus);
        if total().is_none() {
            wanted = vec![0];
        }
        let mut state = pages.borrow_mut();
        state
            .pages
            .retain(|&page, _| frame.keeps(page, focus) || wanted.contains(&page));
        for page in wanted {
            if state.pages.contains_key(&page) || state.pending.contains(&page) {
                continue;
            }
            state.pending.insert(page);
            let generation = state.generation;
            let pages = pages.clone();
            let library = source.library.clone();
            let page_query = ListGames {
                offset: (page * PAGE) as u32,
                limit: PAGE as u32,
                ..query.clone()
            };
            spawn(async move {
                let result = change(async move { library.page(&page_query).await }).await;
                let mut state = pages.borrow_mut();
                if state.generation != generation {
                    return;
                }
                state.pending.remove(&page);
                match result {
                    Ok(got) => {
                        state.pages.insert(page, Rc::new(got.items));
                        drop(state);
                        let count = usize::try_from(got.total).unwrap_or(usize::MAX);
                        if *total.peek() != Some(count) {
                            total.set(Some(count));
                        }
                        loaded += 1;
                    }
                    Err(error) => tracing::warn!(page, %error, "a library page didn't load"),
                }
            });
        }
    }

    // A game to focus has mounted: focus it, once.
    let mounted = pending_focus.borrow().and_then(|index| {
        let handle = scroller.peek().clone()?;
        let doc = handle.doc();
        card_node(&doc, handle.node_id(), index)
    });
    if let Some(node) = mounted {
        pending_focus.borrow_mut().take();
        input.focus(node);
    }

    // LT/RT: a page at a time, or a letter at a time when sorted by title.
    let paging = {
        let pending_focus = pending_focus.clone();
        let letters = letters.clone();
        let input = input.clone();
        move |forward: bool| {
            let Some(handle) = scroller.peek().clone() else {
                return false;
            };
            let Some(frame) = current_frame(&handle, layout, *total.peek()) else {
                return false;
            };
            if frame.total == 0 {
                return false;
            }
            let from = focused.peek().unwrap_or_else(|| frame.first_shown());
            let to = if letters.is_empty() {
                let step = frame.per_page();
                if forward {
                    (from + step).min(frame.total - 1)
                } else {
                    from.saturating_sub(step)
                }
            } else {
                let at = letters
                    .iter()
                    .rposition(|letter| letter.offset as usize <= from)
                    .unwrap_or(0);
                let here = letters[at].offset as usize;
                let target = if forward {
                    letters.get(at + 1)
                } else if from > here {
                    letters.get(at)
                } else {
                    at.checked_sub(1).and_then(|at| letters.get(at))
                };
                match target {
                    Some(letter) => letter.offset as usize,
                    None => return true,
                }
            };
            scroll_to(&handle, frame.top_of(to), &input);
            *pending_focus.borrow_mut() = Some(to);
            focused.set(Some(to));
            true
        }
    };
    let owner = scroller.read().as_ref().map(NodeHandle::node_id);
    let by_letter = !letters.is_empty();
    let mut back = paging.clone();
    use_input_action(
        Action::PagePrev,
        Bind {
            label: Some(if by_letter {
                t!("Previous letter")
            } else {
                t!("Previous page")
            }),
            owner,
            ..Bind::default()
        },
        move || back(false),
    );
    let mut ahead = paging;
    use_input_action(
        Action::PageNext,
        Bind {
            label: Some(if by_letter {
                t!("Next letter")
            } else {
                t!("Next page")
            }),
            owner,
            ..Bind::default()
        },
        move || ahead(true),
    );

    let state = pages.borrow();
    let game_at = |index: usize| -> Option<GameSummary> {
        let page = state.pages.get(&(index / PAGE))?;
        page.get(index % PAGE).cloned()
    };
    let columns = frame.columns();
    let row_height = layout.row_height();
    let rows = frame.mounted(focus);
    let first_shown = frame.first_shown();
    let current_letter = letters
        .iter()
        .rev()
        .find(|letter| letter.offset as usize <= first_shown)
        .map(|letter| letter.letter.clone());
    let jump_pending = pending_focus.clone();
    let list = layout == Layout::List;
    let content_height = frame.content_height();
    let grid_template = format!("repeat({columns}, minmax(0, 1fr))");

    rsx! {
        // The layout itself is inline: it's how the grid works, not how it
        // looks, and holds without a stylesheet.
        div {
            style: "display: flex; flex: 1 1 0px; min-height: 24rem; gap: 8px;",
            section {
                "data-cs": "virtual-library",
                "data-layout": if list { "list" } else { "grid" },
                // A pad moves between the games; the box itself is a Tab
                // stop for scrolling only.
                "data-focus-section": "library-grid",
                "data-spatial": "skip",
                aria_label: t!("Library games"),
                "aria-busy": if total().is_none() { "true" } else { "false" },
                tabindex: "0",
                class: "overscroll-contain",
                style: "position: relative; min-width: 0; flex-grow: 1; overflow: auto;",
                onmounted: move |event| {
                    if let Some(handle) = event.data().downcast::<NodeHandle>().cloned() {
                        scroller.set(Some(handle));
                        // Measured once laid out: a frame later.
                        spawn(async move {
                            for _ in 0..30 {
                                let _ = caskade_app::runtime::spawn(async {
                                    tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                                })
                                .await;
                                moved += 1;
                                let measured = scroller
                                    .peek()
                                    .as_ref()
                                    .and_then(|handle| {
                                        handle.doc().get_client_bounding_rect(handle.node_id())
                                    })
                                    .is_some_and(|rect| rect.height > 0.0);
                                if measured {
                                    break;
                                }
                            }
                        });
                    }
                },
                onscroll: move |_| moved += 1,
                if list {
                    div {
                        "data-part": "table-header",
                        role: "row",
                        class: "z-20 items-center gap-4 bg-surface-base px-3 text-xs text-fg-muted",
                        style: "position: sticky; top: 0; display: flex; height: {layout.rows_top()}px",
                        span { role: "columnheader", class: "grow pl-10", {tp!("library column", "Title")} }
                        span { role: "columnheader", class: "w-40", {tp!("library column", "Last played")} }
                        span { role: "columnheader", class: "w-24 text-right", {tp!("library column", "Playtime")} }
                    }
                }
                div {
                    "data-part": "rows",
                    style: "position: relative; width: 100%; height: {content_height - layout.rows_top()}px",
                    for row in rows {
                        div {
                            key: "{layout:?}-{columns}-{row}",
                            "data-part": "virtual-row",
                            "data-row": "{row}",
                            style: if list { "position: absolute; top: {row as f64 * row_height}px; left: 0; width: 100%; height: {row_height}px;" } else { "position: absolute; top: {row as f64 * row_height}px; left: 0; width: 100%; height: {row_height}px; box-sizing: border-box; display: grid; gap: 16px; padding-bottom: 16px; grid-template-columns: {grid_template};" },
                            for index in frame.games_in(row) {
                                {
                                    match game_at(index) {
                                        Some(game) => rsx! {
                                            Game {
                                                key: "{game.id}",
                                                game,
                                                index,
                                                list,
                                                focused,
                                                open: events.open,
                                            }
                                        },
                                        None => rsx! {
                                            div {
                                                key: "loading-{index}",
                                                "data-part": "placeholder",
                                                role: "status",
                                                aria_label: t!("Loading game"),
                                                class: "rounded-card border border-border bg-surface-raised",
                                            }
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !letters.is_empty() {
                nav {
                    "data-cs": "alphabet-bar",
                    aria_label: t!("Jump to letter"),
                    class: "flex shrink-0 flex-col items-center gap-0.5 text-xs",
                    for letter in letters.iter().cloned() {
                        {
                            let shown = if letter.letter == "#" {
                                t!("Numbers and symbols")
                            } else {
                                letter.letter.clone()
                            };
                            let current = current_letter.as_deref() == Some(letter.letter.as_str());
                            let offset = letter.offset as usize;
                            let jump_pending = jump_pending.clone();
                            let input = input.clone();
                            rsx! {
                                button {
                                    key: "{letter.letter}",
                                    r#type: "button",
                                    "data-part": "letter",
                                    "data-current": if current { "true" },
                                    aria_label: "{shown}",
                                    class: "rounded px-1 text-fg-muted",
                                    onclick: move |_| {
                                        let Some(handle) = scroller.peek().clone() else {
                                            return;
                                        };
                                        let Some(frame) = current_frame(&handle, layout, *total.peek()) else {
                                            return;
                                        };
                                        scroll_to(&handle, frame.top_of(offset), &input);
                                        *jump_pending.borrow_mut() = Some(offset);
                                        focused.set(Some(offset));
                                    },
                                    "{letter.letter}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The frame as last laid out, outside a render.
fn current_frame(handle: &NodeHandle, layout: Layout, total: Option<usize>) -> Option<Frame> {
    let doc = handle.doc();
    let node = doc.get_node(handle.node_id())?;
    let rect = doc.get_client_bounding_rect(handle.node_id())?;
    Some(Frame {
        layout,
        total: total.unwrap_or(0),
        width: rect.width,
        height: rect.height,
        scroll_top: node.scroll_offset().y,
    })
}

/// Scrolls the grid to `top` at once, and tells what follows scrolling:
/// Blitz sends no scroll event for it.
fn scroll_to(handle: &NodeHandle, top: f64, input: &crate::input::Input) {
    handle
        .doc_mut()
        .scroll_to(handle.node_id(), 0.0, top, ScrollBehavior::Instant);
    input.scrolled_by_code();
}

/// The focus stop of game `index` under the scroller, once mounted.
fn card_node(doc: &BaseDocument, scroller: NodeId, index: usize) -> Option<NodeId> {
    let wanted = index.to_string();
    let mut found = None;
    let mut stack = vec![scroller];
    while let Some(at) = stack.pop() {
        let node = doc.get_node(at)?;
        if node.attr(blitz_dom::LocalName::from("data-index")) == Some(wanted.as_str()) {
            found = Some(at);
            break;
        }
        stack.extend(node.children.iter().copied());
    }
    found
}

/// One game: a card in the grid, a row in the list.
#[component]
fn Game(
    game: GameSummary,
    index: usize,
    list: bool,
    focused: Signal<Option<usize>>,
    open: Option<EventHandler<String>>,
) -> Element {
    let id = game.id.clone();
    let choose = move |_| {
        if let Some(open) = open {
            open.call(id.clone());
        }
    };
    let on_focus = move |_| {
        if *focused.peek() != Some(index) {
            focused.set(Some(index));
        }
    };
    let platform = platform_line(&game.platforms);
    if list {
        let icon = art::url(game.icon.as_ref(), Variant::Icon);
        let last_played = game
            .last_played_at_ms
            .map(super::local_date_time)
            .map(caskade_ui_i18n::format::date_time)
            .unwrap_or_else(|| caskade_ui_i18n::format::last_played(None));
        let playtime = caskade_ui_i18n::format::playtime(game.playtime_ms / 1000);
        return rsx! {
            div {
                "data-cs": "game-row",
                "data-index": "{index}",
                "data-focus-key": "game-{game.id}",
                // A pad starts on the first game.
                "data-focus-default": if index == 0 { "true" },
                role: "row",
                tabindex: "0",
                class: "flex h-full items-center gap-4 border-b border-border px-3",
                onfocusin: on_focus,
                onclick: choose,
                div { class: "size-7 shrink-0 overflow-hidden rounded",
                    if let Some(icon) = icon {
                        img { src: "{icon}", alt: "", class: "size-full object-cover" }
                    }
                }
                span { "data-part": "title", class: "grow truncate text-sm", "{game.title}" }
                span { class: "w-40 truncate text-xs text-fg-muted", "{last_played}" }
                span { class: "w-24 text-right text-xs text-fg-muted", "{playtime}" }
            }
        };
    }
    let cover = art::url(game.cover.as_ref(), Variant::Card);
    rsx! {
        div {
            "data-cs": "game-card",
            "data-index": "{index}",
            "data-focus-key": "game-{game.id}",
            // A pad starts on the first game.
            "data-focus-default": if index == 0 { "true" },
            role: "link",
            tabindex: "0",
            class: "flex min-w-0 flex-col gap-2",
            onfocusin: on_focus,
            onclick: choose,
            div {
                "data-cs": "game-art",
                class: "aspect-[2/3] w-full overflow-hidden rounded-card border border-border bg-surface-raised",
                if let Some(cover) = cover {
                    img { src: "{cover}", alt: "", class: "size-full object-cover" }
                }
            }
            span { "data-part": "title", class: "truncate text-sm font-medium", "{game.title}" }
            if let Some(platform) = platform {
                span { "data-part": "platform", class: "truncate text-xs text-fg-muted", "{platform}" }
            }
        }
    }
}

/// A game's platform under its title, unless it's a PC game, as the
/// webview's `platformLine`.
fn platform_line(platforms: &[String]) -> Option<String> {
    match platforms {
        [] => None,
        [only] if only == "PC" || only == "Windows" => None,
        [first] => Some(first.clone()),
        [first, rest @ ..] => Some(format!("{first} +{}", rest.len())),
    }
}
