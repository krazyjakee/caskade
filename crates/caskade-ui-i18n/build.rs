//! Lists every `locales/<lang>/app.po` and `ui.po` for the native UI, so a
//! language's catalogs compile in as soon as its folder has them. This
//! crate is the only one that embeds them: the core doesn't rebuild when
//! the webview's text changes.

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
}
