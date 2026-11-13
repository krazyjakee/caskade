//! The catalogs a theme or plugin ships as `locales/<lang>.po` (#219), in
//! a gettext domain named after its id, with the English text as the
//! message id.

use crate::{Catalog, locale, plural::PluralForms, po::Po};

/// The largest catalog read.
pub const PO_MAX: usize = 1 << 20;
/// Why a catalog over [`PO_MAX`] isn't read.
pub const TOO_LARGE: &str = "is larger than 1 MiB";

/// One language's catalog.
#[derive(Debug, Clone)]
pub struct LocaleCatalog {
    /// Its BCP 47 tag, from the file name.
    pub locale: String,
    pub catalog: Catalog,
    pub po: Po,
    pub plurals: PluralForms,
}

/// A theme's or plugin's catalogs by language.
#[derive(Debug, Clone, Default)]
pub struct LocaleCatalogs(Vec<LocaleCatalog>);

impl LocaleCatalogs {
    /// Adds the catalog in the file `name` (`sv.po`). `Err` says why it
    /// can't be used, so its English shows; `Ok` lists translations that
    /// would show wrong text, kept anyway.
    pub fn add(&mut self, name: &str, text: &str) -> Result<Vec<String>, String> {
        let stem = name.strip_suffix(".po").unwrap_or(name);
        let locale = locale::normalize(stem)
            .ok_or("is not named after a language: use its BCP 47 tag, like sv.po or pt-BR.po")?;
        let loaded = Po::parse(text)
            .map_err(|error| error.to_string())
            .and_then(|po| {
                let plurals = po.plural_forms().map_err(|e| e.to_string())?;
                let catalog = Catalog::from_po(&po).map_err(|e| e.to_string())?;
                Ok((po, plurals, catalog))
            });
        let (po, plurals, catalog) =
            loaded.map_err(|error| format!("can't be read, so its English shows: {error}"))?;
        let problems = crate::lint_po(&po, &plurals);
        self.0.push(LocaleCatalog {
            locale,
            catalog,
            po,
            plurals,
        });
        Ok(problems)
    }

    /// The languages there are catalogs for.
    pub fn available(&self) -> Vec<&str> {
        self.0.iter().map(|c| c.locale.as_str()).collect()
    }

    /// The catalog `locale` falls back to (`pt-BR`, then `pt`), if any.
    pub fn get(&self, locale: &str) -> Option<&LocaleCatalog> {
        let available = self.available();
        let chosen = locale::negotiate([locale], &available)?;
        self.0.iter().find(|c| c.locale == chosen)
    }

    /// `text` in `locale`, or `text` itself.
    pub fn text(&self, locale: &str, text: &str) -> String {
        self.get(locale)
            .and_then(|loaded| loaded.catalog.get(None, text))
            .unwrap_or(text)
            .to_owned()
    }
}
