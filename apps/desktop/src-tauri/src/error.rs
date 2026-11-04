//! The commands' error (#160): the core's [`ErrorReport`], a stable code,
//! a sentence for people, redacted details and recovery hints, instead of a
//! bare string. Write `Result<T, CommandError>` and `.map_err(report)` in new
//! commands; `?` also takes a `caskade_core::Error` or a `String`.
//! See `docs/errors.md`.

pub use caskade_core::error::{ErrorCode, ErrorReport as CommandError, RecoveryHint};

/// `error` as a command's error, logged with its reference so the detail a
/// user reads out can be found in the log.
pub fn report<E: std::error::Error + 'static>(error: E) -> CommandError {
    logged(CommandError::from_error(&error))
}

/// A refusal in the user's own words, for an adapter's check.
pub fn refused(summary: String) -> CommandError {
    logged(CommandError::new(ErrorCode::Invalid, summary))
}

/// The state changed under the request (what it was about is gone, or the
/// work already runs): `summary` says so without naming ids, and reloading
/// shows what's there now.
pub fn conflict(summary: String) -> CommandError {
    logged(CommandError::new(ErrorCode::Conflict, summary))
}

/// A failure the UI can't cause: a plain summary, with `detail` (redacted)
/// behind Show details.
pub fn internal(detail: impl std::fmt::Display) -> CommandError {
    detailed(
        CommandError::new(
            ErrorCode::Internal,
            caskade_core::tr!("Caskade ran into an unexpected problem."),
        ),
        detail,
    )
}

/// `report` with `detail` (redacted), logged: for text that names ids or
/// types, which a summary mustn't.
pub fn detailed(mut report: CommandError, detail: impl std::fmt::Display) -> CommandError {
    report.detail = Some(caskade_core::error::sanitize(&detail.to_string()));
    logged(report)
}

/// Logs `report`: what the user asked wrongly at debug, failures at warn.
pub fn logged(report: CommandError) -> CommandError {
    let detail = report.detail.as_deref().unwrap_or(&report.summary);
    match report.code {
        ErrorCode::Invalid | ErrorCode::Forbidden | ErrorCode::Cancelled | ErrorCode::Conflict => {
            tracing::debug!(reference = %report.reference, code = ?report.code, %detail, "command refused");
        }
        _ => {
            tracing::warn!(reference = %report.reference, code = ?report.code, %detail, "command failed");
        }
    }
    report
}
