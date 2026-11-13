//! Search from a real keyboard, RetroArch style (`shell/TypeSearch.tsx`):
//! typing outside a text field, or `/`, opens a centred overlay that shows
//! only the text and how many games match. In the library the games filter
//! as it changes (within its filter and platform); anywhere else Enter
//! opens the library on the results. Escape puts the earlier search back,
//! and Backspace on an empty line closes it.

use crate::data::use_library_page;
use crate::input::{Bind, use_input, use_input_action};
use crate::router::{Route, href::Href, use_router};
use crate::ui::{Svg, icons};
use caskade_core::model::{GameFilter, GameSort, ListGames};
use caskade_ui_i18n::{t, tn, tp};
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

/// What the overlay holds while open: the text, and what was there before
/// it opened, for Escape.
#[derive(Clone, PartialEq)]
struct Typing {
    text: String,
    query_before: Option<String>,
    focus_before: Option<blitz_dom::NodeId>,
}

/// A page's own search, which typing goes to in place of the overlay
/// while the page fills it (Settings' search, #103): it gets the first
/// text typed.
#[derive(Clone, Copy)]
pub struct PageSearch(pub Signal<Option<Callback<String>>>);

/// Opens the overlay with some text: what the search's keyboard action
/// does.
#[derive(Clone, Copy)]
pub struct OpenTypeSearch(pub Callback<String>);

/// Opens the overlay from the input layer's typing (`Input::on_type`), and
/// shows it over `children`.
#[component]
pub fn TypeSearchRoot(children: Element) -> Element {
    let router = use_router();
    let input = use_input();
    let mut typing = use_signal(|| None::<Typing>);
    let reader = input.clone();
    let page = use_context_provider(|| PageSearch(Signal::new(None)));
    let open = use_callback(move |initial: String| {
        if typing.peek().is_some() {
            return;
        }
        // The page searches itself.
        if let Some(own) = *page.0.peek() {
            own.call(initial);
            return;
        }
        let query_before = match router.peek() {
            Route::Library(library) => library.q,
            _ => None,
        };
        let focus_before = reader.read(|doc| doc.get_focussed_node_id()).flatten();
        typing.set(Some(Typing {
            text: String::new(),
            query_before,
            focus_before,
        }));
        type_text(router, typing, initial);
    });
    use_context_provider(|| OpenTypeSearch(open));
    use_hook(move || input.on_type(move |initial| open.call(initial)));
    rsx! {
        {children}
        if typing().is_some() {
            Overlay { typing }
        }
    }
}

/// In the library, the text is its search, in place.
fn filter_library(router: crate::router::Router, q: Option<&str>) {
    let mut route = router.peek();
    if let Route::Library(library) = &mut route {
        library.q = q
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(str::to_owned);
        library.opening = false;
        router.replace(&route.href());
    }
}

fn type_text(router: crate::router::Router, mut typing: Signal<Option<Typing>>, text: String) {
    if let Some(state) = typing.write().as_mut() {
        state.text = text.clone();
    }
    if let Route::Library(library) = router.peek()
        && library.q.as_deref() != Some(text.trim()).filter(|q| !q.is_empty())
    {
        filter_library(router, Some(&text));
    }
}

#[component]
fn Overlay(typing: Signal<Option<Typing>>) -> Element {
    let router = use_router();
    let input = use_input();
    let mut field = use_signal(|| None::<NodeHandle>);
    let mut dialog = use_signal(|| None::<blitz_dom::NodeId>);
    let state = typing().unwrap_or(Typing {
        text: String::new(),
        query_before: None,
        focus_before: None,
    });
    let text = state.text.clone();
    let query = use_memo(move || {
        let text = typing()
            .map(|state| state.text.trim().to_owned())
            .unwrap_or_default();
        match router.route() {
            Route::Library(library) => ListGames {
                filter: library.filter,
                sort: library.sort,
                platform: library.platform.clone(),
                query: Some(text),
                limit: 1,
                ..ListGames::default()
            },
            _ => ListGames {
                filter: GameFilter::All,
                sort: GameSort::Title,
                query: Some(text),
                limit: 1,
                ..ListGames::default()
            },
        }
    });
    let count = use_library_page(query.into());
    let count = (!text.trim().is_empty())
        .then(|| match &*count.read() {
            Some(Ok(page)) => Some(page.total),
            _ => None,
        })
        .flatten();

    let refocus = use_callback(move |()| {
        if let Some(node) = typing.peek().as_ref().and_then(|state| state.focus_before) {
            input.focus(node);
        }
    });
    let close = use_callback(move |()| typing.set(None));
    let browse = use_callback(move |()| {
        let text = typing
            .peek()
            .as_ref()
            .map(|state| state.text.trim().to_owned())
            .unwrap_or_default();
        close(());
        if text.is_empty() {
            refocus(());
            return;
        }
        if !matches!(router.peek(), Route::Library(_)) {
            router.navigate(
                &Href::new("/library")
                    .text("filter", Some("all"))
                    .text("q", Some(&text))
                    .to_string(),
            );
        }
        // The library's first game takes focus once it's on screen; a pad
        // starts there anyway (`data-focus-default`).
    });
    let cancel = use_callback(move |()| {
        let before = typing
            .peek()
            .as_ref()
            .and_then(|state| state.query_before.clone());
        close(());
        filter_library(router, before.as_deref());
        refocus(());
    });
    use_input_action(
        Action::Back,
        Bind {
            owner: dialog(),
            label: Some(tp!("pad action", "Cancel")),
            ..Bind::default()
        },
        move || {
            cancel(());
            true
        },
    );
    use_input_action(
        Action::NavDown,
        Bind {
            owner: dialog(),
            label: Some(t!(
                note = "Hint bar: go down to the search results",
                "Results"
            )),
            ..Bind::default()
        },
        move || {
            browse(());
            true
        },
    );
    rsx! {
        div { class: "fixed inset-0 z-50 grid place-items-center bg-surface-base/40 p-6",
            div {
                role: "dialog",
                "aria-label": tp!("noun", "Search"),
                "data-cs": "type-search",
                class: "flex w-full max-w-3xl animate-enter flex-col gap-3 rounded-panel border border-accent-strong bg-surface-overlay px-8 py-6 shadow-xl shadow-black/40",
                onmounted: move |event: Event<MountedData>| {
                    dialog.set(event.data().downcast::<NodeHandle>().map(NodeHandle::node_id));
                },
                p {
                    "data-part": "label",
                    class: "flex items-center gap-2 font-mono text-xs tracking-widest text-accent-alt uppercase",
                    Svg { icon: icons::SEARCH, class: "size-4" }
                    {tp!("noun", "Search")}
                }
                div { class: "flex items-baseline gap-4",
                    input {
                        "data-part": "input",
                        "aria-label": t!("Search games"),
                        value: "{text}",
                        spellcheck: "false",
                        autocomplete: "off",
                        class: "min-w-0 flex-1 bg-transparent font-display text-4xl text-fg caret-accent-alt outline-none",
                        onmounted: move |event: Event<MountedData>| {
                            let handle = event.data().downcast::<NodeHandle>().cloned();
                            if let Some(handle) = &handle {
                                handle.doc_mut().set_focus_to(handle.node_id());
                                // Blitz puts a new field's caret before the
                                // text it starts with (its editor is made at
                                // the next layout); setting the value again
                                // then puts it last, after the character
                                // typed, as a browser's value does.
                                let handle = handle.clone();
                                spawn(async move {
                                    crate::data::sleep(std::time::Duration::from_millis(20)).await;
                                    let typed = typing.peek().as_ref().map(|state| state.text.clone()).unwrap_or_default();
                                    let value = blitz_dom::QualName::new(None, blitz_dom::ns!(), blitz_dom::LocalName::from("value"));
                                    let mut doc = handle.doc_mut();
                                    let mut mutator = doc.mutate();
                                    mutator.set_attribute(handle.node_id(), value.clone(), "");
                                    mutator.set_attribute(handle.node_id(), value, &typed);
                                });
                            }
                            field.set(handle);
                        },
                        oninput: move |event: FormEvent| type_text(router, typing, event.value()),
                        onkeydown: move |event: KeyboardEvent| {
                            if event.is_composing() {
                                return;
                            }
                            let empty = typing.peek().as_ref().is_some_and(|state| state.text.is_empty());
                            match event.key() {
                                Key::Named(NamedKey::Enter | NamedKey::ArrowDown) => browse(()),
                                Key::Named(NamedKey::Escape) => cancel(()),
                                Key::Named(NamedKey::Backspace) if empty => {
                                    close(());
                                    refocus(());
                                }
                                _ => return,
                            }
                            event.prevent_default();
                            event.stop_propagation();
                        },
                        // Focus left it: it closes, keeping the search.
                        onfocusout: move |_| close(()),
                    }
                    span {
                        "data-part": "count",
                        role: "status",
                        class: "shrink-0 font-mono text-sm text-fg-muted",
                        if let Some(count) = count {
                            {tn!("# game", "# games", count)}
                        }
                    }
                }
                p {
                    "data-part": "hints",
                    class: "font-mono text-xs tracking-wide text-fg-subtle uppercase",
                    {t!(
                        note = "Keyboard keys in the search overlay",
                        "Enter for results · Esc to cancel · Backspace to delete"
                    )}
                }
            }
        }
    }
}
