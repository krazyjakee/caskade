//! Console mode's file and folder picker, as the webview's `FolderBrowser`:
//! native dialogs can't be driven with a pad, and Steam Deck's game mode
//! has none. A pad moves through the entries, A opens a folder or picks a
//! file, B cancels, LB/RB go to the previous and next place, LT/RT page
//! through a long folder and Start picks the folder shown. Touch and the
//! mouse work the same way.

use blitz_dom::NodeId;
use caskade_core::browse::{DirEntry, DirListing, Place};
use caskade_ui_i18n::{t, tp};
use caskade_ui_input::Action;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use crate::data::change;
use crate::input::{Bind, use_input, use_input_action};
use crate::ui::icons::{ARROW_UP, FILE, FOLDER, HARD_DRIVE, HOUSE};
use crate::ui::{Button, ButtonVariant, Dialog, Svg};

use super::paths::{PathRequest, PickKind, format_path};

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("title", "What is being picked."),
        ("location", "The folder shown, as a path."),
        ("places", "Home, the filesystem root and mounted drives."),
        (
            "place",
            "One place; the one the folder is in has `data-status=\"active\"`.",
        ),
        ("error", "Why a folder couldn't be opened."),
        ("entries", "The folder's contents, scrolling."),
        ("up", "Goes up one folder."),
        (
            "entry",
            "A folder or file; `data-kind` is `folder` or `file`. Files that can't be picked carry `data-disabled`.",
        ),
        ("icon", "An entry's icon."),
        ("name", "An entry's name."),
        ("empty", "Shown in a folder with nothing in it."),
        ("actions", "Cancel, and in folder mode Select this folder."),
        ("select", "Picks the folder shown (a `button`)."),
    ],
    states: &[("data-kind", &["folder", "file"])],
    contains: &["button"],
    ..Entry::new(
        "folder-browser",
        "FolderBrowser (apps/native/src/flows/folder_browser.rs)",
        Source::App,
        "Console mode's file and folder picker, in place of the native dialogs a controller can't drive. A opens a folder or picks a file, B cancels, LB/RB go to the previous and next place, LT/RT page through the folder, Start picks the folder shown. `data-kind` is what it picks.",
    )
};

/// How many entries LT/RT move through.
const PAGE: usize = 8;

/// The place `path` is in: the one with the longest matching path.
pub fn current_place(places: &[Place], path: &str) -> Option<usize> {
    let mut best = None;
    let mut length = 0;
    for (index, place) in places.iter().enumerate() {
        let base = place.path.trim_end_matches(['/', '\\']);
        let inside = path == place.path
            || path == base
            || path.starts_with(&format!("{base}/"))
            || path.starts_with(&format!("{base}\\"));
        if inside && (best.is_none() || base.len() > length) {
            best = Some(index);
            length = base.len();
        }
    }
    best
}

fn pickable(entry: &DirEntry, request: &PathRequest) -> bool {
    if entry.is_dir {
        return true;
    }
    if request.kind == PickKind::Folder {
        return false;
    }
    if request.extensions.is_empty() {
        return true;
    }
    let name = entry.name.to_lowercase();
    request.extensions.iter().any(|extension| {
        name.ends_with(&format!(
            ".{}",
            extension.trim_start_matches('.').to_lowercase()
        ))
    })
}

/// Enter or Space on a row: Blitz gives buttons no key actions of their own.
fn activates(event: &KeyboardEvent) -> bool {
    use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
    match event.key() {
        Key::Named(NamedKey::Enter) => true,
        Key::Character(text) => text == " ",
        _ => false,
    }
}

const ROW: &str = "flex min-h-12 w-full items-center gap-3 rounded-control px-4 py-2 text-left text-base outline-none transition-colors duration-(--cs-motion-duration-fast) hover:bg-surface-hover data-focus-visible:bg-accent/10 data-focus-visible:focus-ring data-disabled:cursor-default data-disabled:opacity-40";

/// Where the browser is: the folder to list, and the one just left by
/// going up, to land on again.
#[derive(Clone, PartialEq)]
struct At {
    path: Option<String>,
    from: Option<String>,
}

/// The browser. `on_close` gets the path picked, or `None` when cancelled.
#[component]
pub fn FolderBrowser(request: PathRequest, on_close: EventHandler<Option<String>>) -> Element {
    let input = use_input();
    let mut at = use_signal(|| At {
        path: request.start_at.clone(),
        from: None,
    });
    let mut shown = use_signal(|| None::<(DirListing, Option<String>)>);
    let mut error = use_signal(|| None::<String>);
    let mut loaded = use_signal(|| false);
    let mut owner = use_signal(|| None::<NodeId>);
    // The entries' nodes, in order, for paging, and the one with focus.
    let mut nodes = use_signal(Vec::<(String, NodeId)>::new);
    let mut focused = use_signal(|| None::<String>);

    use_effect(move || {
        let wanted = at();
        spawn(async move {
            let path = wanted.path.clone();
            let listing =
                change(async move { caskade_app::commands::browse::browse_dir(path, false).await })
                    .await;
            match listing {
                Ok(listing) => {
                    loaded.set(true);
                    nodes.write().clear();
                    shown.set(Some((listing, wanted.from.clone())));
                    error.set(None);
                }
                Err(report) => {
                    error.set(Some(report.summary));
                    // A start folder that has gone: start at home instead.
                    if wanted.path.is_some() && !*loaded.peek() {
                        at.set(At {
                            path: None,
                            from: None,
                        });
                    }
                }
            }
        });
    });

    let mut open = move |path: String, from: Option<String>| {
        at.set(At {
            path: Some(path),
            from,
        })
    };
    let listing = shown().map(|(listing, _)| listing);
    let from = shown().and_then(|(_, from)| from);
    let places = listing
        .as_ref()
        .map(|listing| listing.places.clone())
        .unwrap_or_default();
    let place_index = listing
        .as_ref()
        .and_then(|listing| current_place(&places, &listing.path));
    let folder_mode = request.kind == PickKind::Folder;
    let entries: Vec<(DirEntry, bool)> = listing
        .as_ref()
        .map(|listing| {
            listing
                .entries
                .iter()
                .map(|entry| (entry.clone(), pickable(entry, &request)))
                .collect()
        })
        .unwrap_or_default();
    // Where focus lands when a folder opens: the folder just left, going
    // up; else the first entry that can be picked; else Up.
    let landing = from
        .clone()
        .filter(|from| entries.iter().any(|(entry, _)| &entry.path == from))
        .or_else(|| {
            entries
                .iter()
                .find(|(_, enabled)| *enabled)
                .map(|(entry, _)| entry.path.clone())
        })
        .or_else(|| {
            listing
                .as_ref()
                .and_then(|listing| listing.parent.as_ref())
                .map(|_| "up".to_owned())
        });

    let step = {
        let places = places.clone();
        move |by: isize| {
            if places.is_empty() {
                return false;
            }
            let count = places.len() as isize;
            let index = match place_index {
                None if by > 0 => 0,
                None => count - 1,
                Some(index) => (index as isize + by + count) % count,
            };
            open(places[index as usize].path.clone(), None);
            true
        }
    };
    let paging = input.clone();
    let page = move |sign: isize| {
        let list = nodes.peek();
        if list.is_empty() {
            return true;
        }
        let current = focused
            .peek()
            .as_ref()
            .and_then(|path| list.iter().position(|(known, _)| known == path))
            .unwrap_or(0) as isize;
        let target = (current + sign * PAGE as isize).clamp(0, list.len() as isize - 1);
        paging.focus(list[target as usize].1);
        true
    };
    let bind = |label: String| Bind {
        label: Some(label),
        owner: owner(),
        ..Bind::default()
    };
    let mut previous = step.clone();
    let mut next = step.clone();
    let page_up = page.clone();
    let page_down = page;
    use_input_action(Action::TabPrev, bind(t!("Previous place")), move || {
        previous(-1)
    });
    use_input_action(Action::TabNext, bind(t!("Next place")), move || next(1));
    use_input_action(Action::PagePrev, bind(t!("Page up")), move || page_up(-1));
    use_input_action(Action::PageNext, bind(t!("Page down")), move || {
        page_down(1)
    });
    let shown_path = listing.as_ref().map(|listing| listing.path.clone());
    use_input_action(Action::Menu, bind(t!("Select folder")), move || {
        match (&shown_path, folder_mode) {
            (Some(path), true) => {
                on_close.call(Some(path.clone()));
                true
            }
            _ => false,
        }
    });

    let mut land = move |path: String, node: NodeId| {
        nodes.write().push((path.clone(), node));
    };
    let focus_landing = {
        let landing = landing.clone();
        move |key: &str, event: Event<MountedData>| {
            if let Some(handle) = event.data().downcast::<NodeHandle>() {
                if key != "up" {
                    land(key.to_owned(), handle.node_id());
                }
                if landing.as_deref() == Some(key) {
                    input.focus(handle.node_id());
                }
            }
        }
    };

    rsx! {
        Dialog {
            title: request.title.clone(),
            on_close: move |_| on_close.call(None),
            class: "flex h-[85vh] max-w-3xl flex-col",
            "data-cs": "folder-browser",
            "data-kind": if folder_mode { "folder" } else { "file" },
            div {
                class: "flex min-h-0 flex-1 flex-col gap-4",
                onmounted: move |event: Event<MountedData>| {
                    if let Some(handle) = event.data().downcast::<NodeHandle>() {
                        owner.set(Some(handle.node_id()));
                    }
                },
                p { "data-part": "location", class: "break-all font-mono text-base text-fg-muted",
                    match &listing {
                        Some(listing) => format_path(&listing.path),
                        None => "…".to_owned(),
                    }
                }
                if !places.is_empty() {
                    nav {
                        "data-part": "places",
                        "aria-label": tp!("folder browser", "Places"),
                        class: "flex flex-wrap gap-2",
                        for (index, place) in places.iter().cloned().enumerate() {
                            button {
                                key: "{place.path}",
                                r#type: "button",
                                "data-part": "place",
                                "data-status": (place_index == Some(index)).then_some("active"),
                                "aria-current": (place_index == Some(index)).then_some("true"),
                                class: "flex min-h-11 items-center gap-2 rounded-control border border-border px-4 text-base outline-none hover:bg-surface-hover data-focus-visible:focus-ring aria-[current]:border-accent-alt aria-[current]:bg-accent/10",
                                onclick: {
                                    let path = place.path.clone();
                                    move |_| open(path.clone(), None)
                                },
                                onkeydown: {
                                    let path = place.path.clone();
                                    move |event: KeyboardEvent| {
                                        if activates(&event) {
                                            event.prevent_default();
                                            open(path.clone(), None);
                                        }
                                    }
                                },
                                Svg { icon: if index == 0 { HOUSE } else { HARD_DRIVE }, class: "size-5 text-fg-muted" }
                                "{place.label}"
                            }
                        }
                    }
                }
                if let Some(failure) = error() {
                    p { "data-part": "error", role: "alert", class: "text-base text-danger", "{failure}" }
                }
                section {
                    "data-part": "entries",
                    "aria-label": t!("Folder contents"),
                    class: "min-h-0 flex-1 overflow-y-auto rounded-control border border-border bg-surface-base/40 p-1",
                    if let Some(parent) = listing.as_ref().and_then(|listing| listing.parent.clone()) {
                        button {
                            r#type: "button",
                            "data-part": "up",
                            "data-focus-default": (landing.as_deref() == Some("up")).then_some("true"),
                            class: ROW,
                            onmounted: {
                                let mut focus_landing = focus_landing.clone();
                                move |event| focus_landing("up", event)
                            },
                            onclick: {
                                let here = listing.as_ref().map(|listing| listing.path.clone());
                                let parent = parent.clone();
                                move |_| open(parent.clone(), here.clone())
                            },
                            onkeydown: {
                                let here = listing.as_ref().map(|listing| listing.path.clone());
                                move |event: KeyboardEvent| {
                                    if activates(&event) {
                                        event.prevent_default();
                                        open(parent.clone(), here.clone());
                                    }
                                }
                            },
                            Svg { icon: ARROW_UP, class: "size-6 shrink-0 text-fg-muted" }
                            span { class: "font-medium", {t!("Up one folder")} }
                        }
                    }
                    for (entry, enabled) in entries.iter().cloned() {
                        button {
                            key: "{entry.path}",
                            r#type: "button",
                            "data-part": "entry",
                            "data-focus-default": (landing.as_deref() == Some(entry.path.as_str())).then_some("true"),
                            "data-kind": if entry.is_dir { "folder" } else { "file" },
                            "data-disabled": (!enabled).then_some("true"),
                            disabled: !enabled,
                            class: ROW,
                            onmounted: {
                                let mut focus_landing = focus_landing.clone();
                                let path = entry.path.clone();
                                move |event| focus_landing(&path, event)
                            },
                            onfocus: {
                                let path = entry.path.clone();
                                move |_| focused.set(Some(path.clone()))
                            },
                            onclick: {
                                let entry = entry.clone();
                                move |_| {
                                    if entry.is_dir {
                                        open(entry.path.clone(), None);
                                    } else if enabled {
                                        on_close.call(Some(entry.path.clone()));
                                    }
                                }
                            },
                            onkeydown: {
                                let entry = entry.clone();
                                move |event: KeyboardEvent| {
                                    if activates(&event) {
                                        event.prevent_default();
                                        if entry.is_dir {
                                            open(entry.path.clone(), None);
                                        } else if enabled {
                                            on_close.call(Some(entry.path.clone()));
                                        }
                                    }
                                }
                            },
                            Svg {
                                icon: if entry.is_dir { FOLDER } else { FILE },
                                part: "icon",
                                class: if entry.is_dir { "size-6 shrink-0 text-accent-alt" } else { "size-6 shrink-0 text-fg-muted" },
                            }
                            span { "data-part": "name", class: "min-w-0 truncate", "{entry.name}" }
                        }
                    }
                    if listing.is_some() && entries.is_empty() {
                        p { "data-part": "empty", class: "px-4 py-3 text-base text-fg-muted",
                            {t!("This folder is empty.")}
                        }
                    }
                }
                div { "data-part": "actions", class: "flex justify-end gap-3",
                    Button { id: "folder-cancel", on_press: move |_| on_close.call(None), {t!("Cancel")} }
                    if folder_mode {
                        Button {
                            id: "folder-select",
                            variant: ButtonVariant::Primary,
                            "data-part": "select",
                            disabled: listing.is_none(),
                            on_press: {
                                let path = listing.as_ref().map(|listing| listing.path.clone());
                                move |_| {
                                    if let Some(path) = &path {
                                        on_close.call(Some(path.clone()));
                                    }
                                }
                            },
                            {t!("Select this folder")}
                        }
                    }
                }
            }
        }
    }
}
