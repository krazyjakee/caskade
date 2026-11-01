//! Lists every `locales/<lang>/core.po` for [`bundled::CORE`], so adding a
//! language's catalog is all it takes to compile it in. A new or removed
//! folder under `locales/` reruns this.

use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let locales = Path::new(&manifest).join("../../locales");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", locales.display());

    let mut catalogs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&locales) {
        for entry in entries.flatten() {
            let po = entry.path().join("core.po");
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
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    std::fs::write(Path::new(&out).join("core_catalogs.rs"), code).expect("write core_catalogs.rs");
}
