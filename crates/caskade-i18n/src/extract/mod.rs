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
use std::fmt;
use std::path::{Path, PathBuf};

mod check;
mod merge;
mod native;
mod rsx;
mod scan;
#[cfg(test)]
mod tests;

pub use check::{
    Completion, catalog_problems, check_catalogs, completion, describe, entry_problems,
    msgfmt_problems, package_catalogs,
};
pub use merge::{lost_translations, merge, merge_with};
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
    let names: Vec<&str> = DOMAINS.iter().map(|domain| domain.name).collect();
    crate::completion::language_dirs(&root.join("locales"), &names).map_err(|error| match error {
        crate::completion::CompletionError::Io(path, source) => ExtractError::Io(path, source),
        crate::completion::CompletionError::Parse(path, source) => {
            ExtractError::Catalog(path, source.to_string())
        }
    })
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
