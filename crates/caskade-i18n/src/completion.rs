//! Translation completeness across PO catalogs and POT templates, using only
//! [`crate::po::Po`] and `std` (available without the `extract` feature so
//! build scripts can count translations too).

use crate::po::{ParseError, Po};
use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

/// One language's translated messages in one domain, over the template's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub domain: &'static str,
    pub translated: usize,
    pub total: usize,
}

/// Completeness results for every translated language, along with every
/// template and catalog path inspected (whether present on disk or not, so
/// build scripts can emit `cargo:rerun-if-changed` for them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub languages: Vec<(String, Vec<Completion>)>,
    pub visited: Vec<PathBuf>,
}

/// Why completeness couldn't be counted.
#[derive(Debug)]
pub enum CompletionError {
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, ParseError),
}

impl fmt::Display for CompletionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompletionError::Io(path, error) => write!(f, "{}: {error}", path.display()),
            CompletionError::Parse(path, error) => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl std::error::Error for CompletionError {}

fn read_po(path: &Path, visited: &mut Vec<PathBuf>) -> Result<Option<Po>, CompletionError> {
    visited.push(path.to_path_buf());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(CompletionError::Io(path.to_owned(), error)),
    };
    Po::parse(&text)
        .map(Some)
        .map_err(|error| CompletionError::Parse(path.to_owned(), error))
}

pub(crate) fn language_dirs(
    locales: &Path,
    domains: &[&str],
) -> Result<Vec<String>, CompletionError> {
    let entries = match std::fs::read_dir(locales) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CompletionError::Io(locales.to_owned(), error)),
    };
    let mut paths = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CompletionError::Io(locales.to_owned(), error))?;
    paths.sort();
    let mut languages = Vec::new();
    for entry in paths {
        let has_catalog = domains
            .iter()
            .any(|domain| entry.join(format!("{domain}.po")).is_file());
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

/// Each translated language's completion by domain under `locales`
/// (`<locales>/<domain>.pot` and `<locales>/<language>/<domain>.po`).
///
/// A message counts when every form is translated and it isn't fuzzy;
/// obsolete entries and messages no template has don't. The pseudo-locale
/// (`en-XA`) is skipped.
pub fn count(locales: &Path, domains: &[&'static str]) -> Result<Report, CompletionError> {
    let mut visited = Vec::new();
    let mut templates = Vec::with_capacity(domains.len());
    for domain in domains {
        let keys: HashSet<(Option<String>, String)> =
            read_po(&locales.join(format!("{domain}.pot")), &mut visited)?
                .map(|po| {
                    po.messages
                        .into_iter()
                        .map(|message| (message.context, message.id))
                        .collect()
                })
                .unwrap_or_default();
        templates.push(keys);
    }
    let mut languages = Vec::new();
    for language in language_dirs(locales, domains)? {
        let mut counts = Vec::with_capacity(domains.len());
        for (domain, template) in domains.iter().zip(&templates) {
            let path = locales.join(&language).join(format!("{domain}.po"));
            let translated = read_po(&path, &mut visited)?
                .map(|po| {
                    po.messages
                        .iter()
                        .filter(|message| {
                            !message.is_fuzzy()
                                && !message.translations.is_empty()
                                && message.translations.iter().all(|form| !form.is_empty())
                                && template.contains(&(message.context.clone(), message.id.clone()))
                        })
                        .count()
                })
                .unwrap_or(0);
            counts.push(Completion {
                domain,
                translated,
                total: template.len(),
            });
        }
        languages.push((language, counts));
    }
    Ok(Report { languages, visited })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_non_fuzzy_fully_translated_template_messages_and_records_paths() {
        let dir =
            std::env::temp_dir().join(format!("caskade-i18n-completion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let sv = dir.join("sv");
        let pseudo = dir.join(crate::pseudo::LOCALE);
        std::fs::create_dir_all(&sv).unwrap();
        std::fs::create_dir_all(&pseudo).unwrap();
        std::fs::write(
            dir.join("app.pot"),
            r##"msgid ""
msgstr "Content-Type: text/plain; charset=UTF-8\n"

msgid "One"
msgstr ""

msgid "Two"
msgstr ""

msgid "One game"
msgid_plural "# games"
msgstr[0] ""
msgstr[1] ""
"##,
        )
        .unwrap();
        std::fs::write(
            sv.join("app.po"),
            r##"msgid ""
msgstr "Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "One"
msgstr "Ett"

#, fuzzy
msgid "Two"
msgstr "Två"

msgid "One game"
msgid_plural "# games"
msgstr[0] "Ett spel"
msgstr[1] ""

msgid "Extra"
msgstr "Extra"

#~ msgid "Obsolete"
#~ msgstr "Gammal"
"##,
        )
        .unwrap();
        std::fs::write(pseudo.join("app.po"), "").unwrap();

        let report = count(&dir, &["app", "ui"]).unwrap();
        assert_eq!(
            report.languages,
            [(
                "sv".to_owned(),
                vec![
                    Completion {
                        domain: "app",
                        translated: 1,
                        total: 3,
                    },
                    Completion {
                        domain: "ui",
                        translated: 0,
                        total: 0,
                    },
                ]
            )]
        );
        assert_eq!(
            report.visited,
            [
                dir.join("app.pot"),
                dir.join("ui.pot"),
                sv.join("app.po"),
                sv.join("ui.po"),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
