//! Picks a manual collection's games from the library (the webview's
//! `components/collections/AddGamesDialog.tsx`): a search, and a checkbox
//! per game that adds it or takes it out at once.

use crate::data::{change, use_library, use_query};
use crate::ui::{Button, ButtonVariant, Checkbox, Dialog, TextField, icons};
use caskade_core::model::{CollectionSummary, GameFilter, GameSort, ListGames};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{format, t};
use dioxus_native::prelude::dioxus_core::spawn_forever;
use dioxus_native::prelude::*;

/// The games the dialog lists; a search finds the rest.
const SHOWN: u32 = 50;

#[component]
pub fn AddGamesDialog(collection: CollectionSummary, on_close: EventHandler<()>) -> Element {
    let library = use_library();
    let mut search = use_signal(String::new);
    let games = use_query(DataKey::Games, move |library| {
        let text = search().trim().to_owned();
        let query = ListGames {
            filter: GameFilter::All,
            sort: GameSort::Title,
            query: (!text.is_empty()).then_some(text),
            limit: SHOWN,
            ..ListGames::default()
        };
        async move { library.page(&query).await }
    });
    let members = {
        let id = collection.id.clone();
        use_query(DataKey::Collections, move |library| {
            let _ = super::revision();
            let id = id.clone();
            async move {
                library
                    .collection(&id)
                    .await
                    .map(|found| found.map(|found| found.game_ids).unwrap_or_default())
            }
        })
    };
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let set_member = {
        let id = collection.id.clone();
        use_callback(move |(game_id, member): (String, bool)| {
            let library = library.clone();
            let id = id.clone();
            let games = vec![game_id];
            busy.set(true);
            // Beyond the dialog: Done may close it before the change lands,
            // and the collection's page behind still loads again.
            spawn_forever(async move {
                let result = change(async move {
                    if member {
                        library.add_to_collection(&id, &games).await
                    } else {
                        library.remove_from_collection(&id, &games).await
                    }
                })
                .await;
                if let Ok(mut error) = error.try_write() {
                    *error = result.err().map(|report| report.summary);
                }
                if let Ok(mut busy) = busy.try_write() {
                    *busy = false;
                }
                super::changed();
            });
        })
    };

    let loading = games.read().is_none() || members.read().is_none();
    let read_error = games
        .read()
        .as_ref()
        .and_then(|page| page.as_ref().err().map(|report| report.summary.clone()))
        .or_else(|| {
            members
                .read()
                .as_ref()
                .and_then(|ids| ids.as_ref().err().map(|report| report.summary.clone()))
        });
    let (items, total) = match &*games.read() {
        Some(Ok(page)) => (page.items.clone(), page.total),
        _ => (Vec::new(), 0),
    };
    let inside: Vec<String> = members
        .read()
        .as_ref()
        .and_then(|ids| ids.as_ref().ok().cloned())
        .unwrap_or_default();
    let searching = !search().trim().is_empty();
    let shown = items.len();
    rsx! {
        Dialog {
            title: t!("Add games to {name}", name = collection.name.clone()),
            on_close: move |_| on_close.call(()),
            "data-cs": "add-games",
            class: "flex max-h-[85vh] max-w-lg! flex-col",
            TextField {
                value: search(),
                on_input: move |text: String| search.set(text),
                placeholder: t!("Search the library"),
                icon: icons::SEARCH,
                "aria-label": t!("Search the library"),
                "data-focus-default": "true",
            }
            div { "data-part": "games", class: "-mx-2 min-h-40 overflow-y-auto px-2",
                if loading {
                    p { class: "py-6 text-center text-sm text-fg-muted", {t!("Loading games…")} }
                } else if let Some(message) = read_error {
                    p { role: "alert", class: "text-sm text-danger", "{message}" }
                } else if !items.is_empty() {
                    ul { class: "space-y-1",
                        for game in items {
                            li { key: "{game.id}",
                                Checkbox {
                                    checked: inside.contains(&game.id),
                                    disabled: busy(),
                                    label: game.title.clone(),
                                    "data-spatial": "focusable",
                                    class: "rounded-control px-2 py-1.5 data-hovered:bg-surface-hover",
                                    on_change: {
                                        let id = game.id.clone();
                                        move |member: bool| set_member((id.clone(), member))
                                    },
                                }
                            }
                        }
                    }
                } else if searching {
                    p { class: "py-6 text-center text-sm text-fg-muted", {t!("No games match your search.")} }
                } else {
                    p { class: "py-6 text-center text-sm text-fg-muted", {t!("Your library has no games yet.")} }
                }
            }
            if total > shown as u64 {
                p { class: "text-xs text-fg-subtle",
                    {t!(
                        "Showing {shown} of {total}. Search to find the rest.",
                        shown = shown,
                        total = format::count(total as f64)
                    )}
                }
            }
            if let Some(message) = error() {
                p { role: "alert", class: "text-sm text-danger", "{message}" }
            }
            div { class: "flex justify-end",
                Button { variant: ButtonVariant::Primary, on_press: move |_| on_close.call(()), {t!("Done")} }
            }
        }
    }
}
