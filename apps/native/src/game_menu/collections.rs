//! The manual collections a game is in, from its menu
//! (`components/collections/CollectionPicker.tsx`, #99): a checkbox per
//! collection, and a new one with the game in it. Smart collections pick
//! their own games, so they aren't listed.

use crate::data::{Query, change, use_library, use_query};
use crate::ui::{Button, ButtonVariant, Checkbox, Dialog, TextField, icons};
use caskade_core::model::{CollectionKind, CollectionSummary, SaveCollection};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;

fn use_collections() -> Query<Vec<CollectionSummary>> {
    use_query(DataKey::Collections, |library| async move {
        library.collections().await
    })
}

fn use_game_collections(game_id: String) -> Query<Vec<String>> {
    use_query(DataKey::Collections, move |library| {
        let game_id = game_id.clone();
        async move { library.game_collections(&game_id).await }
    })
}

#[component]
pub fn CollectionPicker(game_id: String, title: String, on_close: EventHandler<()>) -> Element {
    let library = use_library();
    let mut collections = use_collections();
    let mut of = use_game_collections(game_id.clone());
    let mut name = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let loading = collections.read().is_none() || of.read().is_none();
    let manual: Vec<CollectionSummary> = collections
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok())
        .map(|list| {
            list.iter()
                .filter(|collection| matches!(collection.kind, CollectionKind::Manual))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let inside: Vec<String> = of
        .read()
        .as_ref()
        .and_then(|ids| ids.as_ref().ok().cloned())
        .unwrap_or_default();
    let read_error = collections
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().err().map(|error| error.summary.clone()))
        .or_else(|| {
            of.read()
                .as_ref()
                .and_then(|ids| ids.as_ref().err().map(|error| error.summary.clone()))
        });

    let set_member = {
        let library = library.clone();
        let game_id = game_id.clone();
        use_callback(move |(id, member): (String, bool)| {
            let library = library.clone();
            let games = vec![game_id.clone()];
            busy.set(true);
            spawn(async move {
                let result = change(async move {
                    if member {
                        library.add_to_collection(&id, &games).await
                    } else {
                        library.remove_from_collection(&id, &games).await
                    }
                })
                .await;
                error.set(result.err().map(|error| error.summary));
                busy.set(false);
                of.restart();
                // Open collection pages and lists load again (#102).
                crate::collections::changed();
            });
        })
    };
    let create = {
        let library = library.clone();
        let game_id = game_id.clone();
        use_callback(move |()| {
            let trimmed = name.peek().trim().to_owned();
            if trimmed.is_empty() || *busy.peek() {
                return;
            }
            let library = library.clone();
            let games = vec![game_id.clone()];
            busy.set(true);
            spawn(async move {
                let result = change(async move {
                    let saved = library
                        .save_collection(&SaveCollection {
                            id: None,
                            name: trimmed,
                            description: String::new(),
                            kind: CollectionKind::Manual,
                        })
                        .await?;
                    library.add_to_collection(&saved.id, &games).await
                })
                .await;
                match result {
                    Ok(()) => name.set(String::new()),
                    Err(report) => error.set(Some(report.summary)),
                }
                busy.set(false);
                collections.restart();
                of.restart();
                crate::collections::changed();
            });
        })
    };
    let shown_error = error().or(read_error);
    rsx! {
        Dialog {
            title: t!("Collections for {title}", title = title),
            on_close: move |_| on_close.call(()),
            "data-cs": "collection-picker",
            div { class: "flex min-h-0 flex-col gap-4",
                if loading {
                    p { class: "text-sm text-fg-muted", {t!("Loading collections…")} }
                } else if manual.is_empty() {
                    p { class: "text-sm text-fg-muted", {t!("You have no collections to add games to yet.")} }
                } else {
                    ul { "data-part": "collections", class: "-mx-2 space-y-1 overflow-y-auto px-2",
                        for (index, collection) in manual.into_iter().enumerate() {
                            li { key: "{collection.id}",
                                Checkbox {
                                    checked: inside.contains(&collection.id),
                                    disabled: busy(),
                                    label: collection.name.clone(),
                                    "data-spatial": "focusable",
                                    "data-focus-default": (index == 0).then_some("true"),
                                    class: "rounded-control px-2 py-1.5 hover:bg-surface-hover",
                                    on_change: {
                                        let id = collection.id.clone();
                                        move |member: bool| set_member((id.clone(), member))
                                    },
                                }
                            }
                        }
                    }
                }
                div { class: "flex items-end gap-2",
                    TextField {
                        class: "flex-1",
                        label: t!("New collection"),
                        placeholder: tp!("collection", "Name"),
                        value: name(),
                        disabled: busy(),
                        on_input: move |value: String| name.set(value.chars().take(200).collect()),
                        on_submit: move |_| create(()),
                    }
                    Button {
                        icon: Some(icons::PLUS),
                        disabled: name().trim().is_empty() || busy(),
                        on_press: move |_| create(()),
                        {tp!("collection", "Create")}
                    }
                }
                p { class: "text-xs text-fg-subtle", {t!("Smart collections pick their own games.")} }
                if let Some(error) = shown_error {
                    p { role: "alert", class: "text-sm text-danger", "{error}" }
                }
                div { class: "flex justify-end",
                    Button { variant: ButtonVariant::Primary, on_press: move |_| on_close.call(()), {t!("Done")} }
                }
            }
        }
    }
}
