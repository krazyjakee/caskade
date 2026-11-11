//! Makes a collection, or edits one's name, description and rules (#99, the
//! webview's `components/collections/CollectionDialog.tsx`). A new one may
//! start from a preset: the library's view, saved as a smart collection.

use super::platforms;
use crate::data::{change, use_library, use_query};
use crate::ui::{
    Button, ButtonVariant, Dialog, Segment, SegmentedControl, Select, SelectOption, TextField,
    icons,
};
use caskade_core::model::{
    CollectionKind, CollectionSummary, GameFilter, GameSort, ListGames, SaveCollection,
};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{t, tn, tp};
use dioxus_native::prelude::*;

/// "Any platform"'s option.
const ANY_PLATFORM: &str = "any";

/// The library filters, by id, with their names (`rules.ts`'s
/// `filterMessages`).
pub fn filters() -> [(GameFilter, &'static str, String); 7] {
    [
        (GameFilter::All, "all", tp!("library filter", "All games")),
        (
            GameFilter::Installed,
            "installed",
            tp!("library filter", "Installed"),
        ),
        (
            GameFilter::Owned,
            "owned",
            tp!(
                note = "Games a connected store account owns that aren't installed",
                "library filter",
                "Ready to Install"
            ),
        ),
        (
            GameFilter::Recent,
            "recent",
            tp!(
                note = "Games that have been played",
                "library filter",
                "Played"
            ),
        ),
        (
            GameFilter::Emulated,
            "emulated",
            tp!("library filter", "Emulated"),
        ),
        (
            GameFilter::Favorites,
            "favorites",
            tp!("library filter", "Favorites"),
        ),
        (
            GameFilter::Web,
            "web",
            tp!(
                note = "Games played in the browser",
                "library filter",
                "Web"
            ),
        ),
    ]
}

/// The library sorts, by id, with their names (`rules.ts`'s
/// `sortMessages`).
pub fn sorts() -> [(GameSort, &'static str, String); 6] {
    [
        (GameSort::Title, "title", tp!("library sort", "Title")),
        (
            GameSort::Recent,
            "recent",
            tp!("library sort", "Recently played"),
        ),
        (
            GameSort::Playtime,
            "playtime",
            tp!("library sort", "Playtime"),
        ),
        (
            GameSort::Added,
            "added",
            tp!("library sort", "Recently added"),
        ),
        (GameSort::Popular, "popular", tp!("library sort", "Popular")),
        (
            GameSort::Released,
            "released",
            tp!("library sort", "Release date"),
        ),
    ]
}

/// The rules a smart collection starts with: the view's, without paging.
pub fn smart_preset(query: &ListGames) -> ListGames {
    ListGames {
        filter: query.filter,
        sort: query.sort,
        query: query.query.clone(),
        platform: query.platform.clone(),
        ..ListGames::default()
    }
}

#[component]
pub fn CollectionDialog(
    /// The collection to edit; a new one without.
    #[props(default)]
    collection: Option<CollectionSummary>,
    /// What a new one starts as.
    #[props(default)]
    preset: Option<CollectionKind>,
    on_close: EventHandler<()>,
    /// Saved: the collection's id.
    #[props(default)]
    on_saved: Option<EventHandler<String>>,
) -> Element {
    let library = use_library();
    let start = collection
        .as_ref()
        .map(|collection| collection.kind.clone())
        .or(preset)
        .unwrap_or(CollectionKind::Manual);
    let editing = collection.is_some();
    let mut name = use_signal(|| {
        collection
            .as_ref()
            .map(|collection| collection.name.clone())
            .unwrap_or_default()
    });
    let mut description = use_signal(|| {
        collection
            .as_ref()
            .map(|collection| collection.description.clone())
            .unwrap_or_default()
    });
    let mut smart = use_signal(|| matches!(start, CollectionKind::Smart { .. }));
    let mut query = use_signal(|| match &start {
        CollectionKind::Smart { query } => smart_preset(query),
        CollectionKind::Manual => ListGames {
            filter: GameFilter::Installed,
            sort: GameSort::Title,
            ..ListGames::default()
        },
    });
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // The library's platforms and how many games match, for a smart one.
    let platforms_query = use_query(DataKey::Summary, move |data| {
        let wanted = smart();
        async move {
            if wanted {
                data.platforms(GameFilter::All).await
            } else {
                Ok(Vec::new())
            }
        }
    });
    let matches = use_query(DataKey::Games, move |data| {
        let wanted = smart();
        let rules = ListGames {
            limit: 1,
            ..query()
        };
        async move {
            if wanted {
                data.page(&rules).await.map(|page| Some(page.total))
            } else {
                Ok(None)
            }
        }
    });
    let current = query();
    let mut platform_options = vec![SelectOption::new(ANY_PLATFORM, t!("Any platform"))];
    if let Some(Ok(list)) = &*platforms_query.read() {
        platform_options.extend(list.iter().map(|platform| {
            SelectOption::new(
                platform.name.clone(),
                platforms::present(&platform.name).title,
            )
        }));
    }
    // A saved platform the library no longer has stays choosable.
    if let Some(platform) = &current.platform
        && !platform_options
            .iter()
            .any(|option| &option.value == platform)
    {
        platform_options.push(SelectOption::new(
            platform.clone(),
            platforms::present(platform).title,
        ));
    }
    let matching = match &*matches.read() {
        Some(Ok(Some(total))) => Some(*total),
        _ => None,
    };

    let id = collection.as_ref().map(|collection| collection.id.clone());
    let save = move || {
        let trimmed = name.peek().trim().to_owned();
        if trimmed.is_empty() || *busy.peek() {
            return;
        }
        let library = library.clone();
        let saving = SaveCollection {
            id: id.clone(),
            name: trimmed,
            description: description.peek().trim().to_owned(),
            kind: if *smart.peek() {
                CollectionKind::Smart {
                    query: query.peek().clone(),
                }
            } else {
                CollectionKind::Manual
            },
        };
        busy.set(true);
        error.set(None);
        spawn(async move {
            match change(async move { library.save_collection(&saving).await }).await {
                Ok(saved) => {
                    if let Some(on_saved) = on_saved {
                        on_saved.call(saved.id);
                    }
                    on_close.call(());
                }
                Err(report) => error.set(Some(report.summary)),
            }
            busy.set(false);
        });
    };
    let mut submit = save.clone();
    let title = match &collection {
        Some(collection) => t!("Edit {name}", name = collection.name.clone()),
        None => t!("New collection"),
    };
    let filter_options: Vec<SelectOption> = filters()
        .into_iter()
        .map(|(_, id, label)| SelectOption::new(id, label))
        .collect();
    let sort_options: Vec<SelectOption> = sorts()
        .into_iter()
        .map(|(_, id, label)| SelectOption::new(id, label))
        .collect();
    let filter_id = filters()
        .into_iter()
        .find(|(filter, ..)| *filter == current.filter)
        .map(|(_, id, _)| id.to_owned());
    let sort_id = sorts()
        .into_iter()
        .find(|(sort, ..)| *sort == current.sort)
        .map(|(_, id, _)| id.to_owned());
    rsx! {
        Dialog {
            title,
            on_close: move |_| {
                if !busy() {
                    on_close.call(());
                }
            },
            "data-cs": "collection-dialog",
            class: "max-w-lg!",
            div { class: "mt-4 flex flex-col gap-4",
                if !editing {
                    SegmentedControl {
                        "aria-label": t!("Kind of collection"),
                        value: if smart() { "smart" } else { "manual" },
                        on_change: move |id: String| smart.set(id == "smart"),
                        options: vec![
                            Segment {
                                icon: Some(icons::LIST_CHECKS),
                                ..Segment::new("manual", tp!("collection kind", "Manual"))
                            },
                            Segment {
                                icon: Some(icons::SPARKLES),
                                ..Segment::new("smart", tp!("collection kind", "Smart"))
                            },
                        ],
                    }
                }
                TextField {
                    label: tp!("collection", "Name"),
                    "data-focus-default": "true",
                    value: name(),
                    disabled: busy(),
                    on_input: move |value: String| name.set(value.chars().take(200).collect()),
                    on_submit: move |_| submit(),
                }
                TextField {
                    label: tp!("collection", "Description"),
                    value: description(),
                    disabled: busy(),
                    on_input: move |value: String| description.set(value.chars().take(500).collect()),
                }
                if smart() {
                    fieldset { "data-part": "rules", class: "flex flex-col gap-3",
                        legend { class: "mb-2 text-sm text-fg-muted",
                            {t!("Games that match every rule, as the library changes")}
                        }
                        div { class: "grid grid-cols-2 gap-3",
                            Select {
                                label: tp!("smart collection rule", "Games"),
                                options: filter_options,
                                value: filter_id,
                                on_change: move |id: String| {
                                    if let Some((filter, ..)) = filters().into_iter().find(|(_, key, _)| *key == id) {
                                        query.write().filter = filter;
                                    }
                                },
                            }
                            Select {
                                label: tp!("smart collection rule", "Platform"),
                                options: platform_options,
                                value: Some(current.platform.clone().unwrap_or_else(|| ANY_PLATFORM.to_owned())),
                                on_change: move |id: String| {
                                    query.write().platform = (id != ANY_PLATFORM).then_some(id);
                                },
                            }
                        }
                        TextField {
                            label: tp!("smart collection rule", "Title has"),
                            placeholder: t!("Any title"),
                            value: current.query.clone().unwrap_or_default(),
                            on_input: move |text: String| {
                                let text: String = text.chars().take(200).collect();
                                query.write().query = (!text.is_empty()).then_some(text);
                            },
                        }
                        Select {
                            label: t!("Sort by"),
                            options: sort_options,
                            value: sort_id,
                            on_change: move |id: String| {
                                if let Some((sort, ..)) = sorts().into_iter().find(|(_, key, _)| *key == id) {
                                    query.write().sort = sort;
                                }
                            },
                        }
                        p { "data-part": "matches", role: "status", class: "text-sm text-fg-muted",
                            match matching {
                                None => t!("Counting games…"),
                                Some(total) => tn!("# game matches now", "# games match now", total as usize),
                            }
                        }
                    }
                } else if !editing {
                    p { class: "text-sm text-fg-muted",
                        {t!("Add games from their menu, or from the collection's Add games.")}
                    }
                }
                if let Some(error) = error() {
                    p { role: "alert", class: "text-sm text-danger", "{error}" }
                }
                div { class: "flex justify-end gap-2",
                    Button { disabled: busy(), on_press: move |_| on_close.call(()), {t!("Cancel")} }
                    Button {
                        variant: ButtonVariant::Primary,
                        "data-part": "save",
                        disabled: name().trim().is_empty() || busy(),
                        on_press: {
                            let mut save = save.clone();
                            move |_| save()
                        },
                        if busy() {
                            {t!("Saving…")}
                        } else if editing {
                            {t!("Save")}
                        } else {
                            {tp!("collection", "Create")}
                        }
                    }
                }
            }
        }
    }
}
