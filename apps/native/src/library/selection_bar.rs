//! The bar of a view in selection mode (#99, the webview's
//! `components/gameSelection/SelectionBar.tsx`): how many games are
//! chosen, choosing the loaded ones or every game the view finds (with how
//! many that is), clearing, the bulk editor, refreshing details and leaving
//! selection mode.

use super::bulk_edit::BulkEditDialog;
use super::selection::GameSelection;
use crate::router::RouteLink;
use crate::ui::{Button, ButtonSize, ButtonVariant, Dialog, Segment, SegmentedControl, icons};
use caskade_core::error::ErrorReport;
use caskade_core::metadata::RefreshMode;
use caskade_ui_i18n::{t, tn};
use dioxus_native::prelude::*;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

/// Every game the view finds, for "select all".
pub type AllIds = Rc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<Vec<String>, ErrorReport>>>>>;

#[component]
pub fn SelectionBar(
    selection: GameSelection,
    /// How many games the view finds.
    total: usize,
    /// Every game the view finds; `None` where it can't list them.
    #[props(default)]
    all_ids: Option<AllIdsProp>,
) -> Element {
    let mut selection = selection;
    let mut editing = use_signal(|| None::<Vec<String>>);
    let mut refreshing = use_signal(|| None::<Vec<String>>);
    let mut selecting = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    // A pad's Start in selection mode: the bulk editor.
    let edits = selection.edits();
    let mut seen = use_signal(|| edits);
    if edits != *seen.peek() {
        seen.set(edits);
        if selection.count() > 0 && editing.peek().is_none() {
            editing.set(Some(selection.ids()));
        }
    }
    if !selection.active() {
        return rsx! {};
    }
    let count = selection.count();
    let select_all = all_ids.clone().map(|all| {
        move |_| {
            let all = all.0.clone();
            selecting.set(true);
            error.set(None);
            spawn(async move {
                match all().await {
                    Ok(ids) => selection.add(ids),
                    Err(report) => error.set(Some(report.summary)),
                }
                selecting.set(false);
            });
        }
    });
    rsx! {
        div {
            "data-cs": "selection-bar",
            role: "toolbar",
            "aria-label": t!("Selected games"),
            class: "sticky bottom-2 z-30 mx-auto mt-3 flex w-full max-w-4xl flex-wrap items-center gap-2 rounded-panel border border-accent-alt/50 bg-surface-overlay/95 px-4 py-3 shadow-lg backdrop-blur",
            p { "data-part": "count", role: "status", class: "mr-auto text-sm font-medium",
                if count == 0 {
                    {t!("Choose games: click them, Shift+click for a range.")}
                } else {
                    {tn!("# game selected", "# games selected", count)}
                }
                if let Some(error) = error() {
                    span { class: "ml-2 text-danger", "{error}" }
                }
            }
            Button {
                "data-part": "select-loaded",
                "data-spatial": "focusable",
                size: ButtonSize::Sm,
                variant: ButtonVariant::Ghost,
                on_press: move |_| {
                    let loaded = selection.loaded(total);
                    selection.add(loaded);
                },
                {t!("Select loaded")}
            }
            if let Some(select_all) = select_all {
                Button {
                    "data-part": "select-all",
                    "data-spatial": "focusable",
                    size: ButtonSize::Sm,
                    variant: ButtonVariant::Ghost,
                    icon: icons::CHECK_CHECK,
                    disabled: selecting(),
                    on_press: select_all,
                    if selecting() {
                        {t!("Selecting…")}
                    } else {
                        {tn!("Select all # game", "Select all # games", total)}
                    }
                }
            }
            Button {
                "data-part": "clear",
                "data-spatial": "focusable",
                size: ButtonSize::Sm,
                variant: ButtonVariant::Ghost,
                disabled: count == 0,
                on_press: move |_| selection.clear(),
                {t!("Clear")}
            }
            Button {
                "data-part": "edit",
                "data-spatial": "focusable",
                size: ButtonSize::Sm,
                variant: ButtonVariant::Primary,
                icon: icons::PENCIL,
                disabled: count == 0,
                on_press: move |_| editing.set(Some(selection.ids())),
                {t!("Edit selected…")}
            }
            Button {
                "data-part": "refresh",
                "data-spatial": "focusable",
                size: ButtonSize::Sm,
                variant: ButtonVariant::Ghost,
                icon: icons::REFRESH_CW,
                disabled: count == 0,
                on_press: move |_| refreshing.set(Some(selection.ids())),
                {t!("Refresh details…")}
            }
            Button {
                "data-part": "done",
                "data-spatial": "focusable",
                size: ButtonSize::Sm,
                icon: icons::X,
                on_press: move |_| selection.stop(),
                {t!("Done")}
            }
            if let Some(game_ids) = editing() {
                BulkEditDialog { game_ids, on_close: move |_| editing.set(None) }
            }
            if let Some(game_ids) = refreshing() {
                RefreshDialog { game_ids, on_close: move |_| refreshing.set(None) }
            }
        }
    }
}

/// [`AllIds`] as a prop: the same lookup is the same prop.
#[derive(Clone)]
pub struct AllIdsProp(pub AllIds);

impl PartialEq for AllIdsProp {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Starts refreshing the chosen games' details and artwork from the
/// built-in sources (the webview's `RefreshDialog.tsx`): missing details
/// only, or the automatic ones fetched again. The user's own fields,
/// artwork choices and "not on IGDB" are always kept.
#[component]
fn RefreshDialog(game_ids: Vec<String>, on_close: EventHandler<()>) -> Element {
    // Where the app can refresh: a running app's metadata.
    let metadata = crate::shell::services().map(|services| services.metadata.clone());
    let mut mode = use_signal(|| RefreshMode::Missing);
    let mut started = use_signal(|| false);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let count = game_ids.len();
    let start = {
        let metadata = metadata.clone();
        move |_| {
            let Some(metadata) = metadata.clone() else {
                return;
            };
            let ids = game_ids.clone();
            let mode = mode();
            pending.set(true);
            error.set(None);
            spawn(async move {
                let result = caskade_app::runtime::spawn(async move {
                    caskade_app::commands::game_edit::games_refresh_start(&metadata, ids, mode)
                        .await
                })
                .await;
                match result {
                    Ok(Ok(())) => started.set(true),
                    Ok(Err(report)) => error.set(Some(report.summary)),
                    Err(_) => error.set(Some(t!("Something went wrong."))),
                }
                pending.set(false);
            });
        }
    };
    let missing = mode() == RefreshMode::Missing;
    rsx! {
        Dialog {
            title: tn!("Refresh # game's details", "Refresh # games' details", count),
            on_close: move |_| on_close.call(()),
            "data-cs": "games-refresh-start",
            class: "max-w-lg!",
            if started() {
                p { class: "text-sm",
                    {crate::trans!("Refreshing in the background. <0>Follow it in Activity</0>.";
                        0 => |inner| rsx! {
                            RouteLink { to: "/activity?tab=jobs".to_owned(), current: false, class: "text-accent-alt", {inner} }
                        })}
                }
            } else {
                SegmentedControl {
                    "aria-label": t!("What to refresh"),
                    value: if missing { "missing" } else { "automatic" },
                    on_change: move |id: String| {
                        mode.set(if id == "automatic" { RefreshMode::Automatic } else { RefreshMode::Missing })
                    },
                    options: vec![
                        Segment::new("missing", t!("Missing details only")),
                        Segment::new("automatic", t!("Refresh automatic details")),
                    ],
                }
                p { class: "text-sm text-fg-muted",
                    if missing {
                        {t!("Matches games that have no match yet and fills in what they lack from what Caskade already has.")}
                    } else {
                        {t!("Also fetches matched games again from the metadata server and replaces their automatic details and artwork.")}
                    }
                    " "
                    {t!("Details and artwork you set, and games you marked as not on IGDB, stay as they are.")}
                }
            }
            if let Some(error) = error() {
                p { role: "alert", class: "text-sm text-danger", "{error}" }
            }
            div { class: "flex justify-end gap-2",
                Button {
                    "data-spatial": "focusable",
                    on_press: move |_| on_close.call(()),
                    if started() { {t!("Close")} } else { {t!("Cancel")} }
                }
                if !started() && metadata.is_some() {
                    Button {
                        "data-part": "start",
                        "data-spatial": "focusable",
                        variant: ButtonVariant::Primary,
                        disabled: pending(),
                        on_press: start,
                        {t!("Refresh")}
                    }
                }
            }
        }
    }
}
