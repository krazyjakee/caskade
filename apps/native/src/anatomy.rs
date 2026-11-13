//! The anatomy catalogue (#90, #139): one entry per themeable root,
//! declared beside its component and listed here. It is the theming
//! contract's single source: `caskade_ui_theme::anatomy` writes
//! `docs/theming/anatomy.md` and `crates/caskade-theme/anatomy.json` (the
//! contract `caskade-theme` checks themes against) from it. The tests check
//! both are current, that the catalogue holds together and that every
//! `data-cs` the native sources render is catalogued;
//! `CASKADE_WRITE_ANATOMY=1 cargo test -p caskade-native anatomy` writes
//! them.

use caskade_ui_theme::anatomy::{Entry, Source};

/// Every catalogued root. The RSX components of #93 add theirs here.
// Read by the tests, which write the docs and the contract from it.
#[cfg_attr(not(test), allow(dead_code))]
pub fn catalogue() -> Vec<Entry> {
    let mut all = vec![WEBSITE];
    all.extend_from_slice(crate::ui::ANATOMY);
    all.push(crate::settings::ANATOMY);
    all.extend_from_slice(crate::settings::anatomy::PANELS);
    all.extend_from_slice(crate::accounts::ANATOMY);
    all.extend_from_slice(crate::onboarding::ANATOMY);
    all.extend_from_slice(&crate::settings::addons::ANATOMY);
    all.extend_from_slice(&crate::activity::ANATOMY);
    all.extend_from_slice(&crate::plugins::ANATOMY);
    all.extend_from_slice(crate::frame::ANATOMY);
    all.extend_from_slice(crate::flows::ANATOMY);
    all.extend_from_slice(crate::game_menu::ANATOMY);
    all.extend_from_slice(crate::library::ANATOMY);
    all.extend_from_slice(crate::game::ANATOMY);
    all.extend_from_slice(crate::home::ANATOMY);
    all.push(crate::store::STORE_LINKS);
    all.extend_from_slice(crate::collections::ANATOMY);
    all.extend([OSK, GUIDE_MENU]);
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

/// The quick menu a pad's guide button opens (`input/guide.rs`).
const GUIDE_MENU: Entry = Entry::new(
    "guide-menu",
    "GuideMenu (apps/native/src/input/guide.rs)",
    Source::App,
    "The quick menu a pad's guide button opens over everything while a game runs: the game's title, Return to game, Stop game (asked once more) and Game details. For a game streamed from another device, End stream stops it there.",
);

/// The public website's page (`apps/website`), which shares the app's
/// tokens and branding: its `check.py` holds the page to these parts.
const WEBSITE: Entry = Entry {
    parts: &[
        ("skip", "Keyboard skip link to the main content."),
        (
            "header",
            "Brand, primary navigation, language dropdown and download link.",
        ),
        ("main-nav", "The main page navigation links."),
        ("header-actions", "The language dropdown and download link."),
        (
            "language-trigger",
            "Native popover button with the current language flag and name.",
        ),
        (
            "language-options",
            "Language links with flags; aria-current marks the current page.",
        ),
        ("brand", "Caskade's mark and name."),
        (
            "button",
            "A link styled as a button; data-variant is primary or small when set.",
        ),
        ("hero", "Introduction and desktop and console screenshots."),
        ("hero-copy", "The main heading, introduction and actions."),
        ("eyebrow", "A short section introduction."),
        ("status-dot", "A decorative accent beside the introduction."),
        ("intro", "The landing page introduction."),
        (
            "actions",
            "Download and source link groups, including platform installer choices.",
        ),
        ("text-link", "A text link to a project resource."),
        ("hero-note", "Free software and privacy summary."),
        (
            "screenshots",
            "Actual app captures with a sample library, linking to the full images.",
        ),
        (
            "screenshot-main",
            "The cropped desktop screenshot and its link to the full capture.",
        ),
        (
            "screenshot-console",
            "The cropped console screenshot and its link to the full capture.",
        ),
        ("screenshot-label", "A mode label below a screenshot."),
        ("sources", "Examples of supported library sources."),
        (
            "video",
            "The introductory video and its direct YouTube link.",
        ),
        (
            "video-player",
            "A responsive, lazy-loaded YouTube privacy-enhanced embed.",
        ),
        (
            "features",
            "The library, household and emulation feature overview.",
        ),
        ("section-heading", "A section's introduction and heading."),
        ("feature-grid", "The three feature descriptions."),
        (
            "feature",
            "One feature; data-feature is library, household or retro.",
        ),
        ("feature-number", "A decorative feature index."),
        (
            "principles",
            "Local ownership, offline use, privacy and customisation.",
        ),
        ("principle-list", "The three product principles."),
        ("downloads", "Platform status and download links."),
        ("download-grid", "The three platform download panels."),
        (
            "platform",
            "A platform's status, formats and direct installer links.",
        ),
        (
            "platform-title",
            "Platform name, icon and optional status badge.",
        ),
        ("badge", "The Linux reference-platform label."),
        (
            "release-note",
            "Download instructions and the release archive link.",
        ),
        ("faq", "Native details disclosures with common questions."),
        ("closing", "The final download invitation."),
        ("footer", "Project documentation, source and licence links."),
    ],
    ..Entry::new(
        "website",
        "apps/website/index.html (static GitHub Pages site)",
        Source::Website,
        "The public landing page, sharing the app's tokens and branding.",
    )
};

#[cfg(test)]
mod tests {
    use super::*;
    use caskade_ui_theme::anatomy::{contract, markdown};
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Writes `path` (from the repository's root) with `text` under
    /// `CASKADE_WRITE_ANATOMY`, else fails when it isn't `text`.
    fn current(path: &str, text: &str) {
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path);
        if std::env::var_os("CASKADE_WRITE_ANATOMY").is_some() {
            std::fs::write(&file, text).unwrap();
            return;
        }
        let on_disk = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(
            on_disk == text,
            "{path} is stale: CASKADE_WRITE_ANATOMY=1 cargo test -p caskade-native anatomy"
        );
    }

    /// `docs/theming/anatomy.md` is the catalogue's.
    #[test]
    fn the_anatomy_docs_are_the_catalogues() {
        current("docs/theming/anatomy.md", &markdown(&catalogue()));
    }

    /// The contract `caskade-theme` checks themes against is the
    /// catalogue's.
    #[test]
    fn the_theme_contract_is_the_catalogues() {
        current("crates/caskade-theme/anatomy.json", &contract(&catalogue()));
    }

    /// Each root is catalogued once, with its parts and state attributes
    /// once each, and contains only catalogued roots.
    #[test]
    fn the_catalogue_holds_together() {
        let all = catalogue();
        let mut names = BTreeSet::new();
        let mut problems = Vec::new();
        for entry in &all {
            if !names.insert(entry.name) {
                problems.push(format!("{} is catalogued twice", entry.name));
            }
            let mut parts = BTreeSet::new();
            for (part, _) in entry.parts {
                if !parts.insert(*part) {
                    problems.push(format!("{} lists the part {part} twice", entry.name));
                }
            }
            let mut states = BTreeSet::new();
            for (attribute, values) in entry.states {
                if !states.insert(*attribute) || values.is_empty() {
                    problems.push(format!(
                        "{}'s {attribute} is listed twice or has no values",
                        entry.name
                    ));
                }
            }
        }
        for entry in &all {
            for inner in entry.contains {
                if !names.contains(inner) {
                    problems.push(format!(
                        "{} contains {inner}, which isn't catalogued",
                        entry.name
                    ));
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
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
        let all = catalogue();
        let known = |name: &str| all.iter().any(|entry| entry.name == name);
        let missing: Vec<_> = found.iter().filter(|(name, _)| !known(name)).collect();
        assert!(missing.is_empty(), "not catalogued: {missing:?}");
        assert!(found.iter().any(|(name, _)| name == "osk"), "{found:?}");
    }
}
