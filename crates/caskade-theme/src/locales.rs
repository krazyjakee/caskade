//! A theme's translations (#219): `locales/<lang>.po` in the theme folder,
//! in a gettext domain named after the theme's id, with the English text of
//! `theme.json` as the message id. Only the name and the description are
//! translated; a theme has no other text.
//!
//! A catalog that can't be read is a problem `caskade-theme lint` reports,
//! but the theme still applies, in English. No catalogs means English.
use crate::{Files, Problem};
use caskade_i18n::{Catalog, locale};

/// Where a theme keeps its catalogs.
const FOLDER: &str = "locales/";
/// The largest catalog read.
const PO_MAX: usize = 1 << 20;

/// A theme's catalogs by language.
#[derive(Clone, Debug, Default)]
pub struct Locales {
    catalogs: Vec<(String, Catalog)>,
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
            let Some(stem) = name.strip_suffix(".po") else {
                continue;
            };
            if stem.contains('/') {
                continue;
            }
            let Some(tag) = locale::normalize(stem) else {
                problems.push(Problem::new(
                    &path,
                    "is not named after a language: use its BCP 47 tag, like sv.po or pt-BR.po",
                ));
                continue;
            };
            let Some(bytes) = files.read(&path) else {
                continue;
            };
            if bytes.len() > PO_MAX {
                problems.push(Problem::new(&path, "is larger than 1 MiB"));
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else {
                problems.push(Problem::new(&path, "is not UTF-8"));
                continue;
            };
            match Catalog::parse(&text) {
                Ok(catalog) => {
                    if let Ok(found) = caskade_i18n::lint(&text) {
                        problems.extend(found.into_iter().map(|found| Problem::new(&path, found)));
                    }
                    locales.catalogs.push((tag, catalog));
                }
                Err(error) => problems.push(Problem::new(
                    &path,
                    format!("can't be read, so its English shows: {error}"),
                )),
            }
        }
        (locales, problems)
    }

    /// The languages there are catalogs for.
    pub fn available(&self) -> Vec<&str> {
        self.catalogs.iter().map(|(tag, _)| tag.as_str()).collect()
    }

    /// `text` in `locale` (falling back from `pt-BR` to `pt`), or `text`.
    pub fn text(&self, locale: &str, text: &str) -> String {
        let available = self.available();
        locale::negotiate([locale], &available)
            .and_then(|chosen| self.catalogs.iter().find(|(tag, _)| tag == chosen))
            .and_then(|(_, catalog)| catalog.get(None, text))
            .unwrap_or(text)
            .to_owned()
    }
}
