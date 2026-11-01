//! Bounded, host-independent readers for launcher metadata.
//!
//! Paths and environment values are data. These readers never resolve registry
//! keys, expand environment variables, start programs, or follow VDF includes.
pub mod binary_vdf;
mod cursor;
pub mod desktop;
pub mod lnk;
pub mod plist;
pub mod protobuf;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod text_vdf;
pub mod url;

use serde::{Deserialize, Serialize};

/// Allocation and recursion budgets shared by all byte parsers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Limits {
    pub max_input_bytes: usize,
    pub max_string_bytes: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: 64 * 1024 * 1024,
            max_string_bytes: 1024 * 1024,
            max_nodes: 100_000,
            max_depth: 48,
        }
    }
}
impl Limits {
    pub(crate) fn input(&self, bytes: &[u8]) -> Result<()> {
        self.check(bytes.len(), self.max_input_bytes, "input bytes")
    }
    pub(crate) fn check(&self, actual: usize, maximum: usize, budget: &'static str) -> Result<()> {
        if actual > maximum {
            Err(ParseError::Limit { budget, maximum })
        } else {
            Ok(())
        }
    }
    pub(crate) fn text<'a>(&self, bytes: &'a [u8]) -> Result<&'a str> {
        self.input(bytes)?;
        if bytes.contains(&0) {
            return Err(ParseError::Invalid("NUL in text".into()));
        }
        std::str::from_utf8(bytes).map_err(|_| ParseError::Encoding)
    }
}
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("truncated input at byte {0}")]
    Truncated(usize),
    #[error("invalid metadata: {0}")]
    Invalid(String),
    #[error("unsupported format: {0}")]
    Unsupported(String),
    #[error("invalid character encoding")]
    Encoding,
    #[error("{budget} exceeds limit {maximum}")]
    Limit {
        budget: &'static str,
        maximum: usize,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[cfg(feature = "sqlite")]
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}
pub type Result<T> = std::result::Result<T, ParseError>;

/// Parsed values accompanied by safely ignored forward-compatible metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parsed<T> {
    pub value: T,
    pub warnings: Vec<String>,
}

mod plist_binary;
