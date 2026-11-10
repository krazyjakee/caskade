//! A domain's catalogs and its active language.

use crate::catalog::{Catalog, CatalogError};
use crate::locale::{self, SOURCE};
use crate::plural::PluralForms;
use std::borrow::Cow;
use std::sync::{Arc, OnceLock, RwLock};

/// One domain's bundled catalogs and the language lookups use. Thread-safe:
/// the language can change while other threads look messages up. A
/// catalog is parsed the first time its language is chosen.
#[derive(Debug)]
pub struct Translations {
    bundled: Vec<Bundled>,
    active: RwLock<Active>,
}

#[derive(Debug)]
struct Bundled {
    locale: String,
    source: Cow<'static, str>,
    catalog: OnceLock<Result<Arc<Catalog>, CatalogError>>,
}

impl Bundled {
    fn catalog(&self) -> Result<Arc<Catalog>, CatalogError> {
        self.catalog
            .get_or_init(|| Catalog::parse(&self.source).map(Arc::new))
            .clone()
    }
}

#[derive(Debug)]
struct Active {
    locale: String,
    catalog: Option<Arc<Catalog>>,
}

impl Translations {
    /// A domain with these catalogs: (BCP 47 tag, PO text). It starts in
    /// English.
    pub fn new<I, L, S>(bundled: I) -> Self
    where
        I: IntoIterator<Item = (L, S)>,
        L: AsRef<str>,
        S: Into<Cow<'static, str>>,
    {
        let bundled = bundled
            .into_iter()
            .filter_map(|(tag, source)| {
                let Some(locale) = locale::normalize(tag.as_ref()) else {
                    tracing::warn!(tag = tag.as_ref(), "a catalog for no language");
                    return None;
                };
                Some(Bundled {
                    locale,
                    source: source.into(),
                    catalog: OnceLock::new(),
                })
            })
            .collect();
        Self {
            bundled,
            active: RwLock::new(Active {
                locale: SOURCE.to_owned(),
                catalog: None,
            }),
        }
    }

    /// English, then the languages with a catalog.
    pub fn available(&self) -> Vec<&str> {
        std::iter::once(SOURCE)
            .chain(self.bundled.iter().map(|bundled| bundled.locale.as_str()))
            .collect()
    }

    /// Switches to the first of `requested` (BCP 47 or POSIX tags, most
    /// preferred first) there is a catalog for, through each one's
    /// fallbacks (`pt-BR`, then `pt`), else English. A catalog that can't
    /// be read is skipped. Returns the language now in use.
    pub fn set_locale<I, S>(&self, requested: I) -> String
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let requested: Vec<S> = requested.into_iter().collect();
        let mut broken: Vec<&str> = Vec::new();
        let (locale, catalog) = loop {
            let candidates: Vec<&str> = self
                .available()
                .into_iter()
                .filter(|locale| !broken.contains(locale))
                .collect();
            let Some(locale) = locale::negotiate(&requested, &candidates) else {
                break (SOURCE.to_owned(), None);
            };
            let Some(bundled) = self.bundled.iter().find(|bundled| bundled.locale == locale) else {
                break (SOURCE.to_owned(), None);
            };
            match bundled.catalog() {
                Ok(catalog) => break (locale.to_owned(), Some(catalog)),
                Err(error) => {
                    tracing::warn!(locale, %error, "a translation catalog couldn't be read");
                    broken.push(locale);
                }
            }
        };
        let mut active = self
            .active
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active.locale.clone_from(&locale);
        active.catalog = catalog;
        locale
    }

    /// The language in use.
    pub fn locale(&self) -> String {
        self.read(|active| active.locale.clone())
    }

    /// `id` in the language in use, or `id` itself.
    pub fn gettext(&self, context: Option<&str>, id: &str) -> String {
        self.read(|active| {
            active
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.get(context, id))
                .unwrap_or(id)
                .to_owned()
        })
    }

    /// The form of `id` / `plural` for `n` in the language in use, or the
    /// English one.
    pub fn ngettext(&self, context: Option<&str>, id: &str, plural: &str, n: u64) -> String {
        self.read(|active| {
            active
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.get_plural(context, id, n))
                .unwrap_or_else(|| english(id, plural, n))
                .to_owned()
        })
    }

    /// `id`'s translation in the language in use, if it has one (not
    /// English's, nor an empty or fuzzy entry's): for looking through
    /// several domains.
    pub fn find(&self, context: Option<&str>, id: &str) -> Option<String> {
        self.read(|active| {
            active
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.get(context, id))
                .map(str::to_owned)
        })
    }

    /// The translated form of `id` / its plural for `n`, if the language in
    /// use has one.
    pub fn find_plural(&self, context: Option<&str>, id: &str, n: u64) -> Option<String> {
        self.read(|active| {
            active
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.get_plural(context, id, n))
                .map(str::to_owned)
        })
    }

    /// Reads every catalog: the languages whose catalog is broken, and why.
    pub fn check(&self) -> Vec<(&str, CatalogError)> {
        self.bundled
            .iter()
            .filter_map(|bundled| {
                bundled
                    .catalog()
                    .err()
                    .map(|error| (bundled.locale.as_str(), error))
            })
            .collect()
    }

    fn read<T>(&self, f: impl FnOnce(&Active) -> T) -> T {
        f(&self
            .active
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

/// English's form of `id` / `plural` for `n`.
pub fn english<'a>(id: &'a str, plural: &'a str, n: u64) -> &'a str {
    static ENGLISH: OnceLock<PluralForms> = OnceLock::new();
    if ENGLISH.get_or_init(PluralForms::default).index(n) == 0 {
        id
    } else {
        plural
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Show Caskade"
msgstr "Visa Caskade"

msgctxt "folder browser place"
msgid "Home"
msgstr "Hem"

#, fuzzy
msgid "Quit Caskade"
msgstr "Avsluta Caskade"

msgid "One game"
msgid_plural "{n} games"
msgstr[0] "Ett spel"
msgstr[1] "{n} spel"
"#;

    const PT: &str = r#"msgid ""
msgstr ""
"Plural-Forms: nplurals=2; plural=(n > 1);\n"

msgid "Show Caskade"
msgstr "Mostrar o Caskade"

msgid "One game"
msgid_plural "{n} games"
msgstr[0] "Um jogo"
msgstr[1] "{n} jogos"
"#;

    fn translations() -> Translations {
        Translations::new([("sv", SV), ("pt", PT), ("de", "msgid \"broken")])
    }

    #[test]
    fn starts_in_english() {
        let translations = translations();
        assert_eq!(translations.locale(), "en");
        assert_eq!(translations.gettext(None, "Show Caskade"), "Show Caskade");
        assert_eq!(
            translations.ngettext(None, "One game", "{n} games", 1),
            "One game"
        );
        assert_eq!(
            translations.ngettext(None, "One game", "{n} games", 0),
            "{n} games"
        );
    }

    #[test]
    fn switches_language_while_running() {
        let translations = translations();
        assert_eq!(translations.set_locale(["sv_SE.UTF-8"]), "sv");
        assert_eq!(translations.gettext(None, "Show Caskade"), "Visa Caskade");
        assert_eq!(translations.set_locale(["pt-BR"]), "pt");
        assert_eq!(
            translations.gettext(None, "Show Caskade"),
            "Mostrar o Caskade"
        );
        assert_eq!(translations.set_locale(["en-US"]), "en");
        assert_eq!(translations.gettext(None, "Show Caskade"), "Show Caskade");
    }

    #[test]
    fn missing_fuzzy_and_unknown_languages_show_english() {
        let translations = translations();
        translations.set_locale(["sv"]);
        assert_eq!(translations.gettext(None, "Quit Caskade"), "Quit Caskade");
        assert_eq!(
            translations.gettext(None, "Not translated"),
            "Not translated"
        );
        assert_eq!(translations.gettext(None, "Home"), "Home");
        assert_eq!(translations.set_locale(["fr", "ja"]), "en");
        assert_eq!(translations.gettext(None, "Show Caskade"), "Show Caskade");
    }

    #[test]
    fn context_and_plurals_use_the_catalog() {
        let translations = translations();
        translations.set_locale(["sv"]);
        assert_eq!(
            translations.gettext(Some("folder browser place"), "Home"),
            "Hem"
        );
        assert_eq!(
            translations.ngettext(None, "One game", "{n} games", 1),
            "Ett spel"
        );
        assert_eq!(
            translations.ngettext(None, "One game", "{n} games", 0),
            "{n} spel"
        );
        // Portuguese counts zero as one.
        translations.set_locale(["pt"]);
        assert_eq!(
            translations.ngettext(None, "One game", "{n} games", 0),
            "Um jogo"
        );
    }

    #[test]
    fn a_broken_catalog_falls_through_to_the_next_language() {
        let translations = translations();
        assert_eq!(translations.set_locale(["de", "sv"]), "sv");
        assert_eq!(translations.set_locale(["de"]), "en");
        let broken = translations.check();
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].0, "de");
    }

    #[test]
    fn lookups_and_switches_from_many_threads() {
        let translations = Arc::new(translations());
        let threads: Vec<_> = (0..8)
            .map(|index| {
                let translations = Arc::clone(&translations);
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        translations.set_locale([if index % 2 == 0 { "sv" } else { "en" }]);
                        let text = translations.gettext(None, "Show Caskade");
                        assert!(text == "Visa Caskade" || text == "Show Caskade", "{text}");
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
    }
}
