//! A language's translations, ready for lookups.

use crate::plural::{PluralError, PluralForms};
use crate::po::{ParseError, Po};
use std::collections::HashMap;
use std::fmt;

/// Separates the context from the English text in a key, as in MO files.
const CONTEXT_SEPARATOR: char = '\u{4}';

/// The translated messages of one language and domain. Fuzzy and empty
/// translations are left out, so their English shows.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    messages: HashMap<String, Vec<String>>,
    plural_forms: PluralForms,
}

/// Why a catalog couldn't be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    Parse(ParseError),
    Plural(PluralError),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Parse(error) => write!(f, "{error}"),
            CatalogError::Plural(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CatalogError {}

impl Catalog {
    /// Loads PO text.
    pub fn parse(text: &str) -> Result<Self, CatalogError> {
        Self::from_po(&Po::parse(text).map_err(CatalogError::Parse)?)
    }

    /// Loads a parsed PO file.
    pub fn from_po(po: &Po) -> Result<Self, CatalogError> {
        let plural_forms = po.plural_forms().map_err(CatalogError::Plural)?;
        let messages = po
            .messages
            .iter()
            .filter(|message| {
                !message.is_fuzzy() && message.translations.iter().any(|text| !text.is_empty())
            })
            .map(|message| {
                (
                    key(message.context.as_deref(), &message.id),
                    message.translations.clone(),
                )
            })
            .collect();
        Ok(Self {
            messages,
            plural_forms,
        })
    }

    /// The translation of `id` in `context`, if there is one.
    pub fn get(&self, context: Option<&str>, id: &str) -> Option<&str> {
        let translations = match context {
            None => self.messages.get(id),
            Some(context) => self.messages.get(&key(Some(context), id)),
        }?;
        translations
            .first()
            .map(String::as_str)
            .filter(|text| !text.is_empty())
    }

    /// The translation of `id` / `plural` for `n`, if there is one.
    pub fn get_plural(&self, context: Option<&str>, id: &str, n: u64) -> Option<&str> {
        let translations = match context {
            None => self.messages.get(id),
            Some(context) => self.messages.get(&key(Some(context), id)),
        }?;
        translations
            .get(self.plural_forms.index(n))
            .map(String::as_str)
            .filter(|text| !text.is_empty())
    }

    /// How many messages have a translation.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

fn key(context: Option<&str>, id: &str) -> String {
    match context {
        None => id.to_owned(),
        Some(context) => format!("{context}{CONTEXT_SEPARATOR}{id}"),
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

msgid "Home"
msgstr "Start"

#, fuzzy
msgid "Quit Caskade"
msgstr "Avsluta Caskade"

msgid "File system"
msgstr ""

msgid "One game"
msgid_plural "{n} games"
msgstr[0] "Ett spel"
msgstr[1] "{n} spel"

msgid "One file"
msgid_plural "{n} files"
msgstr[0] "En fil"
msgstr[1] ""
"#;

    #[test]
    fn looks_up_translations() {
        let catalog = Catalog::parse(SV).unwrap();
        assert_eq!(catalog.get(None, "Show Caskade"), Some("Visa Caskade"));
        assert_eq!(catalog.get(None, "Not in the catalog"), None);
    }

    #[test]
    fn context_tells_the_same_english_apart() {
        let catalog = Catalog::parse(SV).unwrap();
        assert_eq!(
            catalog.get(Some("folder browser place"), "Home"),
            Some("Hem")
        );
        assert_eq!(catalog.get(None, "Home"), Some("Start"));
        assert_eq!(catalog.get(Some("another context"), "Home"), None);
    }

    #[test]
    fn fuzzy_and_empty_translations_are_missing() {
        let catalog = Catalog::parse(SV).unwrap();
        assert_eq!(catalog.get(None, "Quit Caskade"), None);
        assert_eq!(catalog.get(None, "File system"), None);
    }

    #[test]
    fn plurals_follow_the_languages_rule() {
        let catalog = Catalog::parse(SV).unwrap();
        assert_eq!(catalog.get_plural(None, "One game", 1), Some("Ett spel"));
        assert_eq!(catalog.get_plural(None, "One game", 0), Some("{n} spel"));
        assert_eq!(catalog.get_plural(None, "One game", 5), Some("{n} spel"));
        // An empty form is missing for its counts only.
        assert_eq!(catalog.get_plural(None, "One file", 1), Some("En fil"));
        assert_eq!(catalog.get_plural(None, "One file", 2), None);
    }

    #[test]
    fn a_broken_plural_rule_fails_the_catalog() {
        let text = "msgid \"\"\nmsgstr \"Plural-Forms: nplurals=2; plural=(n !! 1);\\n\"\n";
        assert!(matches!(Catalog::parse(text), Err(CatalogError::Plural(_))));
    }
}
