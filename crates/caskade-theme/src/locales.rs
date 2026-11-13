//! A theme's translations (#219): `locales/<lang>.po` in the theme folder,
//! in a gettext domain named after the theme's id, with the English text of
//! `theme.json` as the message id. Only the name and the description are
//! translated; a theme has no other text.
//!
//! A catalog that can't be read is a problem `caskade-theme lint` reports,
//! but the theme still applies, in English. No catalogs means English.
use crate::{Files, Problem};
use caskade_i18n::catalogs::{LocaleCatalogs, PO_MAX, TOO_LARGE};

/// Where a theme keeps its catalogs.
const FOLDER: &str = "locales/";

/// A theme's catalogs by language.
#[derive(Clone, Debug, Default)]
pub struct Locales {
    catalogs: LocaleCatalogs,
}

impl Locales {
    /// Reads `locales/*.po`, with a problem for each catalog that can't be
    /// read or would show wrong text. Readable catalogs are kept either way.
    pub fn load(files: &dyn Files) -> (Self, Vec<Problem>) {
        let mut locales = Self::default();
        let mut problems = Vec::new();
        for path in files.list() {
            let Some(name) = path.strip_prefix(FOLDER) else {
                continue;
            };
            if !name.ends_with(".po") || name.contains('/') {
                continue;
            }
            let Some(bytes) = files.read(&path) else {
                continue;
            };
            if bytes.len() > PO_MAX {
                problems.push(Problem::new(&path, TOO_LARGE));
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else {
                problems.push(Problem::new(&path, "is not UTF-8"));
                continue;
            };
            match locales.catalogs.add(name, &text) {
                Ok(found) => {
                    problems.extend(found.into_iter().map(|found| Problem::new(&path, found)))
                }
                Err(problem) => problems.push(Problem::new(&path, problem)),
            }
        }
        (locales, problems)
    }

    /// The languages there are catalogs for.
    pub fn available(&self) -> Vec<&str> {
        self.catalogs.available()
    }

    /// `text` in `locale` (falling back from `pt-BR` to `pt`), or `text`.
    pub fn text(&self, locale: &str, text: &str) -> String {
        self.catalogs.text(locale, text)
    }
}
