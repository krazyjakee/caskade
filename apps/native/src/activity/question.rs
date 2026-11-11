//! A job's question (#101), as the webview's `QuestionDialog` and
//! `JobQuestionHost`: the choices it offers, picked as a radio group, and
//! Continue; Not now leaves the job waiting in Activity. A job the user
//! follows (Review choices, a retry) has its questions open over whatever
//! view is shown, each in turn, until it ends or the user puts it off.

use caskade_core::install::{JobAnswer, JobSnapshot, JobStatus};
use caskade_ui_data::ErrorReport;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use super::labels::{self, Asked};
use crate::data::{change, use_jobs_handle, use_jobs_query};
use crate::ui::{Button, ButtonVariant, Dialog, RadioGroup, RadioOption};

/// Jobs whose questions open wherever the user is.
static FOLLOWED: GlobalSignal<Vec<String>> = Signal::global(Vec::new);

/// Opens `job_id`'s question now if it waits for one, else once it asks,
/// and each after it until the job ends or the user puts it off.
pub fn follow(job_id: &str) {
    let mut followed = FOLLOWED.write();
    if !followed.iter().any(|id| id == job_id) {
        followed.push(job_id.to_owned());
    }
}

fn unfollow(job_id: &str) {
    FOLLOWED.write().retain(|id| id != job_id);
}

#[component]
pub fn QuestionDialog(
    asked: Asked,
    busy: bool,
    #[props(default)] error: Option<String>,
    on_close: EventHandler<()>,
    on_answer: EventHandler<u32>,
) -> Element {
    let mut selected = use_signal(|| None::<String>);
    let options: Vec<RadioOption> = asked
        .choices
        .iter()
        .map(|choice| RadioOption {
            description: choice.detail.clone(),
            disabled: busy || !asked.can_answer,
            ..RadioOption::new(choice.index.to_string(), choice.label.clone())
        })
        .collect();
    rsx! {
        Dialog {
            title: asked.title.clone(),
            "data-cs": "job-question",
            "data-focus-scope": "activity-question",
            class: "max-w-2xl",
            on_close: move |_| {
                if !busy {
                    on_close.call(());
                }
            },
            p { class: "text-sm text-fg-muted", "{asked.message}" }
            if !asked.choices.is_empty() {
                RadioGroup {
                    label: if asked.can_answer { t!("Choose an option") } else { t!("Installation details") },
                    options,
                    value: selected(),
                    on_change: move |choice: String| selected.set(Some(choice)),
                    class: "mt-5",
                }
            } else {
                p { class: "mt-4 text-sm text-fg-muted",
                    {t!("No selectable options were supplied. Leave this job waiting or cancel it in Activity.")}
                }
            }
            if let Some(error) = error {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
            div { class: "mt-6 flex justify-end gap-2",
                Button {
                    "data-spatial": "focusable",
                    disabled: busy,
                    on_press: move |_| on_close.call(()),
                    {t!("Not now")}
                }
                if asked.can_answer {
                    Button {
                        "data-spatial": "focusable",
                        variant: ButtonVariant::Primary,
                        disabled: selected().is_none() || busy,
                        on_press: move |_| {
                            if let Some(choice) = selected.peek().as_ref().and_then(|choice| choice.parse().ok()) {
                                on_answer.call(choice);
                            }
                        },
                        if busy {
                            {t!("Submitting…")}
                        } else {
                            {t!("Continue")}
                        }
                    }
                }
            }
        }
    }
}

/// The question of a followed job, over whatever view is open.
#[component]
pub fn JobQuestionHost() -> Element {
    let jobs = use_jobs_query();
    let handle = use_jobs_handle();
    let mut pending = use_signal(|| false);
    let mut failed = use_signal(|| None::<String>);
    // Hidden from the answer until the job's next snapshot replaces it.
    let mut answered = use_signal(|| None::<(String, u32)>);
    let list: Vec<JobSnapshot> = match &*jobs.read() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };
    let followed = FOLLOWED.read().clone();
    // A job that ended is no longer followed.
    let finished: Vec<String> = followed
        .iter()
        .filter(|id| {
            list.iter()
                .any(|job| &job.id == *id && job.status.is_finished())
        })
        .cloned()
        .collect();
    use_effect(use_reactive!(|finished| {
        for id in &finished {
            unfollow(id);
        }
    }));
    let job = followed
        .iter()
        .filter_map(|id| list.iter().find(|job| &job.id == id))
        .find(|job| {
            job.status == JobStatus::NeedsUser
                && job.question.is_some()
                && answered() != Some((job.id.clone(), job.revision))
        })
        .cloned();
    let Some(job) = job else {
        return rsx! {};
    };
    let Some(question) = job.question.as_ref() else {
        return rsx! {};
    };
    let asked = labels::question(question);
    let id = job.id.clone();
    let revision = job.revision;
    rsx! {
        QuestionDialog {
            key: "{job.id}:{job.revision}",
            asked,
            busy: pending(),
            error: failed(),
            on_close: {
                let id = id.clone();
                move |_| {
                    failed.set(None);
                    unfollow(&id);
                }
            },
            on_answer: move |index: u32| {
                let Some(handle) = handle.clone() else {
                    return;
                };
                let id = id.clone();
                pending.set(true);
                failed.set(None);
                spawn(async move {
                    let job = id.clone();
                    let outcome: Result<_, ErrorReport> = change(async move {
                        handle
                            .answer(&job, revision, JobAnswer::Select { index })
                            .await
                            .map_err(|message| ErrorReport::new(caskade_ui_data::ErrorCode::Other, message))
                    })
                    .await;
                    pending.set(false);
                    match outcome {
                        Ok(_) => answered.set(Some((id, revision))),
                        Err(error) => failed.set(Some(error.summary)),
                    }
                });
            },
        }
    }
}
