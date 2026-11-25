//! Binary plist DAG guard delegating to `caskade_installer::plist_guard`.
use super::{Limits, ParseError, Result};
use caskade_installer::plist_guard::{self, GuardError};

pub(super) fn validate(data: &[u8], limits: &Limits) -> Result<()> {
    let guard = plist_guard::Limits {
        max_expanded_bytes: limits.max_input_bytes,
        max_string_bytes: limits.max_string_bytes,
        max_nodes: limits.max_nodes,
        max_depth: limits.max_depth,
    };
    plist_guard::validate(data, &guard).map_err(|error| match error {
        GuardError::Invalid => ParseError::Invalid(error.to_string()),
        GuardError::Limit { budget, maximum } => ParseError::Limit { budget, maximum },
    })
}
