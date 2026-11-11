//! The refresh of chosen games' details in Activity (#176, moved from #99),
//! as the webview's `RefreshCard`: how far it is, what it changed, Cancel
//! while it runs, and retrying the games that failed or continuing a
//! cancelled one. The library's Refresh details… starts it.

use caskade_ui_data::ErrorReport;
use caskade_ui_i18n::{t, tn};
use dioxus_native::prelude::*;

use crate::data::{change, use_jobs_handle, use_refresh_query};
use crate::ui::icons::REFRESH_CW;
use crate::ui::{Button, ButtonSize, ButtonVariant, Svg};

#[component]
pub fn RefreshCard() -> Element {
    let mut status = use_refresh_query();
    let handle = use_jobs_handle();
    let mut pending = use_signal(|| false);
    let mut failed = use_signal(|| None::<String>);
    let again = use_callback({
        let handle = handle.clone();
        move |(game_ids, mode): (Vec<String>, caskade_core::metadata::RefreshMode)| {
            let Some(handle) = handle.clone() else {
                return;
            };
            pending.set(true);
            failed.set(None);
            spawn(async move {
                let outcome: Result<(), ErrorReport> = change(async move {
                    handle
                        .refresh_start(game_ids, mode)
                        .await
                        .map_err(|message| {
                            ErrorReport::new(caskade_ui_data::ErrorCode::Other, message)
                        })
                })
                .await;
                pending.set(false);
                if let Err(error) = outcome {
                    failed.set(Some(error.summary));
                }
                status.restart();
            });
        }
    });
    let Some(Ok(Some(current))) = status.read().clone() else {
        return rsx! {};
    };
    let state = if current.running {
        "running"
    } else if current.cancelled {
        "cancelled"
    } else {
        "done"
    };
    let title = match state {
        "running" => t!("Refreshing game details"),
        "cancelled" => t!("Refreshing game details was cancelled"),
        _ => t!("Game details refreshed"),
    };
    let percent = (current.done as f64 * 100.0 / current.total.max(1) as f64).round();
    let cancel = {
        let handle = handle.clone();
        move |_| {
            if let Some(handle) = &handle {
                handle.refresh_cancel();
            }
            status.restart();
        }
    };
    let mode = current.mode;
    let failed_ids = current.failed.clone();
    let remaining = current.remaining.clone();
    let missing_only = serde_json::to_value(current.mode)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .as_deref()
        == Some("missing");
    rsx! {
        section {
            "data-cs": "games-refresh",
            "data-status": state,
            "aria-label": t!("Refreshing game details"),
            class: "mb-4 flex flex-col gap-3 rounded-panel border border-border bg-surface-raised p-4",
            div { class: "flex flex-wrap items-center gap-2",
                Svg {
                    icon: REFRESH_CW,
                    class: if current.running { "size-4 animate-worker text-accent-alt motion-reduce:animate-none" } else { "size-4" },
                }
                h2 { "data-part": "title", class: "mr-auto font-semibold", "{title}" }
                span { "data-part": "mode", class: "text-xs text-fg-muted",
                    if missing_only {
                        {t!("Missing details only")}
                    } else {
                        {t!("Automatic details refreshed")}
                    }
                }
            }
            div {
                "data-part": "progress",
                role: "progressbar",
                "aria-valuemin": "0",
                "aria-valuemax": "{current.total}",
                "aria-valuenow": "{current.done}",
                "aria-label": t!("{0} of {1} games", 0 = current.done, 1 = current.total),
                class: "h-1.5 overflow-hidden rounded-full bg-surface-base",
                div { class: "h-full bg-accent", style: "width: {percent}%" }
            }
            ul { "data-part": "counts", class: "flex flex-wrap gap-x-5 gap-y-1 text-sm",
                li { {tn!("# updated", "# updated", current.updated as usize)} }
                li { class: "text-fg-muted", {tn!("# unchanged", "# unchanged", current.unchanged as usize)} }
                li { class: "text-fg-muted", {tn!("# not found on IGDB", "# not found on IGDB", current.unmatched as usize)} }
                li { class: "text-fg-muted", {tn!("# skipped", "# skipped", current.skipped as usize)} }
                if !failed_ids.is_empty() {
                    li { class: "text-danger", {tn!("# failed", "# failed", failed_ids.len())} }
                }
            }
            if let Some(error) = failed() {
                p { role: "alert", class: "text-sm text-danger", "{error}" }
            }
            div { class: "flex flex-wrap gap-2",
                if current.running {
                    Button {
                        "data-part": "cancel",
                        "data-spatial": "focusable",
                        size: ButtonSize::Sm,
                        on_press: cancel,
                        {t!("Cancel")}
                    }
                }
                if !current.running && !failed_ids.is_empty() {
                    Button {
                        "data-part": "retry",
                        "data-spatial": "focusable",
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Primary,
                        disabled: pending(),
                        on_press: {
                            let ids = failed_ids.clone();
                            move |_| again((ids.clone(), mode))
                        },
                        {tn!("Retry the failed game", "Retry the # failed games", failed_ids.len())}
                    }
                }
                if current.cancelled && !remaining.is_empty() {
                    Button {
                        "data-part": "continue",
                        "data-spatial": "focusable",
                        size: ButtonSize::Sm,
                        disabled: pending(),
                        on_press: {
                            let ids = remaining.clone();
                            move |_| again((ids.clone(), mode))
                        },
                        {tn!("Continue with the last game", "Continue with the # games left", remaining.len())}
                    }
                }
            }
        }
    }
}
