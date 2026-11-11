//! Activity's jobs (#101), as the webview's `views/activity/`: the details
//! refresh's card (`RefreshCard`), then every job, newest first, live from
//! the job queue, each with its progress and what can be done with it; a
//! followed job's questions over whatever view is open; and installing
//! from a link, a path or a dropped file (`ingress.rs`). Notifications are
//! the frame's (`frame/notifications.rs`).

mod anatomy;
mod card;
mod ingress;
mod labels;
mod question;
mod refresh;
#[cfg(test)]
mod tests;

use caskade_core::install::{JobSnapshot, JobStatus};
use caskade_ui_data::ErrorReport;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

pub use anatomy::ANATOMY;
pub use card::{CardAction, JobCard};
pub use ingress::InstallIngress;
pub use question::{JobQuestionHost, follow};

use crate::data::{change, use_jobs_handle, use_jobs_query};
use crate::router::RouteLink;
use crate::ui::Button;

/// Activity's Jobs tab.
#[component]
pub fn JobsPanel() -> Element {
    let mut jobs = use_jobs_query();
    let handle = use_jobs_handle();
    let mut busy = use_signal(|| false);
    let mut failed = use_signal(|| None::<String>);
    let act = use_callback(move |(id, action): (String, CardAction)| {
        if action == CardAction::Review {
            follow(&id);
            return;
        }
        let Some(handle) = handle.clone() else {
            return;
        };
        busy.set(true);
        failed.set(None);
        let follows = matches!(action, CardAction::Retry(_));
        let job = id.clone();
        spawn(async move {
            let outcome: Result<(), ErrorReport> = change(async move {
                let result = match action {
                    CardAction::Cancel => handle.cancel(&job).map(|_| ()),
                    CardAction::Remove => handle.remove(&job),
                    CardAction::Retry(candidate) => handle.retry(&job, candidate).await.map(|_| ()),
                    CardAction::Review => Ok(()),
                };
                result
                    .map_err(|message| ErrorReport::new(caskade_ui_data::ErrorCode::Other, message))
            })
            .await;
            busy.set(false);
            match outcome {
                // A job run again takes the user to its next question.
                Ok(()) if follows => follow(&id),
                Ok(()) => {}
                Err(error) => failed.set(Some(error.summary)),
            }
            jobs.restart();
        });
    });
    let read: Option<Result<Vec<JobSnapshot>, ErrorReport>> = jobs.read().clone();
    rsx! {
        refresh::RefreshCard {}
        match read {
            None => rsx! {
                p { role: "status", {t!("Loading activity…")} }
            },
            Some(Err(error)) => rsx! {
                div { role: "alert", class: "rounded-panel border border-border p-4",
                    p { "{error.summary}" }
                    Button { class: "mt-3", on_press: move |_| jobs.restart(), {t!("Try again")} }
                }
            },
            Some(Ok(list)) => rsx! {
                if let Some(error) = failed() {
                    p { role: "alert", class: "mb-4 text-danger", "{error}" }
                }
                if list.is_empty() {
                    p {
                        "data-part": "empty",
                        class: "rounded-panel border border-dashed border-border p-12 text-center text-fg-muted",
                        {t!("No jobs yet. Install a game from a file, folder or URL.")}
                    }
                }
                div { class: "flex flex-col gap-4",
                    for job in list {
                        div { key: "{job.id}",
                            JobCard {
                                busy: busy(),
                                on_action: {
                                    let id = job.id.clone();
                                    move |action: CardAction| act((id.clone(), action))
                                },
                                job: job.clone(),
                            }
                            if job.status == JobStatus::Completed {
                                if let Some(game) = &job.game_id {
                                    // Its Back link comes back here (#195).
                                    RouteLink {
                                        to: crate::router::Route::Game {
                                            id: game.clone(),
                                            from: Some("/activity".to_owned()),
                                        }
                                        .href(),
                                        current: false,
                                        class: "mt-2 inline-block text-sm text-accent-alt",
                                        {t!("Open game")}
                                    }
                                }
                            }
                        }
                    }
                }
            },
        }
    }
}
