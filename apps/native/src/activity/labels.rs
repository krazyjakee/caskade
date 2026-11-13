//! What Activity says about a job (#101), as the webview's `jobLabels.ts`
//! and `installMapping.ts`: its kind, phase and status in words, the
//! install ways the installer weighed, and its question with the choices.

use caskade_core::install::{JobKind, JobPhase, JobSnapshot, JobStatus};
use caskade_installer::{Candidate, Fidelity, Question, Strategy};
use caskade_ui_i18n::{format, t, tp};

/// A job's kind, never the raw id.
pub fn kind(kind: JobKind) -> String {
    match kind {
        JobKind::Install => tp!("job kind", "Install"),
        JobKind::Uninstall => tp!("job kind", "Uninstall"),
        JobKind::Move => tp!("job kind", "Move"),
        JobKind::Fetch => tp!("job kind", "Download"),
        JobKind::Verify => tp!("job kind", "Verify"),
        JobKind::Update => tp!("job kind", "Update"),
        JobKind::PeerCopy => tp!(
            note = "A game copied from another of the user's devices",
            "job kind",
            "Copy from another device"
        ),
        JobKind::MoveLocation => tp!(
            note = "Every game on a storage location moved to another, then the location removed",
            "job kind",
            "Move location"
        ),
        #[allow(unreachable_patterns)]
        _ => tp!(
            note = "A background job of a kind the app has no name for (a plugin's own)",
            "job kind",
            "Job"
        ),
    }
}

/// A job's phase.
pub fn phase(phase: Option<JobPhase>) -> String {
    match phase {
        Some(JobPhase::Download) => t!(note = "A background job's phase", "Downloading"),
        Some(JobPhase::Install) => t!(note = "A background job's phase", "Installing"),
        Some(JobPhase::Move) => t!(note = "A background job's phase", "Moving"),
        None => t!(note = "A running job with no phase", "Working"),
    }
}

/// Whether a phase is worth saying beside the kind (not "Install ·
/// Installing").
pub fn phase_differs(job: &JobSnapshot) -> bool {
    !matches!(
        (job.kind, job.phase),
        (_, None)
            | (JobKind::Install, Some(JobPhase::Install))
            | (JobKind::Move, Some(JobPhase::Move))
    )
}

pub fn status(status: JobStatus) -> String {
    match status {
        JobStatus::Queued => t!(note = "A background job's status", "Queued"),
        JobStatus::Downloading | JobStatus::Installing => {
            t!(note = "A background job's status", "In progress")
        }
        JobStatus::NeedsUser => t!(note = "A background job's status", "Needs your attention"),
        JobStatus::Paused => t!(
            note = "A download the user paused; it goes on from where it stopped when resumed",
            "Paused"
        ),
        JobStatus::Completed => t!(note = "A background job's status", "Completed"),
        JobStatus::Failed => t!(note = "A background job's status", "Failed"),
        JobStatus::Cancelled => tp!("job status", "Cancelled"),
    }
}

/// The webview's status names, for `data-status`.
pub fn status_name(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => "queued",
        JobStatus::Downloading | JobStatus::Installing => "running",
        JobStatus::NeedsUser => "needs_user",
        JobStatus::Paused => "paused",
        JobStatus::Completed => "completed",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
    }
}

/// Whether `job` is under way: downloading or installing (the webview's
/// `running`).
pub fn is_running(job: &JobSnapshot) -> bool {
    matches!(job.status, JobStatus::Downloading | JobStatus::Installing)
}

/// Running, queued, paused or waiting for the user: it can be cancelled,
/// and it counts on the Activity button (`api/install.ts`'s
/// `isActiveJob`).
pub fn is_active(job: &JobSnapshot) -> bool {
    is_running(job)
        || matches!(
            job.status,
            JobStatus::Queued | JobStatus::NeedsUser | JobStatus::Paused
        )
}

/// A download that can pause now (#131), or a paused one that can resume.
pub fn can_pause(job: &JobSnapshot) -> bool {
    job.can_pause()
}

pub fn can_resume(job: &JobSnapshot) -> bool {
    job.status == JobStatus::Paused
}

/// A failed or cancelled job can be removed from Activity.
pub fn can_remove(job: &JobSnapshot) -> bool {
    matches!(job.status, JobStatus::Failed | JobStatus::Cancelled)
}

/// A failed or cancelled install, update or copy from another device can
/// run again; a copy goes on from what arrived (#233).
pub fn can_retry(job: &JobSnapshot) -> bool {
    can_remove(job)
        && matches!(
            job.kind,
            JobKind::Install | JobKind::Update | JobKind::PeerCopy
        )
}

fn strategy_name(strategy: Strategy) -> String {
    match strategy {
        Strategy::Copy => t!(
            note = "A way to install a game, offered when there is more than one",
            "Copy the files"
        ),
        Strategy::Extract => t!(
            note = "A way to install a game, offered when there is more than one",
            "Unpack the files"
        ),
        Strategy::RunSilent => t!(
            note = "A way to install a game, offered when there is more than one",
            "Run the installer silently"
        ),
        Strategy::RunInteractive => t!(
            note = "A way to install a game, offered when there is more than one",
            "Run the installer"
        ),
        Strategy::Delegate => t!(
            note = "A way to install a game, offered when there is more than one",
            "Let its package manager install it"
        ),
        Strategy::HandOff => t!(
            note = "A way to install a game, offered when there is more than one",
            "Hand it to another program"
        ),
    }
}

fn fidelity_name(fidelity: Fidelity) -> String {
    match fidelity {
        Fidelity::Exact => t!(
            note = "How close an install way comes to the installer's own result",
            "exact result"
        ),
        Fidelity::Equivalent => t!(
            note = "How close an install way comes to the installer's own result",
            "equivalent result"
        ),
        Fidelity::Partial => t!(
            note = "How close an install way comes to the installer's own result",
            "partial result"
        ),
        Fidelity::Native => t!(
            note = "How close an install way comes to the installer's own result",
            "the installer's own result"
        ),
    }
}

/// An install way: how, and by which handler.
pub fn candidate_label(candidate: &Candidate) -> String {
    format!(
        "{} · {}",
        strategy_name(candidate.strategy),
        candidate.handler.0
    )
}

/// What an install way means: confidence, result, whether it runs code.
pub fn candidate_reason(candidate: &Candidate) -> String {
    let confidence = t!(
        "{percent}% confidence",
        percent = (f64::from(candidate.confidence) * 100.0).round() as i64
    );
    let code = if candidate.requires.executes_code {
        t!("Runs installer code")
    } else {
        t!("Does not run installer code")
    };
    let elevation = if candidate.requires.may_prompt_elevation {
        format!(" · {}", t!("May request elevation"))
    } else {
        String::new()
    };
    format!(
        "{confidence} · {} · {code}{elevation}",
        fidelity_name(candidate.fidelity)
    )
}

/// The install ways a job weighed, and which it took.
pub fn strategies(job: &JobSnapshot) -> Vec<(u32, String, String, bool)> {
    let candidates: &[Candidate] = if !job.candidates.is_empty() {
        &job.candidates
    } else {
        match &job.question {
            Some(
                Question::LowConfidence { candidates } | Question::ConfirmExecution { candidates },
            ) => candidates,
            _ => &[],
        }
    };
    candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            (
                index as u32,
                candidate_label(candidate),
                candidate_reason(candidate),
                job.selected_candidate_index == Some(index as u32),
            )
        })
        .collect()
}

/// One answer to a job's question.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub index: u32,
    pub label: String,
    pub detail: Option<String>,
}

/// A job's question, in words.
#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    pub title: String,
    pub message: String,
    pub choices: Vec<Choice>,
    /// Some questions only inform: the answer is a new install, started
    /// elsewhere.
    pub can_answer: bool,
}

fn choices(labels: impl IntoIterator<Item = (String, Option<String>)>) -> Vec<Choice> {
    labels
        .into_iter()
        .enumerate()
        .map(|(index, (label, detail))| Choice {
            index: index as u32,
            label,
            detail,
        })
        .collect()
}

pub fn question(question: &Question) -> Asked {
    let asked = |title: String, message: String, choices: Vec<Choice>| Asked {
        title,
        message,
        choices,
        can_answer: true,
    };
    match question {
        Question::ChooseLaunchTarget { targets } => asked(
            t!("Choose a launch target"),
            t!("Several possible launch targets were found. Choose the game you want to launch."),
            choices(targets.iter().map(|target| {
                let detail = if target.args.is_empty() {
                    target.reason.clone()
                } else {
                    t!(
                        "{reason} · Arguments: {args}",
                        reason = target.reason.clone(),
                        args = serde_json::to_string(&target.args).unwrap_or_default()
                    )
                };
                (target.path.to_string_lossy().into_owned(), Some(detail))
            })),
        ),
        Question::ChooseInstaller { artifacts } => asked(
            t!("Choose an installer"),
            t!("Several installers were found in this input."),
            choices(artifacts.iter().map(|artifact| (artifact.0.clone(), None))),
        ),
        Question::ChooseAsset { assets } => {
            // A torrent's payloads (#159) are magnet links: the link says
            // nothing the name and size don't.
            let torrent = assets.iter().any(|asset| asset.url.scheme() == "magnet");
            let size = |bytes: Option<u64>| {
                bytes
                    .map(|bytes| format::size(Some(bytes)))
                    .unwrap_or_default()
            };
            asked(
                if torrent {
                    t!("Choose what to download")
                } else {
                    t!("Choose a download asset")
                },
                if torrent {
                    t!(
                        "This torrent holds more than one game or installer. Choose the one to download and install."
                    )
                } else {
                    t!("Choose the release file to download and install.")
                },
                choices(assets.iter().map(|asset| {
                    let detail = if torrent {
                        size(asset.size)
                    } else {
                        match asset.size {
                            Some(_) => format!("{} · {}", asset.url, size(asset.size)),
                            None => asset.url.to_string(),
                        }
                    };
                    (asset.name.clone(), Some(detail))
                })),
            )
        }
        Question::ChooseInstallDirectory { paths, reason } => Asked {
            can_answer: false,
            ..asked(
                t!("Choose the installed folder"),
                t!(
                    "{reason} This folder requires explicit adoption or a new installation. It will not be moved automatically.",
                    reason = reason.clone()
                ),
                choices(
                    paths
                        .iter()
                        .map(|path| (path.to_string_lossy().into_owned(), None)),
                ),
            )
        },
        Question::ReinstallAfterMove { title, reason, .. } => Asked {
            can_answer: false,
            ..asked(
                t!("Reinstall {title}?", title = title.clone()),
                t!(
                    "{reason} Start a new installation from the retained source after reviewing the moved game.",
                    reason = reason.clone()
                ),
                choices([(t!("Reinstall from the retained source"), None)]),
            )
        },
        Question::ChooseDestination { reason, options } => asked(
            t!("Where should these ROMs go?"),
            reason.clone(),
            choices(options.iter().map(|option| {
                (
                    option.label.clone(),
                    Some(
                        option
                            .folder
                            .as_ref()
                            .map(|folder| folder.to_string_lossy().into_owned())
                            .unwrap_or_else(|| t!("Imported from where they are now")),
                    ),
                )
            })),
        ),
        Question::LowConfidence { candidates } => asked(
            t!("Choose an installation strategy"),
            t!(
                "The installer could not confidently select a strategy. Review its requirements before continuing."
            ),
            choices(candidates.iter().map(|candidate| {
                (
                    candidate_label(candidate),
                    Some(candidate_reason(candidate)),
                )
            })),
        ),
        Question::ConfirmExecution { candidates } => asked(
            t!("Run the installer program?"),
            t!(
                "Installing this game means running its installer program on this computer, with your account's access to your files. Run it only if you trust where it came from, or choose a way that doesn't run it."
            ),
            choices(candidates.iter().map(|candidate| {
                (
                    candidate_label(candidate),
                    Some(candidate_reason(candidate)),
                )
            })),
        ),
    }
}
