use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use url::Url;

/// A user-selected local input or one or more URLs forming a single set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Input {
    Path(PathBuf),
    Urls(Vec<Url>),
}
