//! A job in Activity (#101), as the webview's `JobCard`, `ProgressLine` and
//! `JobRecovery`: its kind, phase, status and progress; Review choices,
//! Pause and Resume a download (#131), Cancel job, Retry (as before or
//! another install way) and Remove; why a
//! failed install stopped and what can be done; verification results,
//! warnings, dependencies and the install ways the installer weighed. A
//! right click offers the card's buttons.

use caskade_core::install::{JobSnapshot, JobStatus, Recovery};
use caskade_installer::failure::{Failure, FailureStage, RecoveryAction};
use caskade_ui_data::ErrorReport;
use caskade_ui_i18n::{format, t, tn, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use super::labels;
use crate::data::change;
use crate::flows::paths::{PathRequest, PickKind, use_path_picker};
use crate::ui::icons::{
    FOLDER_SEARCH, HARD_DRIVE, LIST_CHECKS, PAUSE, PLAY, ROTATE_CCW, TIMER_OFF, TRASH, X,
};
use crate::ui::{Button, ButtonSize, ButtonVariant, Dialog, Menu, MenuEntry, MenuItem};

/// What a card asks Activity to do.
#[derive(Clone, Debug, PartialEq)]
pub enum CardAction {
    /// Pause a download, keeping it (#131).
    Pause,
    /// Go on with a paused download.
    Resume,
    Cancel,
    Remove,
    /// Again as before, or with the install way at this index.
    Retry(Option<u32>),
    /// Open the job's question.
    Review,
}

/// A job's progress bar, with what it is doing on the left and how far it
/// is on the right: a percentage of a known total, else `pending` while the
/// bar stays a quarter full.
#[component]
pub fn ProgressLine(
    label: String,
    status: String,
    done: u64,
    total: Option<u64>,
    #[props(default)] pending: Option<String>,
    /// The bar's `data-part`.
    #[props(default)]
    part: Option<&'static str>,
    #[props(default = "mt-3".to_owned())] class: String,
) -> Element {
    let total = total.filter(|total| *total > 0);
    let ratio = total.map(|total| (done as f64 / total as f64).min(1.0));
    let right = match ratio {
        Some(ratio) => format::percent(Some(ratio * 100.0)),
        None => pending.unwrap_or_default(),
    };
    let width = ratio.map_or("25%".to_owned(), |ratio| format!("{}%", ratio * 100.0));
    rsx! {
        div {
            role: "progressbar",
            "aria-label": "{label}",
            "aria-valuemin": "0",
            "aria-valuemax": "100",
            "aria-valuenow": ratio.map(|ratio| format!("{}", (ratio * 100.0).round())),
            "aria-busy": ratio.is_none().then_some("true"),
            class: "{class}",
            div { class: "mb-2 flex justify-between text-xs text-fg-muted",
                span { "{status}" }
                span { "{right}" }
            }
            div { class: "h-1.5 overflow-hidden rounded-full bg-surface-overlay",
                div {
                    "data-part": part,
                    class: "h-full rounded-full bg-accent-alt",
                    style: "width: {width}",
                }
            }
        }
    }
}

#[component]
pub fn JobCard(job: JobSnapshot, busy: bool, on_action: EventHandler<CardAction>) -> Element {
    let mut article = use_signal(|| None::<NodeHandle>);
    let mut menu = use_signal(|| None);
    let mut advanced = use_signal(|| false);
    let heading = format!("job-{}", job.id);
    let ended = matches!(job.status, JobStatus::Failed | JobStatus::Cancelled);
    // A classified failure brings its own recovery buttons (#158).
    let failure = job
        .failure
        .clone()
        .filter(|_| job.status == JobStatus::Failed);
    let strategies = labels::strategies(&job);
    let mut items = Vec::new();
    if job.status == JobStatus::NeedsUser && job.question.is_some() {
        let mut item = MenuItem::new("review", t!("Review choices"));
        item.icon = Some(LIST_CHECKS);
        items.push(MenuEntry::Item(item));
    }
    let pause_label = t!(
        note = "Stops a download for now, keeping what it downloaded",
        "Pause"
    );
    let resume_label = t!(
        note = "Goes on with a paused download from where it stopped",
        "Resume"
    );
    if labels::can_resume(&job) {
        let mut item = MenuItem::new("resume", resume_label.clone());
        item.icon = Some(PLAY);
        item.disabled = busy;
        items.push(MenuEntry::Item(item));
    }
    if labels::can_pause(&job) {
        let mut item = MenuItem::new("pause", pause_label.clone());
        item.icon = Some(PAUSE);
        item.disabled = busy;
        items.push(MenuEntry::Item(item));
    }
    if labels::is_active(&job) {
        let mut item = MenuItem::new("cancel", t!("Cancel job"));
        item.icon = Some(X);
        item.danger = true;
        item.disabled = busy;
        items.push(MenuEntry::Item(item));
    }
    if labels::can_retry(&job) {
        let mut item = MenuItem::new("retry", tp!("job", "Retry"));
        item.icon = Some(ROTATE_CCW);
        item.disabled = busy;
        items.push(MenuEntry::Item(item));
    }
    if ended {
        for (index, label, _, _) in &strategies {
            let mut item = MenuItem::new(
                format!("retry:{index}"),
                t!(
                    note = "Runs a failed job again with another install strategy, named by the installer",
                    "Retry with {strategyLabel}",
                    strategyLabel = label.clone()
                ),
            );
            item.icon = Some(ROTATE_CCW);
            item.disabled = busy;
            items.push(MenuEntry::Item(item));
        }
    }
    let remove_label = t!(
        note = "Takes a failed or cancelled job out of Activity; its files stay",
        "Remove"
    );
    if labels::can_remove(&job) {
        let mut item = MenuItem::new("remove", remove_label.clone());
        item.icon = Some(TRASH);
        item.disabled = busy;
        items.push(MenuEntry::Item(item));
    }
    let progress = job.progress.clone();
    let (done, total, amount) = match &progress {
        Some(progress) if progress.total_bytes.is_some() || progress.bytes > 0 => (
            progress.bytes,
            progress.total_bytes,
            format::size(Some(progress.bytes)),
        ),
        Some(progress) => (
            progress.files,
            None,
            tn!("# file", "# files", progress.files as usize),
        ),
        None => (0, None, t!("Waiting for progress")),
    };
    let message = progress
        .as_ref()
        .map(|progress| progress.message.clone())
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| t!("Working…"));
    // A paused download keeps its bar, standing still.
    let message = if labels::can_resume(&job) {
        labels::status(job.status)
    } else {
        message
    };
    let kind_line = if labels::phase_differs(&job) {
        format!("{} · {}", labels::kind(job.kind), labels::phase(job.phase))
    } else {
        labels::kind(job.kind)
    };
    let mut warnings: Vec<String> = Vec::new();
    for warning in &job.warnings {
        let text = warning.message.clone();
        if !warnings.contains(&text) {
            warnings.push(text);
        }
    }
    let title = job.title.clone();
    rsx! {
        article {
            "aria-labelledby": "{heading}",
            "data-cs": "activity-job",
            "data-status": labels::status_name(job.status),
            class: "rounded-panel border border-border bg-surface-raised p-5",
            onmounted: move |event: Event<MountedData>| article.set(event.data().downcast::<NodeHandle>().cloned()),
            oncontextmenu: move |event: MouseEvent| {
                menu.set(Some(crate::frame::pointer_anchor(article.peek().as_ref(), &event)));
            },
            if let Some(anchor) = menu() {
                Menu {
                    anchor,
                    items,
                    label: title.clone(),
                    on_close: move |_| menu.set(None),
                    on_action: move |id: String| {
                        menu.set(None);
                        on_action.call(match id.as_str() {
                            "review" => CardAction::Review,
                            "pause" => CardAction::Pause,
                            "resume" => CardAction::Resume,
                            "cancel" => CardAction::Cancel,
                            "remove" => CardAction::Remove,
                            "retry" => CardAction::Retry(None),
                            other => CardAction::Retry(other.strip_prefix("retry:").and_then(|index| index.parse().ok())),
                        });
                    },
                }
            }
            div { class: "flex flex-wrap items-start justify-between gap-3",
                div { class: "min-w-0",
                    p { class: "font-mono text-xs text-fg-subtle uppercase", "{kind_line}" }
                    h2 { id: "{heading}", class: "mt-1 break-words font-display text-lg font-semibold", "{job.title}" }
                    p { class: "mt-1 text-sm text-fg-muted", {labels::status(job.status)} }
                }
                div { class: "flex gap-2",
                    if job.status == JobStatus::NeedsUser && job.question.is_some() {
                        Button {
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Primary,
                            on_press: move |_| on_action.call(CardAction::Review),
                            {t!("Review choices")}
                        }
                    }
                    if labels::can_resume(&job) {
                        Button {
                            id: "job-{job.id}-resume",
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Primary,
                            icon: Some(PLAY),
                            disabled: busy,
                            on_press: move |_| on_action.call(CardAction::Resume),
                            "{resume_label}"
                        }
                    }
                    if labels::can_pause(&job) {
                        Button {
                            id: "job-{job.id}-pause",
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Ghost,
                            icon: Some(PAUSE),
                            disabled: busy,
                            on_press: move |_| on_action.call(CardAction::Pause),
                            "{pause_label}"
                        }
                    }
                    if labels::is_active(&job) {
                        Button {
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Ghost,
                            disabled: busy,
                            on_press: move |_| on_action.call(CardAction::Cancel),
                            {t!("Cancel job")}
                        }
                    }
                    if labels::can_retry(&job) && failure.is_none() {
                        Button {
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Primary,
                            icon: Some(ROTATE_CCW),
                            disabled: busy,
                            on_press: move |_| on_action.call(CardAction::Retry(None)),
                            {tp!("job", "Retry")}
                        }
                    }
                    if labels::can_remove(&job) {
                        Button {
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Ghost,
                            icon: Some(TRASH),
                            disabled: busy,
                            on_press: move |_| on_action.call(CardAction::Remove),
                            "{remove_label}"
                        }
                    }
                }
            }
            if let Some(target) = &job.target_dir {
                p { class: "mt-3 break-all font-mono text-xs text-fg-subtle",
                    {t!("Target: {targetDir}", targetDir = target.to_string_lossy().into_owned())}
                }
            }
            if labels::is_running(&job) || (labels::can_resume(&job) && progress.is_some()) {
                ProgressLine {
                    label: t!("{title} progress", title = job.title.clone()),
                    status: message,
                    done,
                    total,
                    pending: Some(amount),
                    part: Some("progress"),
                    class: "mt-4",
                }
            }
            if let Some(failure) = failure.clone() {
                FailurePanel { job: job.clone(), failure, busy }
            } else if let Some(error) = &job.error {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
            if let Some(verification) = &job.verification {
                section {
                    "aria-label": t!("Verification results"),
                    class: "mt-4 rounded-control border border-border p-3",
                    h3 { class: "text-sm font-semibold",
                        if verification.valid {
                            {t!("Verification passed")}
                        } else {
                            {t!("Verification found problems")}
                        }
                    }
                    p { class: "mt-1 text-xs text-fg-muted",
                        {tn!("# file checked", "# files checked", verification.checked_files as usize)}
                        " · "
                        {tn!("# missing", "# missing", verification.missing.len())}
                        " · "
                        {tn!("# changed", "# changed", verification.changed.len())}
                    }
                    ul { class: "mt-2 text-xs text-fg-muted",
                        for path in verification.missing.clone() {
                            li { key: "missing:{path}", {t!("Missing: {path}", path = path.clone())} }
                        }
                        for path in verification.changed.clone() {
                            li { key: "changed:{path}", {t!("Changed: {path}", path = path.clone())} }
                        }
                    }
                }
            }
            if !warnings.is_empty() {
                section { "aria-label": t!("Warnings"), class: "mt-4 rounded-control border border-border p-3",
                    h3 { class: "text-sm font-semibold", {t!("Warnings")} }
                    ul { class: "mt-2 flex flex-col gap-1 text-sm text-fg-muted",
                        for warning in warnings {
                            li { key: "{warning}", "{warning}" }
                        }
                    }
                }
            }
            if !job.dependencies.is_empty() {
                section { "aria-label": t!("Dependencies"), class: "mt-4",
                    h3 { class: "text-sm font-semibold", {t!("Dependencies")} }
                    ul { class: "mt-1 text-sm text-fg-muted",
                        for dependency in job.dependencies.clone() {
                            li { key: "{dependency.id}",
                                "{dependency.name} — "
                                if dependency.installed {
                                    {tp!("dependency status", "Installed")}
                                } else {
                                    {tp!("dependency status", "Required")}
                                }
                            }
                        }
                    }
                }
            }
            div { class: "mt-4 border-t border-border pt-3",
                Button {
                    "data-spatial": "focusable",
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    "aria-expanded": if advanced() { "true" } else { "false" },
                    on_press: move |_| advanced.toggle(),
                    {t!("Advanced strategies")}
                }
                if advanced() {
                    div { class: "mt-2 flex flex-col gap-2",
                        if strategies.is_empty() {
                            p { class: "text-sm text-fg-muted", {t!("No strategy details are available yet.")} }
                        }
                        for (index, label, reason, selected) in strategies.clone() {
                            div {
                                key: "{index}",
                                "data-part": "strategy",
                                "data-selected": selected.then_some("true"),
                                class: "rounded-control border border-border p-3",
                                p { class: "text-sm font-semibold",
                                    "{label}"
                                    if selected {
                                        {format!(" · {}", t!(note = "The install strategy the installer chose", "Selected"))}
                                    }
                                }
                                p { class: "mt-1 text-xs text-fg-muted", "{reason}" }
                                if ended {
                                    Button {
                                        "data-spatial": "focusable",
                                        size: ButtonSize::Sm,
                                        class: "mt-2",
                                        disabled: busy,
                                        on_press: move |_| on_action.call(CardAction::Retry(Some(index))),
                                        {t!(
                                            note = "Runs a failed job again with another install strategy, named by the installer",
                                            "Retry with {strategyLabel}",
                                            strategyLabel = label.clone()
                                        )}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn stage_label(stage: FailureStage) -> String {
    match stage {
        FailureStage::Download => t!(note = "The stage a failed install stopped at", "Download"),
        FailureStage::Inspect => t!(
            note = "The stage a failed install stopped at",
            "Reading the installer"
        ),
        FailureStage::Install => t!(note = "The stage a failed install stopped at", "Installing"),
        FailureStage::Finalize => t!(note = "The stage a failed install stopped at", "Finishing"),
        FailureStage::Commit => t!(
            note = "The stage a failed install stopped at",
            "Checking the install"
        ),
    }
}

fn wire<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Why a failed install or update stopped and what can be done about it
/// (#158): the stage, the file, a plain explanation, redacted details and
/// the recovery actions that suit the cause. A pad reaches every button.
#[component]
fn FailurePanel(job: JobSnapshot, failure: Failure, busy: bool) -> Element {
    let jobs = crate::data::use_jobs_handle();
    let picker = use_path_picker();
    let storage = crate::flows::storage::use_storage();
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut details = use_signal(|| false);
    let mut choosing_root = use_signal(|| false);
    let recover = use_callback({
        let jobs = jobs.clone();
        let id = job.id.clone();
        move |recovery: Option<Recovery>| {
            let Some(jobs) = jobs.clone() else {
                return;
            };
            let id = id.clone();
            pending.set(true);
            error.set(None);
            spawn(async move {
                let outcome: Result<(), ErrorReport> = change(async move {
                    let result = match recovery {
                        // Don't retry automatically: the job stops here.
                        None => jobs.cancel(&id).map(|_| ()),
                        Some(recovery) => jobs.recover(&id, recovery).await.map(|_| ()),
                    };
                    result.map_err(|message| {
                        ErrorReport::new(caskade_ui_data::ErrorCode::Other, message)
                    })
                })
                .await;
                pending.set(false);
                if let Err(failed) = outcome {
                    error.set(Some(failed.summary));
                }
            });
        }
    });
    let start_at = failure
        .path
        .as_ref()
        .and_then(|path| path.parent())
        .map(|folder| folder.to_string_lossy().into_owned());
    let run = move |action: RecoveryAction| match action {
        RecoveryAction::LocateSource => {
            let start_at = start_at.clone();
            spawn(async move {
                let mut request = PathRequest::new(PickKind::File, t!("Locate the installer"));
                request.start_at = start_at;
                if let Some(path) = picker.pick(request).await {
                    recover(Some(Recovery::LocateSource { input: path }));
                }
            });
        }
        RecoveryAction::ChooseDestination => choosing_root.set(true),
        RecoveryAction::Redownload => recover(Some(Recovery::Redownload)),
        _ => recover(Some(Recovery::Retry)),
    };
    let actions: Vec<RecoveryAction> = failure
        .actions
        .iter()
        .copied()
        .filter(|action| {
            *action != RecoveryAction::Cancel
                && (*action != RecoveryAction::ChooseDestination
                    || (storage.is_some() && job.kind == caskade_core::install::JobKind::Install))
        })
        .collect();
    let busy = busy || pending();
    let where_ = match &failure.path {
        Some(path) => format!(
            "{} · {}",
            stage_label(failure.stage),
            path.to_string_lossy()
        ),
        None => stage_label(failure.stage),
    };
    let technical = {
        let mut text = format!("{} · {}", wire(&failure.kind), wire(&failure.stage));
        if failure.occurrences > 1 {
            text.push_str(&format!(
                " · {}",
                tn!(
                    "# time in a row",
                    "# times in a row",
                    failure.occurrences as usize
                )
            ));
        }
        format!("{text}\n{}", failure.detail)
    };
    rsx! {
        section {
            "aria-label": t!("What went wrong"),
            "data-part": "failure",
            "data-kind": wire(&failure.kind),
            "data-status": if failure.retry_at_ms.is_some() { "retrying" } else { "waiting" },
            class: "mt-4 rounded-control border border-danger/40 p-3",
            p { role: "alert", class: "text-sm text-danger", "{failure.summary}" }
            p { class: "mt-1 font-mono text-xs text-fg-subtle", "{where_}" }
            p { class: "mt-2 text-sm text-fg-muted",
                match failure.retry_at_ms {
                    Some(at) => t!(
                        "Caskade will try again at {retryAt}.",
                        retryAt = format::time(crate::library::local_date_time(at), caskade_ui_i18n::intl::Clock::System, false)
                    ),
                    None if failure.transient => t!("Caskade isn't retrying this on its own now. Choose what to do next."),
                    None => t!("This needs you: choose what to do next."),
                }
            }
            div { class: "mt-3 flex flex-wrap gap-2",
                if failure.retry_at_ms.is_some() {
                    Button {
                        "data-spatial": "focusable",
                        variant: ButtonVariant::Ghost,
                        icon: Some(TIMER_OFF),
                        disabled: busy,
                        on_press: move |_| recover(None),
                        {t!("Don't retry automatically")}
                    }
                }
                for (index, action) in actions.into_iter().enumerate() {
                    Button {
                        key: "{wire(&action)}",
                        "data-spatial": "focusable",
                        variant: if index == 0 { ButtonVariant::Primary } else { ButtonVariant::Ghost },
                        icon: Some(match action {
                            RecoveryAction::LocateSource => FOLDER_SEARCH,
                            RecoveryAction::ChooseDestination => HARD_DRIVE,
                            _ => ROTATE_CCW,
                        }),
                        disabled: busy,
                        on_press: {
                            let mut run = run.clone();
                            move |_| run(action)
                        },
                        {match action {
                            RecoveryAction::Redownload => t!("Download again"),
                            RecoveryAction::LocateSource => t!("Locate the installer…"),
                            RecoveryAction::ChooseDestination => t!("Choose another location…"),
                            _ => t!("Try again"),
                        }}
                    }
                }
            }
            if let Some(failed) = error() {
                p { role: "alert", class: "mt-2 text-sm text-danger", "{failed}" }
            }
            div { class: "mt-3",
                Button {
                    "data-spatial": "focusable",
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    "aria-expanded": if details() { "true" } else { "false" },
                    on_press: move |_| details.toggle(),
                    {t!("Technical details")}
                }
                if details() {
                    pre { class: "mt-2 whitespace-pre-wrap break-all font-mono text-xs text-fg-muted", "{technical}" }
                }
            }
            if choosing_root() {
                if let Some(storage) = storage.clone() {
                    Dialog {
                        title: t!("Choose another install location"),
                        on_close: move |_| choosing_root.set(false),
                        crate::flows::storage::InstallRootPicker {
                            storage,
                            always_use_default: Some(false),
                            heading: false,
                            on_cancel: move |_| choosing_root.set(false),
                            on_select: move |root_id: String| {
                                choosing_root.set(false);
                                recover(Some(Recovery::ChooseDestination { root_id }));
                            },
                        }
                    }
                }
            }
        }
    }
}
