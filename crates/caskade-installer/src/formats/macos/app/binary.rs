use super::{AppError, MAX_PLIST_BYTES, Result};
use crate::plist_guard::{self, GuardError, Limits};

const LIMITS: Limits = Limits {
    max_expanded_bytes: MAX_PLIST_BYTES,
    max_string_bytes: MAX_PLIST_BYTES,
    max_nodes: 100_000,
    max_depth: 64,
};

/// Bound both depth and expansion of a binary plist DAG before `plist::Value`'s
/// recursive tree builder is called. Offsets and references stay in the input.
pub(super) fn validate(data: &[u8]) -> Result<()> {
    plist_guard::validate(data, &LIMITS).map_err(|error| match error {
        GuardError::Invalid => AppError::Invalid(error.to_string()),
        GuardError::Limit { .. } => AppError::MetadataLimit,
    })
}
