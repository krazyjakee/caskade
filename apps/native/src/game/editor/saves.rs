//! The editor's Saves tab (the webview's `gameEditor/SavesTab.tsx`): one
//! release's save locations on this device, automatic or the user's own,
//! and the manifest game it matches.

use caskade_core::model::GameDetails;
use caskade_core::saves::{LocationKind, LocationSpec, SaveMatch, SavePreferences};
use caskade_ui_data::Key;
use caskade_ui_i18n::{t, tn};
use dioxus_native::prelude::*;

use super::data::{EditData, SaveLocations};
use super::{FILE_PLUS, release_label};
use crate::flows::paths::{PathRequest, PickKind, use_path_picker};
use crate::game::data::use_answer;
use crate::ui::icons::{FOLDER_PLUS, ROTATE_CCW, TRASH};
use crate::ui::{Button, ButtonVariant, Select, SelectOption, TextField};

fn kind_id(kind: LocationKind) -> &'static str {
    match kind {
        LocationKind::File => "file",
        LocationKind::Folder => "folder",
        LocationKind::Registry => "registry",
    }
}

fn kind_label(kind: LocationKind) -> String {
    match kind {
        LocationKind::Folder => t!("Folder"),
        LocationKind::File => t!("File"),
        LocationKind::Registry => t!("Registry key"),
    }
}

fn match_id(matched: &SaveMatch) -> &'static str {
    match matched {
        SaveMatch::Unknown => "unknown",
        SaveMatch::Matched { .. } => "matched",
        SaveMatch::Ambiguous { .. } => "ambiguous",
    }
}

fn match_text(matched: &SaveMatch) -> String {
    match matched {
        SaveMatch::Unknown => t!(
            "No automatic save locations are known for this release. Add its save folders or files below, or set the manifest game it matches."
        ),
        SaveMatch::Ambiguous { titles } => t!(
            "More than one manifest game matches: {titles}. Enter the right one's name below.",
            titles = titles.join(", ")
        ),
        SaveMatch::Matched { title, .. } => {
            t!("Save locations from {title}.", title = title.clone())
        }
    }
}

#[component]
pub fn SavesTab(
    details: GameDetails,
    data: EditData,
    on_dirty_change: EventHandler<bool>,
) -> Element {
    let mut selected = use_signal(|| None::<String>);
    let entry = details
        .entries
        .iter()
        .find(|entry| Some(&entry.id) == selected.read().as_ref())
        .or_else(|| {
            details
                .entries
                .iter()
                .find(|entry| Some(&entry.id) == details.game.default_entry.as_ref())
        })
        .or_else(|| details.entries.first())
        .cloned();
    let Some(entry) = entry else {
        return rsx! {
            p { class: "text-sm text-fg-muted", {t!("This game has no releases.")} }
        };
    };
    let several = details.entries.len() > 1;
    let options: Vec<SelectOption> = details
        .entries
        .iter()
        .map(|entry| SelectOption::new(entry.id.clone(), release_label(entry)))
        .collect();
    rsx! {
        // One release's locations at a time: another starts afresh.
        for entry_id in [entry.id.clone()] {
            Locations {
                key: "{entry_id}",
                entry_id: entry_id.clone(),
                game_id: details.game.id.clone(),
                data: data.clone(),
                options: several.then(|| options.clone()),
                on_release: move |id: String| selected.set(Some(id)),
                on_dirty_change,
            }
        }
    }
}

#[component]
fn Locations(
    entry_id: String,
    game_id: String,
    data: EditData,
    /// The releases to choose between, when there are several.
    options: Option<Vec<SelectOption>>,
    on_release: EventHandler<String>,
    on_dirty_change: EventHandler<bool>,
) -> Element {
    let query = {
        let (data, entry_id) = (data.clone(), entry_id.clone());
        use_answer(Key::Game(game_id.clone()), move || {
            (data.saves)(entry_id.clone())
        })
    };
    let picker = use_path_picker();
    // The revision the edit started from, so a change made elsewhere is
    // refused.
    let mut draft = use_signal(|| None::<(SavePreferences, u32)>);
    let mut saved = use_signal(|| false);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let loaded = query.read().clone();
    let state: Option<SaveLocations> = match &loaded {
        Some(Ok(state)) => Some(state.clone()),
        _ => None,
    };
    let preferences = draft
        .read()
        .as_ref()
        .map(|(preferences, _)| preferences.clone())
        .or_else(|| state.as_ref().map(|state| state.preferences.clone()));
    let dirty = match (&*draft.read(), &state) {
        (Some((preferences, _)), Some(state)) => *preferences != state.preferences,
        (Some(_), None) => true,
        _ => false,
    };
    use_effect(use_reactive!(|dirty| on_dirty_change.call(dirty)));
    let specs: Vec<LocationSpec> = preferences
        .as_ref()
        .and_then(|preferences| preferences.locations.clone())
        .or_else(|| {
            state
                .as_ref()
                .map(|state| state.automatic.iter().map(|row| row.spec.clone()).collect())
        })
        .unwrap_or_default();
    let busy = pending();

    let change = {
        let state = state.clone();
        move |next: SavePreferences| {
            let Some(state) = &state else { return };
            let revision = draft
                .peek()
                .as_ref()
                .map(|(_, revision)| *revision)
                .unwrap_or(state.revision);
            draft.set(Some((next, revision)));
            saved.set(false);
            error.set(None);
        }
    };
    let add = {
        let (preferences, specs, change) = (preferences.clone(), specs.clone(), change.clone());
        move |kind: PickKind| {
            let Some(preferences) = preferences.clone() else {
                return;
            };
            let (specs, mut change) = (specs.clone(), change.clone());
            let title = match kind {
                PickKind::Folder => t!("Choose a save folder"),
                PickKind::File => t!("Choose a save file"),
            };
            let location_kind = match kind {
                PickKind::Folder => LocationKind::Folder,
                PickKind::File => LocationKind::File,
            };
            error.set(None);
            spawn(async move {
                let Some(path) = picker.pick(PathRequest::new(kind, title)).await else {
                    return;
                };
                if specs.iter().any(|spec| spec.path == path) {
                    return;
                }
                let mut locations = specs.clone();
                locations.push(LocationSpec {
                    kind: location_kind,
                    path,
                });
                change(SavePreferences {
                    locations: Some(locations),
                    ..preferences
                });
            });
        }
    };
    let save = {
        let (data, entry_id) = (data.clone(), entry_id.clone());
        move |_| {
            let Some((preferences, revision)) = draft.peek().clone() else {
                return;
            };
            let work = (data.edit_saves)(entry_id.clone(), preferences, revision);
            pending.set(true);
            error.set(None);
            spawn(async move {
                let result = work.await;
                pending.set(false);
                match result {
                    Ok(_) => {
                        draft.set(None);
                        saved.set(true);
                    }
                    Err(report) => error.set(Some(report.summary)),
                }
            });
        }
    };
    let failure = match &loaded {
        Some(Err(report)) => Some(report.summary.clone()),
        _ => error(),
    };
    // What the core found at a path: its kind and how many files it holds.
    let found = |spec: &LocationSpec| {
        state.as_ref().and_then(|state| {
            state
                .locations
                .iter()
                .chain(&state.automatic)
                .find(|row| row.spec.path == spec.path)
                .cloned()
        })
    };
    rsx! {
        div {
            "data-cs": "game-saves",
            "data-choice": if preferences.as_ref().is_some_and(|preferences| preferences.locations.is_some()) { "chosen" } else { "automatic" },
            class: "flex flex-col gap-4",
            if let Some(options) = options {
                Select {
                    label: t!("Release"),
                    options,
                    value: Some(entry_id.clone()),
                    disabled: dirty || busy,
                    on_change: move |id: String| on_release.call(id),
                }
            }
            if loaded.is_none() {
                p { role: "status", class: "text-sm text-fg-muted", {t!("Finding save locations…")} }
            }
            if let (Some(state), Some(preferences)) = (state.clone(), preferences.clone()) {
                p { "data-part": "match", "data-match": match_id(&state.matched), class: "text-sm text-fg-muted",
                    {match_text(&state.matched)}
                }
                TextField {
                    label: t!("Manifest game name"),
                    description: t!("Leave blank to match automatically. A name from the Ludusavi manifest sets the match by hand."),
                    value: preferences.manifest_title.clone().unwrap_or_default(),
                    disabled: busy,
                    on_input: {
                        let (mut change, preferences) = (change.clone(), preferences.clone());
                        move |value: String| change(SavePreferences {
                            manifest_title: (!value.trim().is_empty()).then_some(value),
                            ..preferences.clone()
                        })
                    },
                }
                p { "data-part": "mode", class: "text-sm text-fg-muted",
                    if preferences.locations.is_none() {
                        {t!("Using automatic locations.")}
                    } else {
                        {t!("Using your chosen locations.")}
                    }
                }
                ul { "data-part": "locations", "aria-label": t!("Save locations"), class: "flex flex-col gap-2",
                    for spec in specs.iter().cloned() {
                        {
                            let row = found(&spec);
                            let kind = row.as_ref().map(|row| row.spec.kind).unwrap_or(spec.kind);
                            let files = row.as_ref().map(|row| row.files).unwrap_or(0);
                            let remove = {
                                let (mut change, preferences, specs, path) =
                                    (change.clone(), preferences.clone(), specs.clone(), spec.path.clone());
                                move |_| change(SavePreferences {
                                    locations: Some(specs.iter().filter(|other| other.path != path).cloned().collect()),
                                    ..preferences.clone()
                                })
                            };
                            rsx! {
                                li {
                                    key: "{kind_id(spec.kind)}:{spec.path}",
                                    "data-part": "location",
                                    "data-kind": kind_id(kind),
                                    class: "flex items-center gap-3 rounded-control border border-border p-3",
                                    div { class: "min-w-0 flex-1",
                                        span { class: "text-xs text-fg-muted",
                                            {kind_label(kind)}
                                            if kind != LocationKind::Registry {
                                                {format!(" · {}", tn!("# save file", "# save files", files))}
                                            }
                                        }
                                        p { "data-part": "path", class: "break-all font-mono text-sm", "{spec.path}" }
                                    }
                                    Button {
                                        "data-spatial": "focusable",
                                        icon: TRASH,
                                        "aria-label": t!("Remove {path}", path = spec.path.clone()),
                                        disabled: busy,
                                        on_press: remove,
                                    }
                                }
                            }
                        }
                    }
                }
                if specs.is_empty() {
                    p { "data-part": "empty", class: "text-sm text-fg-muted",
                        {t!("No save files or folders. Add them here.")}
                    }
                }
                if !state.warnings.is_empty() {
                    ul { "data-part": "warnings", class: "flex flex-col gap-1 text-sm text-fg-muted",
                        for warning in state.warnings.iter() {
                            li { key: "{warning}", "{warning}" }
                        }
                    }
                }
                div { class: "flex flex-wrap gap-2",
                    Button {
                        "data-spatial": "focusable",
                        icon: FOLDER_PLUS,
                        disabled: busy,
                        on_press: {
                            let mut add = add.clone();
                            move |_| add(PickKind::Folder)
                        },
                        {t!("Add folder")}
                    }
                    Button {
                        "data-spatial": "focusable",
                        icon: FILE_PLUS,
                        disabled: busy,
                        on_press: {
                            let mut add = add.clone();
                            move |_| add(PickKind::File)
                        },
                        {t!("Add file")}
                    }
                    Button {
                        "data-spatial": "focusable",
                        icon: ROTATE_CCW,
                        disabled: busy || (preferences.locations.is_none() && preferences.manifest_title.is_none()),
                        on_press: {
                            let mut change = change.clone();
                            move |_| change(SavePreferences { locations: None, manifest_title: None })
                        },
                        {t!("Reset to automatic")}
                    }
                }
                div { class: "flex gap-2",
                    Button {
                        "data-spatial": "focusable",
                        variant: ButtonVariant::Primary,
                        disabled: !dirty || busy,
                        on_press: save,
                        if busy { {t!("Saving…")} } else { {t!("Save locations")} }
                    }
                    if dirty {
                        Button {
                            "data-spatial": "focusable",
                            disabled: busy,
                            on_press: move |_| {
                                draft.set(None);
                                error.set(None);
                            },
                            {t!("Revert")}
                        }
                    }
                }
                if saved() {
                    p { role: "status", class: "text-sm", {t!("Save locations saved.")} }
                }
            }
            if let Some(failure) = failure {
                p { "data-part": "error", role: "alert", class: "text-sm text-danger", "{failure}" }
            }
        }
    }
}
