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
        "crates/caskade-ui-theme/src",
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

struct Messages {
    keywords: &'static [(&'static str, Kind)],
    /// Leaves `#[cfg(test)] mod`s out: test data (the native UI's).
    skip_tests: bool,
    list: Vec<Message>,
    index: HashMap<(Option<String>, String), usize>,
}

impl Default for Messages {
    fn default() -> Self {
        Self::with(KEYWORDS)
    }
}

impl Messages {
    fn with(keywords: &'static [(&'static str, Kind)]) -> Self {
        Self {
            keywords,
            skip_tests: false,
            list: Vec::new(),
            index: HashMap::new(),
        }
    }

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
            if self.skip_tests && test_module(&trees, at) {
                continue;
            }
            // A macro's definition (`macro_rules! t { … }`) isn't a call.
            if let [
                ..,
                TokenTree::Ident(rules),
                TokenTree::Punct(bang),
                TokenTree::Ident(_),
            ] = &trees[..at]
                && rules == "macro_rules"
                && bang.as_char() == '!'
            {
                continue;
            }
            if let [.., TokenTree::Ident(name), TokenTree::Punct(bang)] = &trees[..at]
                && bang.as_char() == '!'
                && group.delimiter() != Delimiter::None
                && let Some((keyword, kind)) =
                    self.keywords.iter().find(|(keyword, _)| name == keyword)
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
        let mut tokens = args.into_iter().peekable();
        // The native UI's macros take a translator note first:
        // `t!(note = "…", "Open")`.
        let mut notes = Vec::new();
        if let Some(TokenTree::Ident(ident)) = tokens.peek()
            && ident == "note"
        {
            tokens.next();
            let literal = match (tokens.next(), tokens.next(), tokens.next()) {
                (
                    Some(TokenTree::Punct(equals)),
                    Some(TokenTree::Literal(literal)),
                    Some(TokenTree::Punct(comma)),
                ) if equals.as_char() == '=' && comma.as_char() == ',' => {
                    syn::parse_str::<syn::LitStr>(&literal.to_string())
                        .ok()
                        .map(|literal| literal.value())
                }
                _ => None,
            };
            let Some(note) = literal else {
                return Err(ExtractError::Source(
                    file.to_owned(),
                    format!("{keyword}!'s note is `note = \"…\",` before the message"),
                ));
            };
            notes.push(note);
        }
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
                // `trans!`'s elements follow a `;`.
                Some(TokenTree::Punct(semi))
                    if semi.as_char() == ';' && strings.len() == wanted => {}
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
        let at = self.add(context, id, plural, file)?;
        let comments = &mut self.list[at].extracted_comments;
        for note in notes {
            if !comments.contains(&note) {
                comments.push(note);
            }
        }
        Ok(())
    }

    fn add(
        &mut self,
        context: Option<String>,
        id: String,
        plural: Option<String>,
        file: &str,
    ) -> Result<usize, ExtractError> {
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
            return Ok(at);
        }
        let mut message = Message::new(key.0.as_deref(), &key.1, plural.as_deref());
        message.references.push(file.to_owned());
        let at = self.list.len();
        self.index.insert(key, at);
        self.list.push(message);
        Ok(at)
    }
}

/// The native UI's sources. Their `t!`, `tn!`, `tp!`, `tnp!`, `trans!` and
/// `transp!` messages share the webview's `app` and `ui` domains (the same
/// English and contexts, so the same translations), and go into those
/// templates through `locales/native.json`: `packages/i18n` writes the
/// templates until the webview goes (BLITZ-33), adding the native UI's
/// messages to Lingui's. (Their `tr!` family stays in [`CORE`].)
pub const NATIVE_SOURCES: &[&str] = &["apps/native/src", "crates/caskade-ui-i18n/src"];

const NATIVE_KEYWORDS: &[(&str, Kind)] = &[
    ("t", Kind::new(false, false)),
    ("tn", Kind::new(false, true)),
    ("tp", Kind::new(true, false)),
    ("tnp", Kind::new(true, true)),
    ("trans", Kind::new(false, false)),
    ("transp", Kind::new(true, false)),
];

/// Where the native UI's messages are listed for `packages/i18n`.
pub fn native_path(root: &Path) -> PathBuf {
    root.join("locales").join("native.json")
}

/// The native UI's messages, by English and context. Test files aren't
/// scanned: their text is test data.
pub fn native_messages(root: &Path) -> Result<Vec<Message>, ExtractError> {
    let mut messages = Messages::with(NATIVE_KEYWORDS);
    messages.skip_tests = true;
    for source in NATIVE_SOURCES {
        let mut files = Vec::new();
        rust_files(&root.join(source), &mut files)?;
        for file in files {
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name == "tests.rs" || name.ends_with("_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&file)
                .map_err(|error| ExtractError::Io(file.clone(), error))?;
            messages.scan(&text, &reference(root, &file))?;
        }
    }
    let mut list = messages.list;
    list.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.context.cmp(&b.context)));
    Ok(list)
}

fn reference(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// `locales/native.json`: the native UI's messages for `packages/i18n`'s
/// extraction, which adds them to the `app` and `ui` templates.
pub fn native_output(root: &Path) -> Result<Output, ExtractError> {
    let messages = native_messages(root)?;
    let mut json = String::from(
        "{\n  \"comment\": \"The native UI's messages, generated by `just i18n`; don't edit. packages/i18n adds them to the app and ui templates.\",\n  \"messages\": [",
    );
    for (at, message) in messages.iter().enumerate() {
        json.push_str(if at == 0 { "\n" } else { ",\n" });
        json.push_str("    {");
        let mut fields = vec![format!("\"id\": {}", json_string(&message.id))];
        if let Some(context) = &message.context {
            fields.push(format!("\"context\": {}", json_string(context)));
        }
        if let Some(plural) = &message.plural {
            fields.push(format!("\"plural\": {}", json_string(plural)));
        }
        if !message.extracted_comments.is_empty() {
            fields.push(format!(
                "\"comments\": {}",
                json_list(&message.extracted_comments)
            ));
        }
        fields.push(format!(
            "\"references\": {}",
            json_list(&message.references)
        ));
        json.push_str(&fields.join(", "));
        json.push('}');
    }
    json.push_str(if messages.is_empty() {
        "]\n}\n"
    } else {
        "\n  ]\n}\n"
    });
    Ok(Output {
        path: native_path(root),
        contents: json,
    })
}

fn json_list(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|item| json_string(item)).collect();
    format!("[{}]", items.join(", "))
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Attributes whose text people read (or hear): a literal there must be a
/// translated message.
const SHOWN_ATTRIBUTES: &[&str] = &[
    "aria_label",
    "aria-label",
    "aria_description",
    "aria-description",
    "aria_placeholder",
    "aria-placeholder",
    "aria_roledescription",
    "aria-roledescription",
    "aria_valuetext",
    "aria-valuetext",
    "title",
    "placeholder",
    "alt",
    "label",
];

/// Whether `text` has words: letters outside its `{placeholders}`. A
/// placeholder or a symbol (`{label}`, `⇧`) needs no translation.
fn has_words(text: &str) -> bool {
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            c if depth == 0 && c.is_alphabetic() => return true,
            _ => {}
        }
    }
    false
}

/// The string literals people would see in `rsx!` calls in `text`: text
/// nodes, and the values of [`SHOWN_ATTRIBUTES`], that have words. Text in
/// a translation macro's call is a message, not a literal, so it isn't
/// listed.
pub fn rsx_literals(text: &str) -> Result<Vec<String>, String> {
    let tokens = TokenStream::from_str(text).map_err(|error| error.to_string())?;
    let mut found = Vec::new();
    find_rsx(tokens, &mut found);
    Ok(found)
}

/// Whether `trees[..at]` ends with `#[cfg(test)] mod name`: the group at
/// `at` is a test module, whose text is test data.
fn test_module(trees: &[TokenTree], at: usize) -> bool {
    let [
        ..,
        TokenTree::Punct(hash),
        TokenTree::Group(attribute),
        TokenTree::Ident(keyword),
        TokenTree::Ident(_),
    ] = &trees[..at]
    else {
        return false;
    };
    hash.as_char() == '#'
        && keyword == "mod"
        && attribute.stream().to_string().replace(' ', "") == "cfg(test)"
}

fn find_rsx(tokens: TokenStream, found: &mut Vec<String>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        let TokenTree::Group(group) = tree else {
            continue;
        };
        if test_module(&trees, at) {
            continue;
        }
        if let [.., TokenTree::Ident(name), TokenTree::Punct(bang)] = &trees[..at]
            && name == "rsx"
            && bang.as_char() == '!'
        {
            rsx_body(group.stream(), found);
            continue;
        }
        find_rsx(group.stream(), found);
    }
}

fn literal_text(literal: &proc_macro2::Literal) -> Option<String> {
    syn::parse_str::<syn::LitStr>(&literal.to_string())
        .ok()
        .map(|literal| literal.value())
}

/// One `rsx!` body (and the element bodies in it).
fn rsx_body(tokens: TokenStream, found: &mut Vec<String>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    // The attribute whose value the tokens are in, from its `:` to the
    // next `,`.
    let mut value_of: Option<String> = None;
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Punct(punct) if punct.as_char() == ',' => value_of = None,
            TokenTree::Punct(punct) if punct.as_char() == ':' => {
                value_of = match at.checked_sub(1).map(|at| &trees[at]) {
                    Some(TokenTree::Ident(name)) => Some(name.to_string()),
                    Some(TokenTree::Literal(name)) => literal_text(name),
                    _ => value_of.take(),
                };
            }
            TokenTree::Literal(literal) => {
                let Some(text) = literal_text(literal) else {
                    continue;
                };
                // An attribute's name (`"data-cs": …`).
                if matches!(trees.get(at + 1), Some(TokenTree::Punct(colon)) if colon.as_char() == ':')
                {
                    continue;
                }
                let shown = match &value_of {
                    Some(name) => SHOWN_ATTRIBUTES.contains(&name.as_str()),
                    // A text node: first, or after another child.
                    None => match at.checked_sub(1).map(|at| &trees[at]) {
                        None | Some(TokenTree::Group(_) | TokenTree::Literal(_)) => true,
                        Some(TokenTree::Punct(comma)) => comma.as_char() == ',',
                        Some(TokenTree::Ident(_)) => false,
                    },
                };
                if shown && has_words(&text) {
                    found.push(text);
                }
            }
            TokenTree::Group(group) => {
                let after_bang = matches!(
                    at.checked_sub(1).map(|at| &trees[at]),
                    Some(TokenTree::Punct(bang)) if bang.as_char() == '!'
                );
                let closure = matches!(
                    at.checked_sub(1).map(|at| &trees[at]),
                    Some(TokenTree::Punct(bar)) if bar.as_char() == '|'
                );
                if let Some(name) = &value_of {
                    // An attribute's value: shown text if the attribute is
                    // (`title: if a { "One" } else { "Other" }`).
                    if SHOWN_ATTRIBUTES.contains(&name.as_str()) && !after_bang {
                        value_literals(group.stream(), found);
                    } else {
                        find_rsx(group.stream(), found);
                    }
                } else if after_bang || closure || group.delimiter() == Delimiter::Parenthesis {
                    // A macro's call (a translation macro's text is a
                    // message), a closure or a call's arguments: code.
                    find_rsx(group.stream(), found);
                } else {
                    // An element's, an `if`'s or a `for`'s body.
                    rsx_body(group.stream(), found);
                }
            }
            _ => {}
        }
    }
}

/// The literals with words in a shown attribute's value, outside macro
/// calls.
fn value_literals(tokens: TokenStream, found: &mut Vec<String>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Literal(literal) => {
                if let Some(text) = literal_text(literal)
                    && has_words(&text)
                {
                    found.push(text);
                }
            }
            TokenTree::Group(group) => {
                if matches!(
                    at.checked_sub(1).map(|at| &trees[at]),
                    Some(TokenTree::Punct(bang)) if bang.as_char() == '!'
                ) {
                    find_rsx(group.stream(), found);
                } else {
                    value_literals(group.stream(), found);
                }
            }
            _ => {}
        }
    }
}

/// Every `.rs` file under `sources` (relative to `root`) but tests, with
/// the user-visible string literals in its `rsx!` calls.
pub fn untranslated_rsx(
    root: &Path,
    sources: &[&str],
) -> Result<Vec<(String, String)>, ExtractError> {
    let mut found = Vec::new();
    for source in sources {
        let mut files = Vec::new();
        rust_files(&root.join(source), &mut files)?;
        for file in files {
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name == "tests.rs" || name.ends_with("_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&file)
                .map_err(|error| ExtractError::Io(file.clone(), error))?;
            let reference = reference(root, &file);
            let literals = rsx_literals(&text)
                .map_err(|error| ExtractError::Source(reference.clone(), error))?;
            found.extend(
                literals
                    .into_iter()
                    .map(|literal| (reference.clone(), literal)),
            );
        }
    }
    Ok(found)
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

#[cfg(test)]
mod native_tests {
    use super::*;

    #[test]
    fn finds_the_native_uis_messages_and_notes() {
        let mut messages = Messages::with(NATIVE_KEYWORDS);
        messages
            .scan(
                r##"
                macro_rules! t { ($($rest:tt)+) => { $crate::t!($($rest)+) }; }
                fn view(n: u64) {
                    let _ = t!("Quick menu");
                    let _ = t!(note = "A game's playtime, before any", "Not played");
                    let _ = tp!("size", "Unknown");
                    let _ = tn!("# game", "# games", n);
                    let _ = tnp!("library", "# file", "# files", n, extra = 1);
                    let _ = trans!("<0>{guide}</0> opens the quick menu", guide = g; 0 => |x| x);
                    let _ = tr!("A core message");
                }
                "##,
                "apps/native/src/a.rs",
            )
            .unwrap();
        let keys: Vec<_> = messages
            .list
            .iter()
            .map(|m| (m.context.as_deref(), m.id.as_str(), m.plural.as_deref()))
            .collect();
        assert_eq!(
            keys,
            [
                (None, "Quick menu", None),
                (None, "Not played", None),
                (Some("size"), "Unknown", None),
                (None, "# game", Some("# games")),
                (Some("library"), "# file", Some("# files")),
                (None, "<0>{guide}</0> opens the quick menu", None),
            ]
        );
        assert_eq!(
            messages.list[1].extracted_comments,
            ["A game's playtime, before any"]
        );
    }

    #[test]
    fn finds_rsx_text_that_isnt_translated() {
        let found = rsx_literals(
            r##"
            fn view() -> Element {
                rsx! {
                    div { "data-cs": "panel", class: "wide", style: "flex: 1;",
                        "Untranslated text"
                        h2 { "{title}" }
                        p { {t!("Translated")} }
                        button { title: "Untranslated title", aria_label: t!("Fine"), "⇧" }
                        img { alt: if big { "Big picture" } else { "{name}" } }
                        span { "data-focused": if on { "true" }, key: "{c}", onclick: move |_| log("clicked"), "{n} · {m}" }
                        if empty { "Nothing here" }
                        for item in items { li { "{item}" } }
                    }
                }
            }
            "##,
        )
        .unwrap();
        assert_eq!(
            found,
            [
                "Untranslated text",
                "Untranslated title",
                "Big picture",
                "Nothing here"
            ]
        );
    }
}
