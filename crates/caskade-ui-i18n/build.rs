//! Lists every `locales/<lang>/app.po` and `ui.po` for the native UI, so a
//! language's catalogs compile in as soon as its folder has them. This
//! crate is the only one that embeds them: the core doesn't rebuild when
//! the UI's text changes.
//!
//! It also counts how much of each language is translated, as `just
//! i18n-check` does (`caskade_i18n::completion::count`), for the
//! Language setting.

use std::path::Path;

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

/// Each language's translated messages over the templates', every domain
/// together, from 0 to 1. Fuzzy and obsolete entries, and translations of
/// messages no template has, don't count.
fn completeness(locales: &Path) -> Vec<(String, f64)> {
    let report = caskade_i18n::completion::count(locales, &DOMAINS)
        .unwrap_or_else(|error| panic!("{error}"));
    for path in &report.visited {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    report
        .languages
        .into_iter()
        .map(|(tag, domains)| {
            let done: usize = domains.iter().map(|domain| domain.translated).sum();
            let total: usize = domains.iter().map(|domain| domain.total).sum();
            let share = if total == 0 {
                1.0
            } else {
                done as f64 / total as f64
            };
            (tag, share)
        })
        .collect()
}
