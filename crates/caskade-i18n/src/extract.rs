//! Extraction: finds the `tr!`, `trn!`, `trp!` and `trnp!` calls (and
//! their stored-message forms `msg!`, `msgn!`, `msgp!` and `msgnp!`) in a
//! domain's Rust sources, writes its POT template and merges the template
//! into each language's PO catalog, like `xgettext` and `msgmerge`, without
//! gettext's tools.
//!
//! Sources are read as Rust tokens, so calls inside other macros'
//! arguments (`format!`, `tracing::warn!`) count, comments don't, and
//! string literals (raw, escaped) unescape as the compiler does. The output
//! is deterministic: references name files without line numbers, and the
//! template has no creation date, so extracting twice changes nothing and
//! moving code around doesn't make the template stale.

use crate::po::{Message, Po};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

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
}

/// The core's domain: the Rust crates, the Tauri adapter, the native app
/// and the tray.
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
        "apps/desktop/src-tauri/src",
        "apps/native/src",
    ],
    title: "Caskade's core messages: the Rust crates, the Tauri adapter, the native app and the tray.",
};

/// The macros extraction looks for, and the string arguments each takes
/// first.
const KEYWORDS: &[(&str, Kind)] = &[
    ("tr", Kind::new(false, false)),
    ("trn", Kind::new(false, true)),
    ("trp", Kind::new(true, false)),
    ("trnp", Kind::new(true, true)),
    // Messages kept to translate later (`caskade_core::i18n::Message`).
    ("msg", Kind::new(false, false)),
    ("msgn", Kind::new(false, true)),
    ("msgp", Kind::new(true, false)),
    ("msgnp", Kind::new(true, true)),
];

/// Whether a macro's first strings include a context and a plural.
#[derive(Debug, Clone, Copy)]
struct Kind {
    context: bool,
    plural: bool,
}

impl Kind {
    const fn new(context: bool, plural: bool) -> Self {
        Self { context, plural }
    }
}

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

impl Domain {
    /// `locales/<name>.pot`.
    pub fn template_path(&self, root: &Path) -> PathBuf {
        root.join("locales").join(format!("{}.pot", self.name))
    }

    /// The domain's catalogs, `locales/<language>/<name>.po`, by language.
    pub fn catalog_paths(&self, root: &Path) -> Result<Vec<PathBuf>, ExtractError> {
        let locales = root.join("locales");
        let mut paths = Vec::new();
        for entry in sorted_entries(&locales)? {
            let path = entry.join(format!("{}.po", self.name));
            if entry.is_dir() && path.is_file() {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    /// The template, from the domain's sources under `root`.
    pub fn template(&self, root: &Path) -> Result<Po, ExtractError> {
        let mut messages = Messages::default();
        for source in self.sources {
            let mut files = Vec::new();
            rust_files(&root.join(source), &mut files)?;
            for file in files {
                let text = std::fs::read_to_string(&file)
                    .map_err(|error| ExtractError::Io(file.clone(), error))?;
                let reference = file
                    .strip_prefix(root)
                    .unwrap_or(&file)
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                messages.scan(&text, &reference)?;
            }
        }
        let mut header = Message::new(None, "", None);
        header.translator_comments = vec![
            self.title.to_owned(),
            "Generated by `just i18n` from the tr!, trn!, trp! and trnp! calls; don't edit."
                .to_owned(),
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
            messages: messages.list,
        })
    }

    /// Everything extraction writes: the template, and each catalog merged
    /// with it.
    pub fn outputs(&self, root: &Path) -> Result<Vec<Output>, ExtractError> {
        let template = self.template(root)?;
        let mut outputs = vec![Output {
            path: self.template_path(root),
            contents: template.write(),
        }];
        for path in self.catalog_paths(root)? {
            let text = std::fs::read_to_string(&path)
                .map_err(|error| ExtractError::Io(path.clone(), error))?;
            let catalog = Po::parse(&text)
                .map_err(|error| ExtractError::Catalog(path.clone(), error.to_string()))?;
            let merged = merge(&template, &catalog)
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
/// matching: translations of messages still in the template stay (marked
/// fuzzy when the English plural changed), new messages come in
/// untranslated, and messages gone from the template go.
pub fn merge(template: &Po, catalog: &Po) -> Result<Po, String> {
    let forms = catalog.plural_forms().map_err(|error| error.to_string())?;
    let existing: HashMap<(Option<&str>, &str), &Message> = catalog
        .messages
        .iter()
        .map(|message| (message.key(), message))
        .collect();
    let messages = template
        .messages
        .iter()
        .map(|source| {
            let mut merged = match existing.get(&source.key()) {
                Some(old) if old.plural.is_some() == source.plural.is_some() => {
                    let mut merged = (*old).clone();
                    if old.plural != source.plural && !merged.is_fuzzy() {
                        merged.flags.push("fuzzy".to_owned());
                    }
                    merged
                }
                _ => {
                    let mut new = source.clone();
                    if source.plural.is_some() {
                        new.translations = vec![String::new(); forms.count];
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
    Ok(Po {
        header: catalog.header.clone().or_else(|| template.header.clone()),
        messages,
    })
}

#[derive(Default)]
struct Messages {
    list: Vec<Message>,
    index: HashMap<(Option<String>, String), usize>,
}

impl Messages {
    fn scan(&mut self, text: &str, file: &str) -> Result<(), ExtractError> {
        let tokens = TokenStream::from_str(text)
            .map_err(|error| ExtractError::Source(file.to_owned(), error.to_string()))?;
        self.visit(tokens, file)
    }

    fn visit(&mut self, tokens: TokenStream, file: &str) -> Result<(), ExtractError> {
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        for (at, tree) in trees.iter().enumerate() {
            let TokenTree::Group(group) = tree else {
                continue;
            };
            if let [.., TokenTree::Ident(name), TokenTree::Punct(bang)] = &trees[..at]
                && bang.as_char() == '!'
                && group.delimiter() != Delimiter::None
                && let Some((keyword, kind)) = KEYWORDS.iter().find(|(keyword, _)| name == keyword)
            {
                self.call(keyword, *kind, group.stream(), file)?;
            }
            // Calls nest: `tr!("{a}", a = tr!("b"))`.
            self.visit(group.stream(), file)?;
        }
        Ok(())
    }

    fn call(
        &mut self,
        keyword: &str,
        kind: Kind,
        args: TokenStream,
        file: &str,
    ) -> Result<(), ExtractError> {
        let wanted = 1 + usize::from(kind.context) + usize::from(kind.plural);
        let mut strings = Vec::with_capacity(wanted);
        let mut tokens = args.into_iter();
        while strings.len() < wanted {
            let literal = match tokens.next() {
                Some(TokenTree::Literal(literal)) => {
                    syn::parse_str::<syn::LitStr>(&literal.to_string())
                        .ok()
                        .map(|literal| literal.value())
                }
                _ => None,
            };
            let Some(literal) = literal else {
                return Err(ExtractError::Source(
                    file.to_owned(),
                    format!("{keyword}! takes {wanted} string literal(s) first"),
                ));
            };
            strings.push(literal);
            match tokens.next() {
                Some(TokenTree::Punct(comma)) if comma.as_char() == ',' => {}
                None if strings.len() == wanted => {}
                _ => {
                    return Err(ExtractError::Source(
                        file.to_owned(),
                        format!("{keyword}! takes {wanted} string literal(s) first"),
                    ));
                }
            }
        }
        let mut strings = strings.into_iter();
        let context = kind.context.then(|| strings.next()).flatten();
        let id = strings.next().unwrap_or_default();
        let plural = kind.plural.then(|| strings.next()).flatten();
        if id.is_empty() {
            return Err(ExtractError::Source(
                file.to_owned(),
                format!("{keyword}! with an empty message"),
            ));
        }
        self.add(context, id, plural, file)
    }

    fn add(
        &mut self,
        context: Option<String>,
        id: String,
        plural: Option<String>,
        file: &str,
    ) -> Result<(), ExtractError> {
        let key = (context, id);
        if let Some(&at) = self.index.get(&key) {
            let message = &mut self.list[at];
            if message.plural != plural {
                return Err(ExtractError::Source(
                    file.to_owned(),
                    format!(
                        "{:?} is marked with different plurals; give one a context",
                        key.1
                    ),
                ));
            }
            if !message.references.iter().any(|known| known == file) {
                message.references.push(file.to_owned());
            }
            return Ok(());
        }
        let mut message = Message::new(key.0.as_deref(), &key.1, plural.as_deref());
        message.references.push(file.to_owned());
        self.index.insert(key, self.list.len());
        self.list.push(message);
        Ok(())
    }
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
            header: None,
            messages: messages.list,
        })
    }

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
}
