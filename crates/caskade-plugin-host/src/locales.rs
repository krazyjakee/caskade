//! A plugin's own translations (#219): `locales/<lang>.po` in its folder,
//! in a gettext domain named after the plugin's id. The English text is the
//! message id, as everywhere in Caskade (`docs/localization.md`).
//!
//! The host translates what it shows from `plugin.json` (the name, the
//! description, slot, view and command titles, and the settings schema's
//! titles and descriptions), serves the catalog of the language in use to
//! the plugin's UI half ([`PluginMessages`]) and tells its backend the
//! language (`host.locale`). A catalog that can't be read is a warning on
//! the plugin, and its English shows; a plugin without catalogs is English.

use crate::{
    manifest::{Manifest, ManifestError},
    read, write,
};
use caskade_i18n::{Catalog, locale, plural::PluralForms, po::Po};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::{
    path::Path,
    sync::{Arc, RwLock},
};

/// The folder in a plugin that holds its catalogs.
pub const LOCALES: &str = "locales";
/// The largest catalog read.
const PO_MAX: u64 = 1 << 20;
/// The most catalogs read from one plugin.
const CATALOGS_MAX: usize = 100;
/// Counts the UI's plural table covers; see [`PluginMessages::plurals`].
const PLURAL_TABLE: u64 = 200;

/// The language plugins' text shows in: the core's (`caskade_core::i18n`),
/// unless a test pins one for its host.
#[derive(Debug, Clone, Default)]
pub struct Language {
    pinned: Arc<RwLock<Option<String>>>,
}

impl Language {
    /// The language in use: a BCP 47 tag, `en` for English.
    pub fn current(&self) -> String {
        read(&self.pinned)
            .clone()
            .unwrap_or_else(caskade_core::i18n::locale)
    }

    /// Uses `locale` instead of the core's language; `None` follows the
    /// core again.
    pub fn pin(&self, locale: Option<&str>) {
        *write(&self.pinned) = locale.map(str::to_owned);
    }
}

struct Loaded {
    locale: String,
    catalog: Catalog,
    po: Po,
    plurals: PluralForms,
}

/// A plugin's catalogs, read from its `locales/` folder.
#[derive(Default)]
pub struct PluginLocales {
    /// (file name, text) of each catalog read, to tell when they changed.
    sources: Vec<(String, String)>,
    catalogs: Vec<Loaded>,
    problems: Vec<ManifestError>,
}

impl PartialEq for PluginLocales {
    fn eq(&self, other: &Self) -> bool {
        self.sources == other.sources && self.problems == other.problems
    }
}

impl std::fmt::Debug for PluginLocales {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginLocales")
            .field(
                "locales",
                &self.catalogs.iter().map(|c| &c.locale).collect::<Vec<_>>(),
            )
            .field("problems", &self.problems)
            .finish()
    }
}

fn problem(file: &str, message: impl Into<String>) -> ManifestError {
    ManifestError {
        key: format!("{LOCALES}/{file}"),
        message: message.into(),
    }
}

impl PluginLocales {
    /// Reads `<folder>/locales/*.po`. Nothing there is not a problem; a
    /// file that can't be read, or isn't named after a language, is.
    pub fn read(folder: &Path) -> Self {
        let mut locales = Self::default();
        let Ok(entries) = std::fs::read_dir(folder.join(LOCALES)) else {
            return locales;
        };
        let mut files: Vec<_> = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "po"))
            .collect();
        files.sort_by_key(|entry| entry.file_name());
        for entry in files.into_iter().take(CATALOGS_MAX) {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Symlinks are not followed, so nothing outside the folder is read.
            let plain = entry.file_type().is_ok_and(|kind| kind.is_file());
            let size = entry.metadata().map_or(u64::MAX, |meta| meta.len());
            if !plain {
                locales.problems.push(problem(&name, "is not a plain file"));
                continue;
            }
            if size > PO_MAX {
                locales
                    .problems
                    .push(problem(&name, "is larger than 1 MiB"));
                continue;
            }
            match std::fs::read_to_string(entry.path()) {
                Ok(text) => locales.add(&name, text),
                Err(error) => locales.problems.push(problem(&name, error.to_string())),
            }
        }
        locales
    }

    /// Adds one catalog, `name` being its file name (`sv.po`).
    fn add(&mut self, name: &str, text: String) {
        let stem = name.strip_suffix(".po").unwrap_or(name);
        let Some(tag) = locale::normalize(stem) else {
            self.problems.push(problem(
                name,
                "is not named after a language: use its BCP 47 tag, like sv.po or pt-BR.po",
            ));
            return;
        };
        let loaded = Po::parse(&text)
            .map_err(|error| error.to_string())
            .and_then(|po| {
                let plurals = po.plural_forms().map_err(|e| e.to_string())?;
                let catalog = Catalog::from_po(&po).map_err(|e| e.to_string())?;
                Ok((po, plurals, catalog))
            });
        match loaded {
            Ok((po, plurals, catalog)) => {
                // Readable, but some translations would show wrong text.
                if let Ok(found) = caskade_i18n::lint(&text) {
                    self.problems
                        .extend(found.into_iter().map(|found| problem(name, found)));
                }
                self.catalogs.push(Loaded {
                    locale: tag,
                    catalog,
                    po,
                    plurals,
                });
            }
            Err(error) => {
                tracing::warn!(file = name, %error, "a plugin's translation catalog couldn't be read");
                self.problems.push(problem(
                    name,
                    format!("can't be read, so its English shows: {error}"),
                ));
            }
        }
        self.sources.push((name.to_owned(), text));
    }

    /// Catalogs that couldn't be read, or have translations that would show
    /// wrong text.
    pub fn problems(&self) -> &[ManifestError] {
        &self.problems
    }

    /// The languages there are catalogs for.
    pub fn available(&self) -> Vec<&str> {
        self.catalogs.iter().map(|c| c.locale.as_str()).collect()
    }

    /// The catalog `locale` falls back to (`pt-BR`, then `pt`), if any.
    fn catalog(&self, locale: &str) -> Option<&Loaded> {
        let available = self.available();
        let chosen = locale::negotiate([locale], &available)?;
        self.catalogs.iter().find(|c| c.locale == chosen)
    }

    /// `text` in `locale`, or `text` itself.
    pub fn text(&self, locale: &str, text: &str) -> String {
        self.catalog(locale)
            .and_then(|loaded| loaded.catalog.get(None, text))
            .unwrap_or(text)
            .to_owned()
    }

    /// The manifest's texts in `locale`: the name, the description, slot,
    /// view and command titles and the settings schema's titles and
    /// descriptions. Only for showing; ids and everything else stay.
    pub fn translate_manifest(&self, locale: &str, manifest: &mut Manifest) {
        let Some(loaded) = self.catalog(locale) else {
            return;
        };
        let catalog = &loaded.catalog;
        let tr = |text: &mut String| {
            if let Some(translated) = catalog.get(None, text) {
                *text = translated.to_owned();
            }
        };
        tr(&mut manifest.name);
        if let Some(description) = &mut manifest.description {
            tr(description);
        }
        let contributes = &mut manifest.contributes;
        for slot in &mut contributes.slots {
            if let Some(title) = &mut slot.title {
                tr(title);
            }
        }
        for view in &mut contributes.views {
            tr(&mut view.title);
        }
        for command in &mut contributes.commands {
            tr(&mut command.title);
        }
        if let Some(schema) = &mut contributes.settings {
            translate_schema(catalog, schema);
        }
    }

    /// A settings schema's titles and descriptions in `locale`.
    pub fn translate_schema(&self, locale: &str, schema: &mut Value) {
        if let Some(loaded) = self.catalog(locale) {
            translate_schema(&loaded.catalog, schema);
        }
    }

    /// The catalog for `locale`, for the plugin's UI half; English (no
    /// messages) when it has none.
    pub fn messages(&self, locale: &str) -> PluginMessages {
        let Some(loaded) = self.catalog(locale) else {
            return PluginMessages {
                locale: locale::SOURCE.to_owned(),
                messages: Vec::new(),
                plurals: (0..PLURAL_TABLE).map(|n| u8::from(n != 1)).collect(),
            };
        };
        PluginMessages {
            locale: loaded.locale.clone(),
            messages: loaded
                .po
                .messages
                .iter()
                .filter(|message| {
                    !message.is_fuzzy() && message.translations.iter().any(|text| !text.is_empty())
                })
                .map(|message| PluginMessage {
                    context: message.context.clone(),
                    id: message.id.clone(),
                    translations: message.translations.clone(),
                })
                .collect(),
            plurals: (0..PLURAL_TABLE)
                .map(|n| u8::try_from(loaded.plurals.index(n)).unwrap_or(0))
                .collect(),
        }
    }
}

/// Translates a JSON Schema's `title` and `description` strings in place.
fn translate_schema(catalog: &Catalog, schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                match (key.as_str(), value) {
                    ("title" | "description", Value::String(text)) => {
                        if let Some(translated) = catalog.get(None, text) {
                            *text = translated.to_owned();
                        }
                    }
                    // Values, not schemas.
                    ("default" | "const" | "enum" | "examples", _) => {}
                    (_, value) => translate_schema(catalog, value),
                }
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| translate_schema(catalog, item)),
        _ => {}
    }
}

/// A plugin's catalog for the language in use, for `t` and `<Trans>` in its
/// UI half (`@caskade/sdk`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginMessages {
    /// The catalog's language; `en` when the plugin has none for the
    /// language in use, and its English shows.
    pub locale: String,
    /// Translated messages; fuzzy and empty ones are left out.
    pub messages: Vec<PluginMessage>,
    /// The plural form (`msgstr[i]`) for each count below 200, from the
    /// catalog's `Plural-Forms`. A larger count `n` takes the form of
    /// `100 + n % 100`, as gettext's rules only look at that much.
    pub plurals: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginMessage {
    pub context: Option<String>,
    /// The English text.
    pub id: String,
    /// One translation, or one per plural form.
    pub translations: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Stats"
msgstr "Statistik"

msgid "Top games"
msgstr "Toppspel"

#, fuzzy
msgid "Info"
msgstr "Information"

msgid "One pack"
msgid_plural "{n} packs"
msgstr[0] "Ett paket"
msgstr[1] "{n} paket"
"#;

    fn locales(files: &[(&str, &str)]) -> PluginLocales {
        let mut locales = PluginLocales::default();
        for (name, text) in files {
            locales.add(name, (*text).to_owned());
        }
        locales
    }

    #[test]
    fn manifest_text_is_looked_up_with_the_english_as_id() {
        let locales = locales(&[("sv.po", SV)]);
        assert!(locales.problems().is_empty(), "{:?}", locales.problems());
        assert_eq!(locales.text("sv", "Stats"), "Statistik");
        // `sv-FI` falls back to `sv`; other languages show English.
        assert_eq!(locales.text("sv-FI", "Stats"), "Statistik");
        assert_eq!(locales.text("de", "Stats"), "Stats");
        assert_eq!(locales.text("sv", "Info"), "Info", "fuzzy shows English");
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {"limit": {"type": "integer", "title": "Top games", "default": 5}},
        });
        locales.translate_schema("sv", &mut schema);
        assert_eq!(schema["properties"]["limit"]["title"], "Toppspel");
    }

    #[test]
    fn the_ui_gets_the_catalog_and_its_plural_table() {
        let locales = locales(&[("sv.po", SV)]);
        let messages = locales.messages("sv");
        assert_eq!(messages.locale, "sv");
        assert!(messages.messages.iter().all(|message| message.id != "Info"));
        let packs = messages
            .messages
            .iter()
            .find(|message| message.id == "One pack")
            .unwrap();
        assert_eq!(packs.translations, ["Ett paket", "{n} paket"]);
        assert_eq!(&messages.plurals[..3], [1, 0, 1]);
        assert_eq!(locales.messages("de").locale, "en");
    }

    #[test]
    fn a_broken_catalog_is_a_problem_and_shows_english() {
        let locales = locales(&[
            ("sv.po", "msgid \"Stats\"\nmsgstr \"Statistik"),
            ("not a language.po", SV),
        ]);
        assert_eq!(locales.problems().len(), 2, "{:?}", locales.problems());
        assert_eq!(locales.problems()[0].key, "locales/sv.po");
        assert_eq!(locales.text("sv", "Stats"), "Stats");
    }

    #[test]
    fn no_catalogs_is_english_without_problems() {
        let folder = tempfile::tempdir().unwrap();
        let locales = PluginLocales::read(folder.path());
        assert!(locales.problems().is_empty());
        assert_eq!(locales.text("sv", "Stats"), "Stats");
    }
}
