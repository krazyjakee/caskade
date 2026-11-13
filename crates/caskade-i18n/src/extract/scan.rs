//! Reading a source's translation macro calls.

use super::ExtractError;
use crate::po::Message;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::collections::HashMap;
use std::str::FromStr;

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
pub(super) struct Kind {
    context: bool,
    plural: bool,
}

impl Kind {
    const fn new(context: bool, plural: bool) -> Self {
        Self { context, plural }
    }
}

pub(super) const NATIVE_KEYWORDS: &[(&str, Kind)] = &[
    ("t", Kind::new(false, false)),
    ("tn", Kind::new(false, true)),
    ("tp", Kind::new(true, false)),
    ("tnp", Kind::new(true, true)),
    ("trans", Kind::new(false, false)),
    ("transp", Kind::new(true, false)),
    // English marked where it's kept and translated where it's shown
    // (`caskade_ui_data::labels`), as gettext's `N_`.
    ("t_noop", Kind::new(false, false)),
];

pub(super) struct Messages {
    keywords: &'static [(&'static str, Kind)],
    /// Leaves `#[cfg(test)] mod`s out: test data (the native UI's).
    pub(super) skip_tests: bool,
    pub(super) list: Vec<Message>,
    index: HashMap<(Option<String>, String), usize>,
}

impl Default for Messages {
    fn default() -> Self {
        Self::with(KEYWORDS)
    }
}

impl Messages {
    pub(super) fn with(keywords: &'static [(&'static str, Kind)]) -> Self {
        Self {
            keywords,
            skip_tests: false,
            list: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub(super) fn scan(&mut self, text: &str, file: &str) -> Result<(), ExtractError> {
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
                ) if equals.as_char() == '=' && comma.as_char() == ',' => literal_text(&literal),
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
                Some(TokenTree::Literal(literal)) => literal_text(&literal),
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

/// Whether `trees[..at]` ends with `#[cfg(test)] mod name`: the group at
/// `at` is a test module, whose text is test data.
pub(super) fn test_module(trees: &[TokenTree], at: usize) -> bool {
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

/// A string literal's value.
pub(super) fn literal_text(literal: &proc_macro2::Literal) -> Option<String> {
    syn::parse_str::<syn::LitStr>(&literal.to_string())
        .ok()
        .map(|literal| literal.value())
}
