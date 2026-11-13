//! Lists every `locales/<lang>/app.po` and `ui.po` for the native UI, so a
//! language's catalogs compile in as soon as its folder has them. This
//! crate is the only one that embeds them: the core doesn't rebuild when
//! the UI's text changes.
//!
//! It also counts how much of each language is translated, as `just
//! i18n-check` does (`caskade_i18n::extract::completion`), for the
//! Language setting.

use std::collections::HashSet;
use std::path::Path;

use caskade_i18n::po::{Message, Po};

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let locales = Path::new(&manifest).join("../../locales");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", locales.display());
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    for domain in ["app", "ui"] {
        let mut catalogs = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&locales) {
            for entry in entries.flatten() {
                let po = entry.path().join(format!("{domain}.po"));
                if let (Some(tag), true) = (entry.file_name().to_str(), po.is_file()) {
                    let po = po.canonicalize().unwrap_or(po);
                    catalogs.push((tag.to_owned(), po.display().to_string()));
                }
            }
        }
        catalogs.sort();
        let mut code = String::from("&[\n");
        for (tag, po) in &catalogs {
            code.push_str(&format!("    ({tag:?}, include_str!({po:?})),\n"));
        }
        code.push(']');
        std::fs::write(Path::new(&out).join(format!("{domain}_catalogs.rs")), code)
            .unwrap_or_else(|error| panic!("write {domain}_catalogs.rs: {error}"));
    }
    let shares = completeness(&locales);
    let mut code = String::from("&[\n");
    for (tag, share) in shares {
        code.push_str(&format!("    ({tag:?}, {share:?}),\n"));
    }
    code.push(']');
    std::fs::write(Path::new(&out).join("completeness.rs"), code)
        .unwrap_or_else(|error| panic!("write completeness.rs: {error}"));
}

/// Every domain's messages, the core's too: the share covers all of
/// Caskade's text.
const DOMAINS: [&str; 3] = ["app", "ui", "core"];

fn read(path: &Path) -> Vec<Message> {
    println!("cargo:rerun-if-changed={}", path.display());
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    Po::parse(&text)
        .unwrap_or_else(|error| panic!("{}: {error:?}", path.display()))
        .messages
}

fn key(message: &Message) -> (Option<String>, String) {
    (message.context.clone(), message.id.clone())
}

/// Each language's translated messages over the templates', every domain
/// together, from 0 to 1. Fuzzy and obsolete entries, and translations of
/// messages no template has, don't count.
fn completeness(locales: &Path) -> Vec<(String, f64)> {
    let templates: Vec<HashSet<_>> = DOMAINS
        .iter()
        .map(|domain| {
            read(&locales.join(format!("{domain}.pot")))
                .iter()
                .map(key)
                .collect()
        })
        .collect();
    let mut tags: Vec<String> = std::fs::read_dir(locales)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            DOMAINS
                .iter()
                .any(|domain| entry.path().join(format!("{domain}.po")).is_file())
        })
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|tag| tag != "en-XA")
        .collect();
    tags.sort();
    let total: usize = templates.iter().map(HashSet::len).sum();
    tags.into_iter()
        .map(|tag| {
            let done: usize = DOMAINS
                .iter()
                .zip(&templates)
                .map(|(domain, template)| {
                    read(&locales.join(&tag).join(format!("{domain}.po")))
                        .iter()
                        .filter(|message| {
                            !message.is_fuzzy()
                                && !message.translations.is_empty()
                                && message.translations.iter().all(|form| !form.is_empty())
                                && template.contains(&key(message))
                        })
                        .count()
                })
                .sum();
            let share = if total == 0 {
                1.0
            } else {
                done as f64 / total as f64
            };
            (tag, share)
        })
        .collect()
}
