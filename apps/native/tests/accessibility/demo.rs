//! The demo elements of the accessibility test (#83), built as Caskade's
//! components build theirs: the test reads their AccessKit tree, and
//! `examples/accessibility_demo.rs` opens them in a window for screen
//! readers (`scripts/a11y-atspi.py`).

use dioxus_native::prelude::keyboard_types::NamedKey;
use dioxus_native::prelude::*;
use std::sync::Mutex;

/// What the demo's handlers did.
pub static DONE: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn did(what: impl Into<String>) {
    DONE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(what.into());
}

pub fn take_done() -> Vec<String> {
    std::mem::take(&mut *DONE.lock().unwrap_or_else(|e| e.into_inner()))
}

#[component]
pub fn Demo() -> Element {
    let mut favourite = use_signal(|| false);
    let mut volume = use_signal(|| 40);
    let mut progress = use_signal(|| 25);
    let mut sort_open = use_signal(|| false);
    rsx! {
        title { "Caskade accessibility demo" }
        nav { "aria-label": "Library",
            a { href: "#all", "aria-current": "page", "All games" }
            a { href: "#installed", "Installed" }
        }
        main {
            h1 { "Hollow Knight" }
            button {
                id: "favourite",
                "aria-pressed": "{favourite}",
                onclick: move |_| {
                    favourite.toggle();
                    did("favourite");
                },
                "Favourite"
            }
            button {
                id: "sort",
                "aria-haspopup": "listbox",
                "aria-expanded": "{sort_open}",
                "aria-controls": "sort-options",
                onclick: move |_| sort_open.toggle(),
                "Sort by name"
            }
            if sort_open() {
                div {
                    id: "sort-options",
                    role: "listbox",
                    "aria-label": "Sort",
                    "aria-activedescendant": "sort-recent",
                    div { id: "sort-name", role: "option", "aria-selected": "true",
                        "aria-posinset": "1", "aria-setsize": "2", "Name" }
                    div { id: "sort-recent", role: "option", "aria-selected": "false",
                        "aria-posinset": "2", "aria-setsize": "2", "Recently played" }
                }
            }
            label {
                input { r#type: "checkbox", checked: true }
                "Show hidden games"
            }
            div {
                id: "volume",
                role: "slider",
                tabindex: "0",
                "aria-label": "Volume",
                "aria-valuenow": "{volume}",
                "aria-valuemin": "0",
                "aria-valuemax": "100",
                "aria-valuetext": "{volume}%",
                onkeydown: move |event: KeyboardEvent| match event.key() {
                    Key::Named(NamedKey::ArrowUp) => volume += 10,
                    Key::Named(NamedKey::ArrowDown) => volume -= 10,
                    _ => {}
                },
            }
            input { "aria-label": "Search", placeholder: "Search your library", required: true,
                "aria-invalid": "true" }
            div { role: "tablist", "aria-orientation": "horizontal",
                button { role: "tab", "aria-selected": "true", "Overview" }
                button { role: "tab", "aria-selected": "false", "Achievements" }
            }
            div {
                id: "confirm",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Uninstall Hollow Knight?",
                button { onclick: move |_| progress += 25, "Uninstall" }
            }
        }
        footer {
            div { role: "status", "aria-busy": "{progress() < 100}",
                if progress() < 100 { "Installing Celeste" } else { "Celeste is installed" }
                div { role: "progressbar", "aria-label": "Celeste",
                    "aria-valuenow": "{progress}", "aria-valuemin": "0", "aria-valuemax": "100" }
            }
            div { role: "alert", "Couldn't reach RomM" }
        }
    }
}
