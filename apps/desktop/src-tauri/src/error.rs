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
