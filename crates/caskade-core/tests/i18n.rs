//! The core domain's template (#212): `locales/core.pot` matches the
//! `tr!` calls in the Rust crates and the native app, and extraction is
//! stable.

use caskade_i18n::extract::CORE;
use std::path::PathBuf;

fn root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.canonicalize().unwrap_or(root)
}

#[test]
fn the_committed_template_is_current() {
    let outputs = CORE.outputs(&root()).unwrap();
    let template = &outputs[0];
    assert_eq!(template.path, CORE.template_path(&root()));
    assert!(
        template.is_current(),
        "locales/core.pot is out of date: run `just i18n` and commit it"
    );
}

#[test]
fn extracting_twice_changes_nothing() {
    let first = CORE.outputs(&root()).unwrap();
    let second = CORE.outputs(&root()).unwrap();
    assert_eq!(first, second);
    // Merging a merged catalog again gives the same file too.
    let template = CORE.template(&root()).unwrap();
    for output in &first[1..] {
        let merged = caskade_i18n::po::Po::parse(&output.contents).unwrap();
        let again = caskade_i18n::extract::merge(&template, &merged).unwrap();
        assert_eq!(again.write(), output.contents, "{}", output.path.display());
    }
}

#[test]
fn the_template_lists_the_tray_menu() {
    let template = CORE.template(&root()).unwrap();
    for id in ["Show Caskade", "Quit Caskade"] {
        let message = template
            .messages
            .iter()
            .find(|message| message.id == id)
            .unwrap_or_else(|| panic!("{id:?} isn't extracted"));
        assert_eq!(message.references, ["crates/caskade-app/src/tray.rs"]);
    }
}
