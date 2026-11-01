//! Lookups over parsed KeyValues. Steam writes keys in inconsistent case
//! (`apps` / `Apps`), so every lookup ignores ASCII case.

use crate::{
    env::HostEnv,
    parsers::{Limits, binary_vdf::Value, text_vdf},
    types::SourceError,
};
use std::{path::Path, str::FromStr};

/// Manifests, library lists, login users.
pub(super) fn small() -> Limits {
    Limits {
        max_input_bytes: 4 << 20,
        max_string_bytes: 64 << 10,
        max_nodes: 100_000,
        max_depth: 32,
    }
}

/// `localconfig.vdf` and `config.vdf`, which grow with the account.
pub(super) fn large() -> Limits {
    Limits {
        max_input_bytes: 64 << 20,
        max_string_bytes: 1 << 20,
        max_nodes: 4_000_000,
        max_depth: 48,
    }
}

/// Reads and parses a text VDF; parser warnings are returned with it.
pub(super) fn load(
    env: &HostEnv,
    path: &Path,
    limits: &Limits,
) -> Result<(Value, Vec<String>), SourceError> {
    let bytes = super::super::read(env, path, limits.max_input_bytes as u64)
        .map_err(|error| SourceError::io(path, error))?;
    let parsed =
        text_vdf::parse(&bytes, limits).map_err(|error| SourceError::parse(path, error))?;
    Ok((parsed.value, parsed.warnings))
}

pub(super) fn entries(value: &Value) -> &[(String, Value)] {
    match value {
        Value::Object(pairs) => pairs,
        _ => &[],
    }
}

pub(super) fn child<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    entries(value)
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

pub(super) fn path<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names
        .iter()
        .try_fold(value, |value, name| child(value, name))
}

pub(super) fn text<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    child(value, name)?.as_str()
}

pub(super) fn number<T: FromStr>(value: &Value, name: &str) -> Option<T> {
    text(value, name)?.trim().parse().ok()
}
