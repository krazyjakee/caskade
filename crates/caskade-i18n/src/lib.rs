//! Gettext catalogs for Caskade's Rust side, in pure Rust: no libintl, so
//! the bundles and the Windows and macOS cross builds need no C gettext.
//! See `docs/translating.md`.
//!
//! - [`po`] reads and writes PO and POT files.
//! - [`Catalog`] is one language's translations: lookups by context, plural
//!   forms from the `Plural-Forms` header, fuzzy and empty entries left out
//!   so their English shows.
//! - [`Translations`] is a domain's bundled catalogs and its active
//!   language, switched at run time from any thread.
//! - [`locale`] normalizes BCP 47 and POSIX tags, reads the OS's languages
//!   and negotiates one (`pt-BR`, then `pt`, then English).
//! - [`format()`] fills `{name}` placeholders after the lookup.
//! - [`bundled`] holds every `locales/<lang>/core.po`, listed at build time.
//! - `extract` (feature `extract`) writes a domain's POT template from its
//!   `tr!` calls and merges it into the catalogs: `just i18n`.
//!
//! The `tr!` macros themselves belong to each domain; the core's are in
//! `caskade_core::i18n`.

pub mod catalog;
#[cfg(feature = "extract")]
pub mod extract;
pub mod format;
pub mod locale;
pub mod plural;
pub mod po;
pub mod translations;

pub use catalog::{Catalog, CatalogError};
pub use format::format;
pub use translations::Translations;

/// The catalogs in the repository's `locales/`, compiled in so a release
/// needs no locale files on disk.
pub mod bundled {
    /// The core domain's catalogs: (BCP 47 tag, PO text) for every
    /// `locales/<lang>/core.po`, found by the build script, so adding a
    /// language's catalog ships it. For [`Translations::new`](crate::Translations::new).
    pub const CORE: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/core_catalogs.rs"));
}

/// Problems in a catalog that reads but would show wrong text: a
/// translation with a placeholder its English doesn't have (it would show
/// as typed), or a plural message without one translation per form.
pub fn lint(text: &str) -> Result<Vec<String>, CatalogError> {
    let po = po::Po::parse(text).map_err(CatalogError::Parse)?;
    let forms = po.plural_forms().map_err(CatalogError::Plural)?;
    let mut problems = Vec::new();
    for message in &po.messages {
        if message.is_fuzzy() {
            continue;
        }
        let mut source = format::placeholders(&message.id);
        if let Some(plural) = &message.plural {
            source.extend(format::placeholders(plural));
            if message.translations.len() != forms.count
                && message.translations.iter().any(|text| !text.is_empty())
            {
                problems.push(format!(
                    "{:?}: {} plural forms, the language has {}",
                    message.id,
                    message.translations.len(),
                    forms.count
                ));
            }
        }
        for translation in &message.translations {
            let unknown: Vec<&str> = format::placeholders(translation)
                .difference(&source)
                .copied()
                .collect();
            if !unknown.is_empty() {
                problems.push(format!(
                    "{:?}: the translation has placeholders the English doesn't: {}",
                    message.id,
                    unknown.join(", ")
                ));
            }
        }
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_finds_unknown_placeholders_and_missing_forms() {
        let problems = lint(
            r#"msgid ""
msgstr "Plural-Forms: nplurals=3; plural=(n==1 ? 0 : n==2 ? 1 : 2);\n"

msgid "Installed {game}"
msgstr "Installerade {spel}"

msgid "One game"
msgid_plural "{n} games"
msgstr[0] "Ett spel"
msgstr[1] "{n} spel"

msgid "Fine {game}"
msgstr "Bra {game}"

#, fuzzy
msgid "Ignored"
msgstr "{whatever}"
"#,
        )
        .unwrap();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].contains("spel"), "{problems:?}");
        assert!(problems[1].contains("plural forms"));
    }

    #[test]
    fn every_core_catalog_in_locales_is_bundled() {
        let locales = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales");
        for entry in std::fs::read_dir(locales).unwrap() {
            let path = entry.unwrap().path();
            let po = path.join("core.po");
            if !po.is_file() {
                continue;
            }
            let tag = path.file_name().unwrap().to_string_lossy().into_owned();
            let bundled = bundled::CORE.iter().find(|(bundled, _)| *bundled == tag);
            assert_eq!(
                bundled.map(|(_, text)| *text),
                Some(std::fs::read_to_string(&po).unwrap().as_str()),
                "locales/{tag}/core.po"
            );
        }
    }
}
