//! Extraction: finds each domain's marked messages in its Rust sources,
//! writes its POT template and merges the template into each language's PO
//! catalog, like `xgettext` and `msgmerge` (without fuzzy matching),
//! without gettext's tools. The core's messages are its `tr!`, `trn!`,
//! `trp!` and `trnp!` calls (and their stored-message forms `msg!`,
//! `msgn!`, `msgp!` and `msgnp!`); the native UI's `app` and `ui` domains
//! are its `t!`, `tn!`, `tp!`, `tnp!`, `trans!` and `transp!` calls, and
//! the `t_noop!` English translated where it's shown.
//! [`check_catalogs`] adds the translator checks behind `just i18n-check`.
//!
//! Sources are read as Rust tokens, so calls inside other macros'
//! arguments (`format!`, `tracing::warn!`) count, comments don't, and
//! string literals (raw, escaped) unescape as the compiler does. The output
//! is deterministic: references name files without line numbers, and the
//! template has no creation date, so extracting twice changes nothing and
//! moving code around doesn't make the template stale.

use crate::po::{Message, Po};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

mod check;
mod native;
mod rsx;
mod scan;

pub use check::{
    Completion, catalog_problems, check_catalogs, completion, describe, entry_problems,
    msgfmt_problems, package_catalogs,
};
pub use native::{NATIVE_SOURCES, UI_KIT, native_messages};
pub use rsx::{rsx_literals, untranslated_rsx};
use scan::Messages;

/// How a domain's messages are marked, and the syntax their text is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syntax {
    /// The core's `tr!` family: `{name}` placeholders (`{{` and `}}` are
    /// literal braces); a plural's count is `{n}`.
    Rust,
    /// The native UI's `t!` family: ICU (`{name}`, `#` for a plural's
    /// count, `{n, plural, …}` and `{x, select, …}` inside a message) and
    /// numbered tags (`<0>…</0>`).
    Icu,
}

/// Which of its sources' messages a domain has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    All,
    /// Those used in a file under this folder.
    Within(&'static str),
    /// Those not used in any file under this folder.
    Outside(&'static str),
}

impl Part {
    fn has(self, message: &Message) -> bool {
        let within = |folder: &str| {
            message
                .references
                .iter()
                .any(|file| file.starts_with(folder))
        };
        match self {
            Part::All => true,
            Part::Within(folder) => within(folder),
            Part::Outside(folder) => !within(folder),
        }
    }
}

/// A translation domain: where its messages are marked and where its
/// template and catalogs live (`locales/<name>.pot`,
/// `locales/<language>/<name>.po`).
#[derive(Debug, Clone, Copy)]
pub struct Domain {
    pub name: &'static str,
    /// Folders scanned for `.rs` files, relative to the repository root.
    pub sources: &'static [&'static str],
    /// The template's first comment line.
    pub title: &'static str,
    pub syntax: Syntax,
    pub part: Part,
    /// Domains whose catalogs in the same language a message new to this
    /// one takes its translation from (`msgmerge`'s compendium): the native
    /// UI looks `app` and `ui` up together, so a message moving between
    /// them keeps its translation.
    pub shares: &'static [&'static str],
}

/// The core's domain: the Rust crates, the native app's `tr!` calls and
/// the tray.
pub const CORE: Domain = Domain {
    name: "core",
    sources: &[
        "crates/caskade-core/src",
        "crates/caskade-achievements/src",
        "crates/caskade-addons/src",
        "crates/caskade-backup/src",
        "crates/caskade-share/src",
        "crates/caskade-streaming/src",
        "crates/caskade-system-stats/src",
        "crates/caskade-app/src",
        "crates/caskade-player/src",
        "crates/caskade-ui-theme/src",
        "crates/caskade-ui-data/src",
        "apps/native/src",
    ],
    title: "Caskade's core messages: the Rust crates, the native app's errors and dialogs, and the tray.",
    syntax: Syntax::Rust,
    part: Part::All,
    shares: &[],
};

/// The native UI's text, but for its shared components'.
pub const APP: Domain = Domain {
    name: "app",
    sources: NATIVE_SOURCES,
    title: "Caskade's interface: the native app's text, but for its shared components'.",
    syntax: Syntax::Icu,
    part: Part::Outside(UI_KIT),
    shares: &["ui"],
};

/// The native UI's shared components (`apps/native/src/ui`): buttons,
/// badges, dialogs.
pub const UI: Domain = Domain {
    name: "ui",
    sources: NATIVE_SOURCES,
    title: "Caskade's shared components (apps/native/src/ui): buttons, badges, dialogs.",
    syntax: Syntax::Icu,
    part: Part::Within(UI_KIT),
    shares: &["app"],
};

/// Every domain, in the order extraction writes them.
pub const DOMAINS: [Domain; 3] = [CORE, APP, UI];

/// The `Plural-Forms` of a catalog started without another of its
/// language's to copy it from: English's, for the translator to correct.
pub const DEFAULT_PLURAL_FORMS: &str = "nplurals=2; plural=(n != 1);";

/// Why extraction failed.
#[derive(Debug)]
pub enum ExtractError {
    Io(PathBuf, std::io::Error),
    /// A source file that isn't Rust tokens, or a call extraction can't
    /// read.
    Source(String, String),
    /// A catalog that can't be merged.
    Catalog(PathBuf, String),
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExtractError::Io(path, error) => write!(f, "{}: {error}", path.display()),
            ExtractError::Source(file, message) => write!(f, "{file}: {message}"),
            ExtractError::Catalog(path, message) => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for ExtractError {}

/// A file extraction writes, and what it should contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub path: PathBuf,
    pub contents: String,
}

impl Output {
    /// The file on disk already has these contents.
    pub fn is_current(&self) -> bool {
        std::fs::read_to_string(&self.path).is_ok_and(|on_disk| on_disk == self.contents)
    }

    /// Writes the file if it differs; whether it did.
    pub fn write(&self) -> Result<bool, ExtractError> {
        if self.is_current() {
            return Ok(false);
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| ExtractError::Io(parent.to_owned(), error))?;
        }
        std::fs::write(&self.path, &self.contents)
            .map_err(|error| ExtractError::Io(self.path.clone(), error))?;
        Ok(true)
    }
}

/// The translated languages: each folder of `locales/` with a catalog of
/// any domain, sorted. The pseudo-locale has none: it's made from the
/// English.
pub fn languages(root: &Path) -> Result<Vec<String>, ExtractError> {
    let mut languages = Vec::new();
    for entry in sorted_entries(&root.join("locales"))? {
        let has_catalog = DOMAINS
            .iter()
            .any(|domain| entry.join(format!("{}.po", domain.name)).is_file());
        if let Some(name) = entry.file_name().and_then(|name| name.to_str())
            && entry.is_dir()
            && has_catalog
            && name != crate::pseudo::LOCALE
        {
            languages.push(name.to_owned());
        }
    }
    Ok(languages)
}

/// A catalog, or `None` when there's no such file.
pub fn read_catalog(path: &Path) -> Result<Option<Po>, ExtractError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ExtractError::Io(path.to_owned(), error)),
    };
    Po::parse(&text)
        .map(Some)
        .map_err(|error| ExtractError::Catalog(path.to_owned(), error.to_string()))
}

/// An empty catalog for `language`, as `msginit` starts one: its
/// `Plural-Forms` copied from another of the language's catalogs, else
/// [`DEFAULT_PLURAL_FORMS`].
pub fn start_catalog(root: &Path, language: &str) -> Result<Po, ExtractError> {
    let mut plural_forms = None;
    for domain in DOMAINS {
        let path = domain.catalog_path(root, language);
        if let Some(po) = read_catalog(&path)?
            && let Some(forms) = po.header_field("Plural-Forms")
        {
            plural_forms = Some(forms.to_owned());
            break;
        }
    }
    let plural_forms = plural_forms.unwrap_or_else(|| DEFAULT_PLURAL_FORMS.to_owned());
    let mut header = Message::new(None, "", None);
    header.translations = vec![format!(
        "Project-Id-Version: Caskade\nLanguage: {language}\nMIME-Version: 1.0\nContent-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 8bit\nPlural-Forms: {plural_forms}\n"
    )];
    Ok(Po {
        header: Some(header),
        ..Po::default()
    })
}

impl Domain {
    /// `locales/<name>.pot`.
    pub fn template_path(&self, root: &Path) -> PathBuf {
        root.join("locales").join(format!("{}.pot", self.name))
    }

    /// `locales/<language>/<name>.po`.
    pub fn catalog_path(&self, root: &Path, language: &str) -> PathBuf {
        root.join("locales")
            .join(language)
            .join(format!("{}.po", self.name))
    }

    /// The domain's catalogs on disk, by language.
    pub fn catalog_paths(&self, root: &Path) -> Result<Vec<PathBuf>, ExtractError> {
        Ok(languages(root)?
            .iter()
            .map(|language| self.catalog_path(root, language))
            .filter(|path| path.is_file())
            .collect())
    }

    /// The domain's messages, from its sources under `root`: the core's in
    /// the order the sources have them, the native UI's by English and
    /// context.
    pub fn messages(&self, root: &Path) -> Result<Vec<Message>, ExtractError> {
        let messages = match self.syntax {
            Syntax::Rust => {
                let mut messages = Messages::default();
                each_source(root, self.sources, false, |reference, text| {
                    messages.scan(text, reference)
                })?;
                messages.list
            }
            Syntax::Icu => native::scan_sorted(root, self.sources)?,
        };
        Ok(messages
            .into_iter()
            .filter(|message| self.part.has(message))
            .collect())
    }

    /// The template, from the domain's sources under `root`.
    pub fn template(&self, root: &Path) -> Result<Po, ExtractError> {
        let mut header = Message::new(None, "", None);
        let calls = match self.syntax {
            Syntax::Rust => "the tr!, trn!, trp! and trnp! calls",
            Syntax::Icu => "the t!, tn!, tp!, tnp!, trans!, transp! and t_noop! calls",
        };
        header.translator_comments = vec![
            self.title.to_owned(),
            format!("Generated by `just i18n` from {calls}; don't edit."),
        ];
        header.translations = vec![
            [
                "Project-Id-Version: Caskade\n",
                "MIME-Version: 1.0\n",
                "Content-Type: text/plain; charset=UTF-8\n",
                "Content-Transfer-Encoding: 8bit\n",
                "Plural-Forms: nplurals=INTEGER; plural=EXPRESSION;\n",
            ]
            .concat(),
        ];
        Ok(Po {
            header: Some(header),
            messages: self.messages(root)?,
            obsolete: Vec::new(),
        })
    }

    /// Everything extraction writes for the translated languages: the
    /// template, and each language's catalog merged with it.
    pub fn outputs(&self, root: &Path) -> Result<Vec<Output>, ExtractError> {
        self.outputs_for(root, &languages(root)?)
    }

    /// The template, and the catalog of each of `languages` merged with it;
    /// a catalog the language lacks is started ([`start_catalog`]).
    pub fn outputs_for(
        &self,
        root: &Path,
        languages: &[String],
    ) -> Result<Vec<Output>, ExtractError> {
        let template = self.template(root)?;
        let mut outputs = vec![Output {
            path: self.template_path(root),
            contents: template.write(),
        }];
        for language in languages {
            let path = self.catalog_path(root, language);
            let catalog = match read_catalog(&path)? {
                Some(catalog) => catalog,
                None => start_catalog(root, language)?,
            };
            let mut compendium = Vec::new();
            for name in self.shares {
                let shared = root
                    .join("locales")
                    .join(language)
                    .join(format!("{name}.po"));
                compendium.extend(read_catalog(&shared)?);
            }
            let merged = merge_with(&template, &catalog, &compendium)
                .map_err(|error| ExtractError::Catalog(path.clone(), error))?;
            outputs.push(Output {
                path,
                contents: merged.write(),
            });
        }
        Ok(outputs)
    }
}

/// `catalog` updated to `template`, as `msgmerge` does without fuzzy
/// matching or a compendium: see [`merge_with`].
pub fn merge(template: &Po, catalog: &Po) -> Result<Po, String> {
    merge_with(template, catalog, &[])
}

type Key<'a> = (Option<&'a str>, &'a str);

/// Has a translation a lookup would show: not fuzzy, a form not empty.
fn translated(message: &Message) -> bool {
    !message.is_fuzzy() && message.translations.iter().any(|text| !text.is_empty())
}

/// `catalog` updated to `template`, as `msgmerge --compendium` does without
/// fuzzy matching:
///
/// - a message still in the template keeps its translation, comments and
///   flags (marked fuzzy when its English plural changed, or when it gained
///   or lost its plural, keeping the first form);
/// - an obsolete entry whose message is back is brought back;
/// - a new message, or one still untranslated, takes the translation a
///   `compendium` catalog (the same language's other domains) has for it,
///   else comes in untranslated;
/// - a translated message gone from the template becomes an obsolete
///   (`#~`) entry, keeping its translation, translator comments and fuzzy
///   flag; untranslated ones go.
///
/// References and notes for translators follow the template.
pub fn merge_with(template: &Po, catalog: &Po, compendium: &[Po]) -> Result<Po, String> {
    let forms = catalog.plural_forms().map_err(|error| error.to_string())?;
    let active: HashMap<Key<'_>, &Message> = catalog
        .messages
        .iter()
        .map(|message| (message.key(), message))
        .collect();
    let obsolete: HashMap<Key<'_>, &Message> = catalog
        .obsolete
        .iter()
        .map(|message| (message.key(), message))
        .collect();
    let mut memory: HashMap<Key<'_>, &Message> = HashMap::new();
    for found in compendium
        .iter()
        .flat_map(|po| po.messages.iter().chain(&po.obsolete))
    {
        let whole = found.plural.is_none() || found.translations.len() == forms.count;
        if translated(found) && whole {
            memory.entry(found.key()).or_insert(found);
        }
    }
    let messages = template
        .messages
        .iter()
        .map(|source| {
            let remembered = memory
                .get(&source.key())
                .filter(|found| found.plural.is_some() == source.plural.is_some());
            let mut merged = match active
                .get(&source.key())
                .or_else(|| obsolete.get(&source.key()))
            {
                Some(old) => {
                    let mut merged = (*old).clone();
                    if old.plural.is_some() != source.plural.is_some() {
                        let first = old.translations.first().cloned().unwrap_or_default();
                        merged.translations = if source.plural.is_some() {
                            vec![String::new(); forms.count]
                        } else {
                            vec![String::new()]
                        };
                        if let Some(slot) = merged.translations.first_mut() {
                            slot.clone_from(&first);
                        }
                        if !first.is_empty() && !merged.is_fuzzy() {
                            merged.flags.push("fuzzy".to_owned());
                        }
                    } else if old.plural != source.plural && !merged.is_fuzzy() {
                        merged.flags.push("fuzzy".to_owned());
                    }
                    let blank = merged.translations.iter().all(String::is_empty);
                    if blank
                        && !merged.is_fuzzy()
                        && let Some(found) = remembered
                    {
                        merged.translations.clone_from(&found.translations);
                    }
                    merged
                }
                None => {
                    let mut new = source.clone();
                    match remembered {
                        Some(found) => {
                            new.translations.clone_from(&found.translations);
                            new.translator_comments
                                .clone_from(&found.translator_comments);
                        }
                        None if source.plural.is_some() => {
                            new.translations = vec![String::new(); forms.count];
                        }
                        None => {}
                    }
                    new
                }
            };
            merged.plural.clone_from(&source.plural);
            merged.references.clone_from(&source.references);
            merged
                .extracted_comments
                .clone_from(&source.extracted_comments);
            merged
        })
        .collect();
    let current: HashSet<Key<'_>> = template.messages.iter().map(Message::key).collect();
    let mut seen = HashSet::new();
    let mut gone = Vec::new();
    for old in catalog.messages.iter().chain(&catalog.obsolete) {
        if current.contains(&old.key())
            || old.translations.iter().all(String::is_empty)
            || !seen.insert(old.key())
        {
            continue;
        }
        let mut entry = old.clone();
        entry.references.clear();
        entry.extracted_comments.clear();
        entry.flags.retain(|flag| flag == "fuzzy");
        gone.push(entry);
    }
    Ok(Po {
        header: catalog.header.clone().or_else(|| template.header.clone()),
        messages,
        obsolete: gone,
    })
}

/// The translations in `before` that `after` doesn't have: an entry with a
/// translation (fuzzy too) whose context and English no message or
/// obsolete entry of `after` has with a translation. A merge must never
/// lose one; a changed context would.
pub fn lost_translations(before: &Po, after: &Po) -> Vec<String> {
    let kept: HashSet<Key<'_>> = after
        .messages
        .iter()
        .chain(&after.obsolete)
        .filter(|entry| entry.translations.iter().any(|text| !text.is_empty()))
        .map(Message::key)
        .collect();
    before
        .messages
        .iter()
        .chain(&before.obsolete)
        .filter(|entry| {
            entry.translations.iter().any(|text| !text.is_empty()) && !kept.contains(&entry.key())
        })
        .map(describe)
        .collect()
}

fn reference(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Calls `visit` with the reference and text of every `.rs` file under
/// `sources` (relative to `root`), in order; test files (`tests.rs`,
/// `*_tests.rs`) are left out when `skip_tests`.
fn each_source(
    root: &Path,
    sources: &[&str],
    skip_tests: bool,
    mut visit: impl FnMut(&str, &str) -> Result<(), ExtractError>,
) -> Result<(), ExtractError> {
    for source in sources {
        let mut files = Vec::new();
        rust_files(&root.join(source), &mut files)?;
        for file in files {
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if skip_tests && (name == "tests.rs" || name.ends_with("_tests.rs")) {
                continue;
            }
            let text = std::fs::read_to_string(&file)
                .map_err(|error| ExtractError::Io(file.clone(), error))?;
            visit(&reference(root, &file), &text)?;
        }
    }
    Ok(())
}

fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>, ExtractError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ExtractError::Io(dir.to_owned(), error)),
    };
    let mut paths = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ExtractError::Io(dir.to_owned(), error))?;
    paths.sort();
    Ok(paths)
}

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ExtractError> {
    if !dir.is_dir() {
        return Err(ExtractError::Io(
            dir.to_owned(),
            std::io::Error::new(std::io::ErrorKind::NotFound, "no such source folder"),
        ));
    }
    for path in sorted_entries(dir)? {
        if path.is_dir() {
            rust_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(sources: &[(&str, &str)]) -> Result<Po, ExtractError> {
        let mut messages = Messages::default();
        for (file, text) in sources {
            messages.scan(text, file)?;
        }
        Ok(Po {
            messages: messages.list,
            ..Po::default()
        })
    }

    /// The `t!` family's messages, as the native UI's domains scan them.
    fn extract_native(sources: &[(&str, &str)]) -> Po {
        let mut messages = Messages::with(scan::NATIVE_KEYWORDS);
        for (file, text) in sources {
            messages.scan(text, file).unwrap();
        }
        Po {
            messages: messages.list,
            ..Po::default()
        }
    }

    const SV_HEADER: &str =
        "msgid \"\"\nmsgstr \"Language: sv\\nPlural-Forms: nplurals=2; plural=(n != 1);\\n\"\n\n";

    #[test]
    fn finds_every_kind_of_call() {
        let po = extract(&[(
            "src/a.rs",
            r##"
            // tr!("not in a comment")
            /// tr!("nor in docs")
            fn f(n: u64) {
                let _ = crate::tr!("Show Caskade");
                let _ = trn!("One game", "{n} games", n);
                let _ = trp!("folder browser place", "Home");
                let _ = caskade_core::trnp!("tray", "One window", "{n} windows", n, extra = 1);
                tracing::warn!("{}", tr!(r#"A "raw" string"#));
                let _ = tr!("Line\nbreak {a}", a = tr!("nested"));
                let _ = not_tr!("skipped");
                let _ = caskade_core::msgnp!("feed", "One update", "{n} updates", n);
                let _ = crate::msg!("{game} added", game = crate::msgp!("verb", "Quit"));
            }
            macro_rules! tr { ($msgid:literal) => {}; }
            "##,
        )])
        .unwrap();
        let keys: Vec<_> = po
            .messages
            .iter()
            .map(|message| {
                (
                    message.context.as_deref(),
                    message.id.as_str(),
                    message.plural.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            keys,
            [
                (None, "Show Caskade", None),
                (None, "One game", Some("{n} games")),
                (Some("folder browser place"), "Home", None),
                (Some("tray"), "One window", Some("{n} windows")),
                (None, "A \"raw\" string", None),
                (None, "Line\nbreak {a}", None),
                (None, "nested", None),
                (Some("feed"), "One update", Some("{n} updates")),
                (None, "{game} added", None),
                (Some("verb"), "Quit", None),
            ]
        );
        assert_eq!(po.messages[1].translations, ["", ""]);
    }

    #[test]
    fn a_message_used_twice_lists_both_files_once() {
        let po = extract(&[
            ("src/a.rs", r#"fn a() { tr!("Quit"); tr!("Quit"); }"#),
            (
                "src/b.rs",
                r#"fn b() { tr!("Quit"); trp!("verb", "Quit"); }"#,
            ),
        ])
        .unwrap();
        assert_eq!(po.messages.len(), 2);
        assert_eq!(po.messages[0].references, ["src/a.rs", "src/b.rs"]);
        assert_eq!(po.messages[1].references, ["src/b.rs"]);
    }

    #[test]
    fn rejects_calls_it_cannot_read() {
        for source in [
            r#"fn a() { tr!(MESSAGE); }"#,
            r#"fn a() { trn!("One"); }"#,
            r#"fn a() { tr!(""); }"#,
            r#"fn a() { tr!("x" "y"); }"#,
            r#"fn a() { tr!("Games"); trn!("Games", "{n} games", 2); }"#,
        ] {
            assert!(extract(&[("src/a.rs", source)]).is_err(), "{source}");
        }
    }

    #[test]
    fn merging_keeps_translations_and_follows_the_template() {
        let template = extract(&[(
            "src/a.rs",
            r#"fn a(n: u64) { tr!("Kept"); tr!("New"); trn!("One game", "{n} games", n); }"#,
        )])
        .unwrap();
        let catalog = Po::parse(
            r#"msgid ""
msgstr "Plural-Forms: nplurals=3; plural=(n==1 ? 0 : n==2 ? 1 : 2);\n"

# Translator's note.
#: src/old.rs
msgid "Kept"
msgstr "Behållen"

msgid "One game"
msgid_plural "{n} game(s)"
msgstr[0] "Ett spel"
msgstr[1] "Två spel"
msgstr[2] "{n} spel"

msgid "Gone"
msgstr "Borta"
"#,
        )
        .unwrap();
        let merged = merge(&template, &catalog).unwrap();
        assert_eq!(merged.header, catalog.header);
        let ids: Vec<_> = merged
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect();
        assert_eq!(ids, ["Kept", "New", "One game"]);
        // The message the code dropped keeps its translation, obsolete.
        assert_eq!(merged.obsolete.len(), 1);
        assert_eq!(merged.obsolete[0].id, "Gone");
        assert_eq!(merged.obsolete[0].translations, ["Borta"]);
        let kept = &merged.messages[0];
        assert_eq!(kept.translations, ["Behållen"]);
        assert_eq!(kept.translator_comments, ["Translator's note."]);
        assert_eq!(kept.references, ["src/a.rs"]);
        assert_eq!(merged.messages[1].translations, [""]);
        // The English plural changed: the translation stays, as a guess.
        let games = &merged.messages[2];
        assert!(games.is_fuzzy());
        assert_eq!(games.plural.as_deref(), Some("{n} games"));
        assert_eq!(games.translations.len(), 3);
        // Merging again changes nothing.
        let again = merge(&template, &merged).unwrap();
        assert_eq!(again.write(), merged.write());
    }

    #[test]
    fn new_plural_messages_get_the_languages_forms() {
        let template = extract(&[("src/a.rs", r#"fn a() { trn!("One", "{n}", 2); }"#)]).unwrap();
        let catalog = Po::parse(
            "msgid \"\"\nmsgstr \"Plural-Forms: nplurals=3; plural=(n==1 ? 0 : n==2 ? 1 : 2);\\n\"\n",
        )
        .unwrap();
        let merged = merge(&template, &catalog).unwrap();
        assert_eq!(merged.messages[0].translations, ["", "", ""]);
    }

    #[test]
    fn merging_keeps_contexts_plurals_notes_and_tags() {
        let template = extract_native(&[(
            "apps/native/src/a.rs",
            r##"fn a(n: u64) {
                t!(note = "A button", "Open");
                tp!("verb", "Open");
                tn!("# game", "# games", n);
                trans!("<0>{guide}</0> opens the quick menu", guide = g; 0 => |x| x);
            }"##,
        )]);
        let catalog = Po::parse(&format!(
            "{SV_HEADER}{}",
            r##"# Checked by a native speaker.
#. An old note
#: apps/desktop/src/Old.tsx
msgid "Open"
msgstr "Öppna"

msgctxt "verb"
msgid "Open"
msgstr "Öppna (verb)"

msgid "# game"
msgid_plural "# games"
msgstr[0] "# spel"
msgstr[1] "# spel"

msgid "<0>{guide}</0> opens the quick menu"
msgstr "<0>{guide}</0> öppnar snabbmenyn"
"##
        ))
        .unwrap();
        let merged = merge(&template, &catalog).unwrap();
        let find = |context: Option<&str>, id: &str| {
            merged
                .messages
                .iter()
                .find(|message| message.key() == (context, id))
                .unwrap()
        };
        let open = find(None, "Open");
        assert_eq!(open.translations, ["Öppna"]);
        assert_eq!(open.translator_comments, ["Checked by a native speaker."]);
        // Notes and references follow the code.
        assert_eq!(open.extracted_comments, ["A button"]);
        assert_eq!(open.references, ["apps/native/src/a.rs"]);
        assert_eq!(find(Some("verb"), "Open").translations, ["Öppna (verb)"]);
        let games = find(None, "# game");
        assert_eq!(games.plural.as_deref(), Some("# games"));
        assert_eq!(games.translations, ["# spel", "# spel"]);
        assert!(!games.is_fuzzy());
        assert_eq!(
            find(None, "<0>{guide}</0> opens the quick menu").translations,
            ["<0>{guide}</0> öppnar snabbmenyn"]
        );
        assert!(merged.obsolete.is_empty());
        assert!(lost_translations(&catalog, &merged).is_empty());
    }

    #[test]
    fn messages_the_code_drops_stay_obsolete_and_come_back() {
        let catalog = Po::parse(&format!(
            "{SV_HEADER}{}",
            r##"# Checked.
#. A note
#: apps/desktop/src/Old.tsx
#, fuzzy, ts-format
msgctxt "navigation"
msgid "Library"
msgstr "Bibliotek"

msgid "One file"
msgid_plural "# files"
msgstr[0] "En fil"
msgstr[1] "# filer"

msgid "Untranslated"
msgstr ""

#~ msgid "Older"
#~ msgstr "Äldre"
"##
        ))
        .unwrap();
        let empty = extract_native(&[]);
        let gone = merge(&empty, &catalog).unwrap();
        assert!(gone.messages.is_empty());
        let ids: Vec<_> = gone
            .obsolete
            .iter()
            .map(|entry| (entry.context.as_deref(), entry.id.as_str()))
            .collect();
        // Untranslated entries go; the rest keep their context.
        assert_eq!(
            ids,
            [
                (Some("navigation"), "Library"),
                (None, "One file"),
                (None, "Older")
            ]
        );
        let library = &gone.obsolete[0];
        assert_eq!(library.translator_comments, ["Checked."]);
        assert!(library.references.is_empty() && library.extracted_comments.is_empty());
        assert_eq!(library.flags, ["fuzzy"]);
        assert_eq!(gone.obsolete[1].translations, ["En fil", "# filer"]);
        assert!(lost_translations(&catalog, &gone).is_empty());
        // Written and read back, it merges the same.
        let reread = Po::parse(&gone.write()).unwrap();
        assert_eq!(merge(&empty, &reread).unwrap().write(), gone.write());
        // The code marks them again: their translations are back.
        let back = extract_native(&[(
            "apps/native/src/a.rs",
            r##"fn a(n: u64) { tp!("navigation", "Library"); tn!("One file", "# files", n); }"##,
        )]);
        let revived = merge(&back, &reread).unwrap();
        assert_eq!(revived.messages.len(), 2);
        assert_eq!(revived.messages[0].translations, ["Bibliotek"]);
        assert!(revived.messages[0].is_fuzzy());
        assert_eq!(revived.messages[1].translations, ["En fil", "# filer"]);
        assert_eq!(revived.messages[1].references, ["apps/native/src/a.rs"]);
        let ids: Vec<_> = revived.obsolete.iter().map(|entry| &entry.id).collect();
        assert_eq!(ids, ["Older"]);
    }

    #[test]
    fn a_changed_context_is_a_lost_translation() {
        let catalog = Po::parse(&format!(
            "{SV_HEADER}msgctxt \"size\"\nmsgid \"Unknown\"\nmsgstr \"Okänd\"\n"
        ))
        .unwrap();
        let mut renamed = catalog.clone();
        renamed.messages[0].context = Some("game size".to_owned());
        let lost = lost_translations(&catalog, &renamed);
        assert_eq!(lost.len(), 1);
        assert!(
            lost[0].contains("size") && lost[0].contains("Unknown"),
            "{lost:?}"
        );
        // Through a merge, the old context's translation stays obsolete.
        let template = extract_native(&[(
            "apps/native/src/a.rs",
            r##"fn a() { tp!("game size", "Unknown"); }"##,
        )]);
        let merged = merge(&template, &catalog).unwrap();
        assert_eq!(merged.messages[0].translations, [""]);
        assert_eq!(merged.obsolete[0].context.as_deref(), Some("size"));
        assert!(lost_translations(&catalog, &merged).is_empty());
    }

    #[test]
    fn a_message_moving_between_domains_keeps_its_translation() {
        let ui = Po::parse(&format!(
            "{SV_HEADER}{}",
            r##"msgid "Cancel"
msgstr "Avbryt"

msgid "# store"
msgid_plural "# stores"
msgstr[0] "# butik"
msgstr[1] "# butiker"

#~ msgid "More"
#~ msgstr "Mer"

#, fuzzy
msgid "Guess"
msgstr "Gissning"
"##
        ))
        .unwrap();
        let app = Po::parse(&format!("{SV_HEADER}msgid \"Cancel\"\nmsgstr \"\"\n")).unwrap();
        let template = extract_native(&[(
            "apps/native/src/a.rs",
            r##"fn a(n: u64) { t!("Cancel"); t!("More"); t!("Guess"); tn!("# store", "# stores", n); }"##,
        )]);
        let merged = merge_with(&template, &app, std::slice::from_ref(&ui)).unwrap();
        let translations: Vec<_> = merged
            .messages
            .iter()
            .map(|message| (message.id.as_str(), message.translations.clone()))
            .collect();
        assert_eq!(
            translations,
            [
                ("Cancel", vec!["Avbryt".to_owned()]),
                ("More", vec!["Mer".to_owned()]),
                // A guess isn't carried over.
                ("Guess", vec![String::new()]),
                (
                    "# store",
                    vec!["# butik".to_owned(), "# butiker".to_owned()]
                ),
            ]
        );
        // Merging again changes nothing.
        let again = merge_with(&template, &merged, std::slice::from_ref(&ui)).unwrap();
        assert_eq!(again.write(), merged.write());
    }

    #[test]
    fn a_message_gaining_a_plural_keeps_its_translation_as_a_guess() {
        let catalog =
            Po::parse(&format!("{SV_HEADER}msgid \"# game\"\nmsgstr \"# spel\"\n")).unwrap();
        let template = extract_native(&[(
            "apps/native/src/a.rs",
            r##"fn a(n: u64) { tn!("# game", "# games", n); }"##,
        )]);
        let merged = merge(&template, &catalog).unwrap();
        let games = &merged.messages[0];
        assert!(games.is_fuzzy());
        assert_eq!(games.translations, ["# spel", ""]);
        assert!(lost_translations(&catalog, &merged).is_empty());
    }

    #[test]
    fn the_native_ui_splits_into_app_and_ui_by_folder() {
        let messages = extract_native(&[
            (
                "apps/native/src/library/view.rs",
                r##"fn a() { t!("Library"); t!("Cancel"); }"##,
            ),
            (
                "apps/native/src/ui/dialog.rs",
                r##"fn a() { t!("Cancel"); t!("Close"); }"##,
            ),
            (
                "crates/caskade-ui-i18n/src/format.rs",
                r##"fn a() { t!("Unknown"); }"##,
            ),
        ])
        .messages;
        let ids = |part: Part| {
            messages
                .iter()
                .filter(|message| part.has(message))
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(APP.part), ["Library", "Unknown"]);
        assert_eq!(ids(UI.part), ["Cancel", "Close"]);
    }

    #[test]
    fn a_started_catalog_has_the_languages_header() {
        let root = std::env::temp_dir().join(format!("caskade-i18n-start-{}", std::process::id()));
        let sv = root.join("locales/sv");
        std::fs::create_dir_all(&sv).unwrap();
        std::fs::write(
            sv.join("core.po"),
            "msgid \"\"\nmsgstr \"Plural-Forms: nplurals=3; plural=(n==1 ? 0 : n==2 ? 1 : 2);\\n\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("locales/en-XA")).unwrap();
        std::fs::write(root.join("locales/en-XA/app.po"), "").unwrap();
        assert_eq!(languages(&root).unwrap(), ["sv"]);
        let started = start_catalog(&root, "sv").unwrap();
        assert_eq!(started.header_field("Language"), Some("sv"));
        assert_eq!(started.plural_forms().unwrap().count, 3);
        let new = start_catalog(&root, "de").unwrap();
        assert_eq!(new.header_field("Plural-Forms"), Some(DEFAULT_PLURAL_FORMS));
        let _ = std::fs::remove_dir_all(&root);
    }
}
