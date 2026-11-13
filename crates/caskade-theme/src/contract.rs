//! The theming contract a theme is checked against: the token manifest
//! (`packages/tokens/manifest.json`, #137) and the anatomy contract
//! (`anatomy.json`, #138), which the native UI's anatomy catalogue writes
//! (`CASKADE_WRITE_ANATOMY=1 cargo test -p caskade-native anatomy`).
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const TOKENS: &str = include_str!("../../../packages/tokens/manifest.json");
const ANATOMY: &str = include_str!("../anatomy.json");

#[derive(Deserialize)]
pub(crate) struct Token {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// The default value, aliases resolved.
    pub resolved: String,
}

#[derive(Deserialize)]
pub(crate) struct Root {
    pub parts: BTreeSet<String>,
}

#[derive(Deserialize)]
pub(crate) struct Anatomy {
    pub roots: BTreeMap<String, Root>,
    pub attributes: BTreeSet<String>,
}

pub(crate) struct Contract {
    /// By dotted path.
    pub tokens: BTreeMap<String, Token>,
    /// `--cs-*` names.
    pub names: BTreeSet<String>,
    pub anatomy: Anatomy,
}

pub(crate) fn contract() -> &'static Contract {
    static CONTRACT: OnceLock<Contract> = OnceLock::new();
    CONTRACT.get_or_init(|| {
        let tokens: Vec<Token> =
            serde_json::from_str(TOKENS).expect("packages/tokens/manifest.json is valid");
        let anatomy: Anatomy = serde_json::from_str(ANATOMY).expect("anatomy.json is valid");
        Contract {
            names: tokens.iter().map(|token| token.name.clone()).collect(),
            tokens: tokens
                .into_iter()
                .map(|token| (token.path.clone(), token))
                .collect(),
            anatomy,
        }
    })
}

impl Contract {
    pub fn is_part(&self, part: &str) -> bool {
        self.anatomy
            .roots
            .values()
            .any(|root| root.parts.contains(part))
    }
}
