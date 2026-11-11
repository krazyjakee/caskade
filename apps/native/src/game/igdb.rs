//! A game's IGDB match on its page (#100, the webview's
//! `views/game/IgdbPanel.tsx`): what IGDB says about it, how it was
//! matched, and the controls to refresh it, choose another IGDB game (by
//! title or IGDB id), mark it not on IGDB, or match it again.

use super::data::{IgdbChange, use_answer, use_page_data};
use crate::ui::{Button, ButtonSize, ButtonVariant, TextField, icons};
use caskade_core::igdb::IgdbGame;
use caskade_core::metadata::{GameIgdb, MatchSource};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

/// The panel's heading: the database's name, in every language.
const IGDB: &str = "IGDB";

fn source_name(source: MatchSource) -> String {
    match source {
        MatchSource::Romm => t!("Matched by your RomM server"),
        MatchSource::Hasheous => t!("Matched by name (Hasheous)"),
        MatchSource::Store => t!("Matched by store id"),
        MatchSource::Device => t!("Matched on another device"),
        MatchSource::Manual => t!(note = "How the game's IGDB match was made", "Chosen by you"),
        MatchSource::None => t!("No match found"),
    }
}

/// A search result's year and platforms.
fn describe(game: &IgdbGame) -> String {
    let year = game
        .release_date
        .as_deref()
        .map(|date| date.chars().take(4).collect::<String>());
    let platforms: Vec<String> = game
        .platforms
        .iter()
        .filter_map(|platform| platform.name.clone().or(platform.abbreviation.clone()))
        .collect();
    [year, (!platforms.is_empty()).then(|| platforms.join(" · "))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A `YYYY-MM-DD` release date as a calendar day, without a time zone;
/// anything else as it is.
fn release_date(date: &str) -> String {
    let mut parts = date.splitn(3, '-').map(|part| part.parse::<u32>().ok());
    match (
        parts.next().flatten(),
        parts.next().flatten(),
        parts.next().flatten(),
    ) {
        (Some(year), Some(month), Some(day)) => {
            caskade_ui_i18n::format::day(caskade_ui_i18n::intl::DateTime {
                year: year as i32,
                month: month as u8,
                day: day as u8,
                hour: 0,
                minute: 0,
                second: 0,
            })
        }
        _ => date.to_owned(),
    }
}

/// Whether `text` is an IGDB id.
fn is_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 19
        && !text.starts_with('0')
        && text.chars().all(|c| c.is_ascii_digit())
}

#[component]
pub fn IgdbPanel(
    game_id: String,
    title: String,
    /// A new match merged the game into another that is the same game.
    on_merged: EventHandler<String>,
) -> Element {
    let data = use_page_data();
    let matched = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(DataKey::Game(game_id.clone()), move || {
            (data.igdb)(game_id.clone())
        })
    };
    let mut draft = use_signal(|| None::<String>);
    let mut results = use_signal(|| None::<Result<Vec<IgdbGame>, String>>);
    let mut searching = use_signal(|| false);
    let mut changing = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let read = matched.read();
    let state: Option<Result<Option<GameIgdb>, String>> = read
        .as_ref()
        .map(|result| result.clone().map_err(|report| report.summary));
    drop(read);
    let current = state
        .as_ref()
        .and_then(|state| state.as_ref().ok().cloned().flatten());
    let current_id = current.as_ref().and_then(|current| current.igdb_id);
    let change = {
        let (data, game_id) = (data.clone(), game_id.clone());
        move |what: IgdbChange| {
            let (data, game_id) = (data.clone(), game_id.clone());
            changing.set(true);
            error.set(None);
            spawn(async move {
                match (data.igdb_change)(game_id.clone(), what).await {
                    Ok(Some(changed)) if changed.game_id != game_id => {
                        on_merged.call(changed.game_id)
                    }
                    Ok(_) => draft.set(None),
                    Err(report) => error.set(Some(report.summary)),
                }
                changing.set(false);
            });
        }
    };
    let search = {
        let data = data.clone();
        move |text: String| {
            let data = data.clone();
            let query = text.trim().to_owned();
            if query.is_empty() {
                results.set(None);
                return;
            }
            searching.set(true);
            spawn(async move {
                let found = (data.igdb_search)(query)
                    .await
                    .map_err(|report| report.summary);
                results.set(Some(found));
                searching.set(false);
            });
        }
    };
    let source = current.as_ref().map(|current| {
        let mut parts = vec![source_name(current.source)];
        if let Some(id) = current.igdb_id {
            parts.push(t!("IGDB ID {igdbId}", igdbId = id.to_string()));
        }
        if current.pending {
            parts.push(t!(
                note = "The IGDB match's details aren't applied yet",
                "not filled in yet"
            ));
        }
        parts.join(" · ")
    });
    let typed = draft().unwrap_or_default();
    let typed_id = typed.trim().to_owned();
    rsx! {
        section { "data-part": "igdb", class: "mt-8", "aria-labelledby": "igdb-heading",
            h2 { id: "igdb-heading", class: "font-display text-xl font-semibold", {IGDB} }
            div { class: "mt-4 rounded-panel border border-border bg-surface-raised p-4",
                match &state {
                    None => rsx! { p { role: "status", class: "text-fg-muted", {t!("Loading the IGDB match…")} } },
                    Some(Err(error)) => rsx! { p { class: "text-fg-muted", "{error}" } },
                    Some(Ok(None)) => rsx! {
                        OnlineLookups {
                            p { class: "text-fg-muted", {t!("Not matched yet. Caskade matches games with IGDB in the background.")} }
                        }
                    },
                    Some(Ok(Some(current))) => rsx! {
                        Facts { current: current.clone() }
                    },
                }
                if let Some(source) = source {
                    p { "data-part": "igdb-source", class: "mt-3 text-xs text-fg-subtle", "{source}" }
                }
                if draft().is_none() {
                    div { class: "mt-4 flex flex-wrap gap-2",
                        if current_id.is_some() {
                            Button {
                                "data-part": "igdb-refresh",
                                size: ButtonSize::Sm,
                                icon: Some(icons::REFRESH_CW),
                                disabled: changing(),
                                on_press: {
                                    let mut change = change.clone();
                                    move |_| change(IgdbChange::Refresh)
                                },
                                {t!(note = "Button: fetches the IGDB game's details again", "Refresh")}
                            }
                        }
                        Button {
                            "data-part": "igdb-change",
                            size: ButtonSize::Sm,
                            icon: Some(icons::SEARCH),
                            disabled: changing() || state.is_none(),
                            on_press: {
                                let title = title.clone();
                                let mut search = search.clone();
                                move |_| {
                                    error.set(None);
                                    draft.set(Some(title.clone()));
                                    search(title.clone());
                                }
                            },
                            if current_id.is_some() { {t!("Change IGDB game")} } else { {t!("Set IGDB game")} }
                        }
                        if current_id.is_some() {
                            Button {
                                "data-part": "igdb-none",
                                size: ButtonSize::Sm,
                                variant: ButtonVariant::Ghost,
                                icon: Some(icons::X),
                                disabled: changing(),
                                on_press: {
                                    let mut change = change.clone();
                                    move |_| change(IgdbChange::Set(None))
                                },
                                {t!("Not on IGDB")}
                            }
                        }
                        if current.as_ref().is_none_or(|current| matches!(current.source, MatchSource::Manual | MatchSource::None)) && state.is_some() {
                            Button {
                                "data-part": "igdb-rematch",
                                size: ButtonSize::Sm,
                                variant: ButtonVariant::Ghost,
                                icon: Some(icons::SPARKLES),
                                disabled: changing(),
                                on_press: {
                                    let mut change = change.clone();
                                    move |_| change(IgdbChange::Rematch)
                                },
                                if current.as_ref().is_some_and(|current| current.source == MatchSource::Manual) {
                                    {t!("Match automatically")}
                                } else {
                                    {t!("Match now")}
                                }
                            }
                        }
                    }
                } else {
                    div { class: "mt-4",
                        div { class: "flex flex-wrap items-end gap-2",
                            TextField {
                                class: "w-96 max-w-full",
                                "data-part": "igdb-search",
                                label: t!("Search IGDB"),
                                description: t!("A title, or the number listed as IGDB ID on the game's igdb.com page."),
                                icon: Some(icons::SEARCH),
                                "data-focus-default": "true",
                                value: typed.clone(),
                                on_input: move |text: String| draft.set(Some(text)),
                                on_submit: {
                                    let mut search = search.clone();
                                    let mut change = change.clone();
                                    move |text: String| {
                                        let text = text.trim().to_owned();
                                        if is_id(&text) {
                                            change(IgdbChange::Set(Some(text)));
                                        } else {
                                            search(text);
                                        }
                                    }
                                },
                            }
                            Button {
                                size: ButtonSize::Sm,
                                variant: ButtonVariant::Ghost,
                                on_press: move |_| draft.set(None),
                                {t!("Cancel")}
                            }
                        }
                        ul { "data-part": "igdb-results", "aria-label": t!("IGDB games"), class: "mt-3 flex flex-col gap-1.5",
                            if is_id(&typed_id) {
                                ResultRow {
                                    title: t!("IGDB ID {id}", id = typed_id.clone()),
                                    detail: t!("The game with this id on igdb.com"),
                                    current: current_id.map(|id| id.to_string()) == Some(typed_id.clone()),
                                    disabled: changing(),
                                    on_use: {
                                        let mut change = change.clone();
                                        let id = typed_id.clone();
                                        move |_| change(IgdbChange::Set(Some(id.clone())))
                                    },
                                }
                            }
                            if let Some(Ok(found)) = results() {
                                for result in found {
                                    ResultRow {
                                        key: "{result.id}",
                                        title: result.title.clone(),
                                        detail: describe(&result),
                                        url: result.url.clone(),
                                        current: current_id == Some(result.id),
                                        disabled: changing(),
                                        on_use: {
                                            let mut change = change.clone();
                                            let id = result.id.to_string();
                                            move |_| change(IgdbChange::Set(Some(id.clone())))
                                        },
                                    }
                                }
                            }
                        }
                        if searching() || changing() {
                            p { role: "status", class: "mt-2 text-sm text-fg-muted",
                                if changing() { {t!("Checking the IGDB game…")} } else { {t!("Searching IGDB…")} }
                            }
                        }
                        if let Some(Err(message)) = results() {
                            p { class: "mt-2 text-sm text-fg-muted", "{message}" }
                        }
                        if results().is_some_and(|found| found.is_ok_and(|found| found.is_empty())) && !searching() {
                            p { class: "mt-2 text-sm text-fg-muted",
                                {t!("No IGDB games match “{searched}”.", searched = typed_id.clone())}
                            }
                        }
                    }
                }
                if let Some(error) = error() {
                    p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
                }
            }
        }
    }
}

/// What IGDB says about the matched game.
#[component]
fn Facts(current: GameIgdb) -> Element {
    let Some(game) = current.game.clone() else {
        return match current.igdb_id {
            Some(id) => rsx! {
                OnlineLookups {
                    p { class: "text-fg-muted",
                        {t!("IGDB game {igdbId}. Its details show once Caskade can reach the metadata server.", igdbId = id.to_string())}
                    }
                }
            },
            None if current.source == MatchSource::Manual => rsx! {
                p { class: "text-fg-muted", {t!("You marked this game as not on IGDB.")} }
            },
            None => {
                rsx! {
                    OnlineLookups {
                        p { class: "text-fg-muted", {t!("Caskade found no IGDB game for it.")} }
                    }
                }
            }
        };
    };
    let platforms: Vec<String> = game
        .platforms
        .iter()
        .filter_map(|platform| platform.name.clone().or(platform.abbreviation.clone()))
        .collect();
    rsx! {
        dl { "data-part": "igdb-facts", class: "grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm",
            dt { class: "text-fg-subtle", {t!(note = "The IGDB game the library game is matched to", "Game")} }
            dd {
                if let Some(url) = game.url.clone() {
                    super::ExternalLink { href: url, class: "inline-flex items-center gap-1.5 text-accent-alt", "{game.title}" }
                } else {
                    "{game.title}"
                }
            }
            if let Some(date) = game.release_date.clone() {
                dt { class: "text-fg-subtle", {t!("Released")} }
                dd { {release_date(&date)} }
            }
            if let Some(rating) = game.total_rating {
                dt { class: "text-fg-subtle", {t!("IGDB rating")} }
                dd { class: "font-mono", {t!(note = "A rating out of 100", "{rating} / 100", rating = (rating.round() as i64).to_string())} }
            }
            if !game.genres.is_empty() {
                dt { class: "text-fg-subtle", {t!("Genres")} }
                dd { {game.genres.join(" · ")} }
            }
            if !platforms.is_empty() {
                dt { class: "text-fg-subtle", {t!("Platforms")} }
                dd { {platforms.join(" · ")} }
            }
        }
    }
}

#[component]
fn ResultRow(
    title: String,
    detail: String,
    #[props(default)] url: Option<String>,
    current: bool,
    disabled: bool,
    on_use: EventHandler<()>,
) -> Element {
    rsx! {
        li { "data-part": "igdb-result", class: "flex items-center justify-between gap-3 rounded-control border border-border px-3 py-2",
            div { class: "min-w-0",
                p { class: "truncate text-sm font-medium",
                    if let Some(url) = url {
                        super::ExternalLink { href: url, class: "inline-flex items-center gap-1.5", "{title}" }
                    } else {
                        "{title}"
                    }
                }
                if !detail.is_empty() {
                    p { class: "truncate text-xs text-fg-muted", "{detail}" }
                }
            }
            if current {
                span { class: "text-xs text-accent-alt", {t!("Current match")} }
            } else {
                Button {
                    "data-part": "use",
                    size: ButtonSize::Sm,
                    "aria-label": t!("Use {title}", title = title.clone()),
                    disabled,
                    on_press: move |_| on_use.call(()),
                    {t!("Use")}
                }
            }
        }
    }
}

/// `children`, or while online lookups are off on this device, why the
/// game isn't matched or filled in and the switch to turn them on.
#[component]
fn OnlineLookups(children: Element) -> Element {
    let metadata = crate::settings::use_settings_section("core.metadata");
    let off = metadata
        .value()
        .is_some_and(|value| value.get("online") == Some(&serde_json::Value::Bool(false)));
    if !off {
        return children;
    }
    rsx! {
        div { "data-part": "igdb-lookups-off",
            p { class: "text-fg-muted",
                {t!("Online lookups are off on this device, so Caskade doesn't match games with IGDB or fetch their details and artwork.")}
            }
            Button {
                size: ButtonSize::Sm,
                icon: icons::CLOUD,
                class: "mt-3",
                disabled: metadata.saving(),
                on_press: move |_| metadata.set_field("online", serde_json::Value::Bool(true)),
                {t!("Turn on online lookups")}
            }
            if let Some(error) = metadata.save_error() {
                p { role: "alert", class: "mt-2 text-sm text-danger", "{error.summary}" }
            }
        }
    }
}
