//! The native UI's anatomy catalogue (#90): one entry per themeable root,
//! declared beside its component and listed here. Each must match the
//! webview's entry (`packages/ui/src/anatomy.ts`) word for word, apart
//! from the component it names; the tests check that, and that every
//! `data-cs` the native sources render is catalogued.
//!
//! `docs/theming/anatomy.md` is written from it (#93) by
//! `caskade_ui_theme::anatomy`: the native entry for each root the native
//! UI renders, the webview's (`packages/ui/anatomy-entries.json`) for the
//! rest until their views are ported. A test checks the docs are current;
//! `CASKADE_WRITE_ANATOMY=1 cargo test -p caskade-native anatomy` writes
//! them.

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root the native UI renders. The RSX components of #93
/// add theirs here.
// Read by the tests now, and by the docs at the cutover (BLITZ-33).
#[cfg_attr(not(test), allow(dead_code))]
pub fn catalogue() -> Vec<Entry> {
    let mut all = vec![OSK];
    all.extend_from_slice(crate::ui::ANATOMY);
    all.push(crate::settings::ANATOMY);
    all.extend_from_slice(crate::frame::ANATOMY);
    all.extend_from_slice(crate::flows::ANATOMY);
    all.extend_from_slice(crate::game_menu::ANATOMY);
    all.extend_from_slice(crate::library::ANATOMY);
    all
}

/// The on-screen keyboard (`input/keyboard.rs`, #92).
const OSK: Entry = Entry {
    parts: &[
        ("preview", "The field's text as it is typed."),
        ("row", "A row of keys."),
        (
            "key",
            "A key; `data-key` is its character or what it does (`shift`, `layer`, `backspace`, `space`, `left`, `right`, `done`), `data-focused` is the pad's place, `data-selected` an active shift.",
        ),
    ],
    states: &[
        ("data-layer", &["letters", "symbols"]),
        ("data-shift", &["off", "once", "lock"]),
    ],
    ..Entry::new(
        "osk",
        "the on-screen keyboard (apps/native/src/input/keyboard.rs)",
        Source::App,
        "The on-screen keyboard a pad types with; it types into the focused field, which keeps focus.",
    )
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Roots the native UI renders that the webview doesn't catalogue yet,
    /// with the issue that will.
    const NOT_YET: &[(&str, &str)] = &[
        (
            "guide-menu",
            "#93: the webview's GuideMenu isn't catalogued either",
        ),
        (
            "virtual-library",
            "#93: the webview's VirtualLibrary isn't catalogued either",
        ),
        // The virtual grid's (#94) minimal cards and rows; the full ones,
        // with the webview's parts, are #93's components.
        ("game-card", "#93: the full GameCard"),
        ("game-art", "#93: the full GameCard's art"),
        ("game-row", "#93: the full LibraryRow"),
        ("alphabet-bar", "#99: the Library view's AlphabetBar"),
        (
            "collection-picker",
            "#99: the webview's CollectionPicker isn't catalogued either",
        ),
        (
            "collection-dialog",
            "#99: the webview's CollectionDialog isn't catalogued either",
        ),
        (
            "add-web-game",
            "#99: the webview's AddWebGame isn't catalogued either",
        ),
        (
            "add-game",
            "#108: the webview's AddGame isn't catalogued either",
        ),
        (
            "install-root-picker",
            "#108: the webview's InstallRootPicker isn't catalogued either",
        ),
        (
            "install-url",
            "#108: the webview's install-from-link dialog isn't catalogued either",
        ),
        (
            "stream-pair-prompt",
            "#108: the webview's StreamPairingHost isn't catalogued either",
        ),
        (
            "pair-request",
            "#108: the webview's PairRequestForm isn't catalogued either",
        ),
        (
            "stop-host",
            "#108: the webview's StopHostDialog isn't catalogued either",
        ),
        (
            "schema-settings",
            "#103: the webview's SchemaSettings isn't catalogued either",
        ),
    ];

    /// The webview's catalogue, as `pnpm --filter @caskade/ui docs`
    /// writes it.
    fn webview() -> Vec<Entry> {
        caskade_ui_theme::anatomy::from_json(include_str!(
            "../../../packages/ui/anatomy-entries.json"
        ))
        .expect("the webview's catalogue")
    }

    #[test]
    fn every_entry_is_the_webviews() {
        let problems = caskade_ui_theme::anatomy::differences(
            &catalogue(),
            &caskade_ui_theme::anatomy::markdown(&webview()),
        );
        assert!(problems.is_empty(), "{}", problems.join("\n\n"));
    }

    /// `docs/theming/anatomy.md` is the native components' catalogue, with
    /// the webview's entries for the roots only it renders yet.
    #[test]
    fn the_anatomy_docs_are_the_native_catalogues() {
        use caskade_ui_theme::anatomy::{GENERATED, markdown_by, with_native};
        let docs =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/theming/anatomy.md");
        let text = markdown_by(&with_native(&webview(), &catalogue()), GENERATED);
        if std::env::var_os("CASKADE_WRITE_ANATOMY").is_some() {
            std::fs::write(&docs, &text).unwrap();
            return;
        }
        let current = std::fs::read_to_string(&docs).unwrap_or_default();
        assert!(
            current == text,
            "docs/theming/anatomy.md is stale: CASKADE_WRITE_ANATOMY=1 cargo test -p caskade-native anatomy"
        );
    }

    /// Every `data-cs` a component renders is catalogued: it's the
    /// theming API (smoke, test scaffolding and the screenshots' reference
    /// screens aside).
    #[test]
    fn every_rendered_root_is_catalogued() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![src];
        let mut found = Vec::new();
        while let Some(path) = files.pop() {
            if path.is_dir() {
                // The screenshot harness's reference screens: fixtures (#90).
                if path.ends_with("shots") {
                    continue;
                }
                files.extend(
                    std::fs::read_dir(&path)
                        .unwrap()
                        .map(|entry| entry.unwrap().path()),
                );
                continue;
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if !name.ends_with(".rs") || matches!(name, "smoke.rs" | "tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for (at, _) in text.match_indices("\"data-cs\":") {
                let rest = text[at + "\"data-cs\":".len()..].trim_start();
                if let Some(value) = rest
                    .strip_prefix('"')
                    .and_then(|rest| rest.split('"').next())
                {
                    found.push((value.to_owned(), path.display().to_string()));
                }
            }
        }
        let known = |name: &str| {
            catalogue().iter().any(|entry| entry.name == name)
                || NOT_YET.iter().any(|(root, _)| *root == name)
        };
        let missing: Vec<_> = found.iter().filter(|(name, _)| !known(name)).collect();
        assert!(missing.is_empty(), "not catalogued: {missing:?}");
        assert!(found.iter().any(|(name, _)| name == "osk"), "{found:?}");
    }
}
