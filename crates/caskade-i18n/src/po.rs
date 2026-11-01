//! Reading and writing PO and POT files (the GNU gettext text format).
//!
//! The reader keeps what translators and tools write around a message
//! (comments, references, flags) so a catalog survives a merge; obsolete
//! (`#~`) entries are dropped. The writer is canonical: writing what it
//! read gives the same text back, so extraction twice changes nothing.

use crate::plural::{PluralError, PluralForms};
use std::collections::HashSet;
use std::fmt::{self, Write as _};

/// One message: its context, source text and translations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    /// `# …` lines, written by translators.
    pub translator_comments: Vec<String>,
    /// `#. …` lines, written by the extractor for translators.
    pub extracted_comments: Vec<String>,
    /// `#: …` source files the message comes from.
    pub references: Vec<String>,
    /// `#, …` flags, such as `fuzzy`.
    pub flags: Vec<String>,
    /// `#| …` lines: the previous source text of a fuzzy message.
    pub previous: Vec<String>,
    /// `msgctxt`: tells apart the same English with different meanings.
    pub context: Option<String>,
    /// `msgid`: the English text.
    pub id: String,
    /// `msgid_plural`: the English plural, for messages that count.
    pub plural: Option<String>,
    /// `msgstr`, or `msgstr[0]`, `msgstr[1]`, … for plural messages. Empty
    /// strings are untranslated.
    pub translations: Vec<String>,
}

impl Message {
    /// A message without translations, as a template has it.
    pub fn new(context: Option<&str>, id: &str, plural: Option<&str>) -> Self {
        Self {
            context: context.map(str::to_owned),
            id: id.to_owned(),
            plural: plural.map(str::to_owned),
            translations: vec![String::new(); if plural.is_some() { 2 } else { 1 }],
            ..Self::default()
        }
    }

    /// Marked `fuzzy`: a guess no translator has confirmed, shown in English.
    pub fn is_fuzzy(&self) -> bool {
        self.flags.iter().any(|flag| flag == "fuzzy")
    }

    /// The context and English text, which identify a message.
    pub fn key(&self) -> (Option<&str>, &str) {
        (self.context.as_deref(), &self.id)
    }

    fn is_header(&self) -> bool {
        self.context.is_none() && self.id.is_empty()
    }
}

/// A PO catalog or POT template.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Po {
    /// The entry with an empty `msgid`: `Language`, `Plural-Forms`, …
    pub header: Option<Message>,
    pub messages: Vec<Message>,
}

/// Why a PO file couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

impl Po {
    /// Reads PO text.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        Parser::default().parse(text)
    }

    /// A header field, such as `Plural-Forms` or `Language`.
    pub fn header_field(&self, name: &str) -> Option<&str> {
        let header = self.header.as_ref()?.translations.first()?;
        header.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
        })
    }

    /// The `Plural-Forms` header's rule, or English's without one.
    pub fn plural_forms(&self) -> Result<PluralForms, PluralError> {
        self.header_field("Plural-Forms")
            .map_or_else(|| Ok(PluralForms::default()), PluralForms::parse)
    }

    /// The PO text, in a canonical layout.
    pub fn write(&self) -> String {
        let mut out = String::new();
        for (index, message) in self.header.iter().chain(&self.messages).enumerate() {
            if index > 0 {
                out.push('\n');
            }
            write_message(&mut out, message);
        }
        out
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Context,
    Id,
    Plural,
    Translation(usize),
}

#[derive(Default)]
struct Parser {
    po: Po,
    keys: HashSet<(Option<String>, String)>,
    current: Message,
    /// Line of the current entry's first keyword.
    start: usize,
    has_id: bool,
    has_keyword: bool,
    obsolete: bool,
    /// What a continuation line (`"…"`) appends to.
    field: Option<Field>,
}

impl Parser {
    fn parse(mut self, text: &str) -> Result<Po, ParseError> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut last = 0;
        for (index, raw) in text.lines().enumerate() {
            let line_no = index + 1;
            last = line_no;
            let line = raw.trim();
            if line.is_empty() {
                self.finish(line_no)?;
                continue;
            }
            if let Some(rest) = line.strip_prefix('#') {
                if self.has_keyword {
                    self.finish(line_no)?;
                }
                self.comment(rest);
                continue;
            }
            if line.starts_with('"') {
                let value = unquote(line).map_err(|message| error(line_no, message))?;
                self.append(&value, line_no)?;
                continue;
            }
            let (keyword, rest) = line.split_once(char::is_whitespace).ok_or_else(|| {
                error(line_no, format!("expected a keyword and a string: {line}"))
            })?;
            let value = unquote(rest.trim()).map_err(|message| error(line_no, message))?;
            let field = match keyword {
                "msgctxt" => Field::Context,
                "msgid" => Field::Id,
                "msgid_plural" => Field::Plural,
                "msgstr" => Field::Translation(0),
                _ => match keyword
                    .strip_prefix("msgstr[")
                    .and_then(|rest| rest.strip_suffix(']'))
                    .and_then(|index| index.parse().ok())
                {
                    Some(index) => Field::Translation(index),
                    None => return Err(error(line_no, format!("unknown keyword {keyword}"))),
                },
            };
            self.keyword(field, keyword, value, line_no)?;
        }
        self.finish(last + 1)?;
        Ok(self.po)
    }

    fn comment(&mut self, rest: &str) {
        let (kind, text) = match rest.chars().next() {
            Some(kind @ ('.' | ':' | ',' | '|' | '~')) => (Some(kind), &rest[1..]),
            _ => (None, rest),
        };
        let text = text.strip_prefix(' ').unwrap_or(text).trim_end();
        let message = &mut self.current;
        match kind {
            None => message.translator_comments.push(text.to_owned()),
            Some('.') => message.extracted_comments.push(text.to_owned()),
            Some(':') => message
                .references
                .extend(text.split_whitespace().map(str::to_owned)),
            Some(',') => message.flags.extend(
                text.split(',')
                    .map(str::trim)
                    .filter(|flag| !flag.is_empty())
                    .map(str::to_owned),
            ),
            Some('|') => message.previous.push(text.to_owned()),
            _ => self.obsolete = true,
        }
    }

    fn keyword(
        &mut self,
        field: Field,
        keyword: &str,
        value: String,
        line_no: usize,
    ) -> Result<(), ParseError> {
        // A new entry may follow the last without a blank line.
        if self.has_id && matches!(field, Field::Context | Field::Id) {
            self.finish(line_no)?;
        }
        if !self.has_keyword {
            self.start = line_no;
        }
        self.has_keyword = true;
        let message = &mut self.current;
        match field {
            Field::Context if message.context.is_none() => message.context = Some(value),
            Field::Id if !self.has_id => {
                message.id = value;
                self.has_id = true;
            }
            Field::Plural if self.has_id && message.plural.is_none() => {
                message.plural = Some(value);
            }
            Field::Translation(index) if self.has_id && index == message.translations.len() => {
                let indexed = keyword != "msgstr";
                if indexed != message.plural.is_some() {
                    return Err(error(
                        line_no,
                        if indexed {
                            "msgstr[n] belongs to a message with msgid_plural".to_owned()
                        } else {
                            "a message with msgid_plural takes msgstr[n]".to_owned()
                        },
                    ));
                }
                message.translations.push(value);
            }
            _ => return Err(error(line_no, format!("{keyword} out of place"))),
        }
        self.field = Some(field);
        Ok(())
    }

    fn append(&mut self, value: &str, line_no: usize) -> Result<(), ParseError> {
        let message = &mut self.current;
        let target = match self.field {
            Some(Field::Context) => message.context.as_mut(),
            Some(Field::Id) => Some(&mut message.id),
            Some(Field::Plural) => message.plural.as_mut(),
            Some(Field::Translation(index)) => message.translations.get_mut(index),
            None => None,
        };
        match target {
            Some(target) => {
                target.push_str(value);
                Ok(())
            }
            None => Err(error(line_no, "a string outside a message".to_owned())),
        }
    }

    /// Ends the current entry: a message, the header, or comments without
    /// one (dropped, as are obsolete entries).
    fn finish(&mut self, line_no: usize) -> Result<(), ParseError> {
        let message = std::mem::take(&mut self.current);
        let (has_id, has_keyword, obsolete) = (self.has_id, self.has_keyword, self.obsolete);
        self.has_id = false;
        self.has_keyword = false;
        self.obsolete = false;
        self.field = None;
        if obsolete || !has_keyword {
            return Ok(());
        }
        if !has_id {
            return Err(error(self.start, "a message without msgid".to_owned()));
        }
        if message.translations.is_empty() {
            return Err(error(
                line_no.max(self.start),
                "a message without msgstr".to_owned(),
            ));
        }
        if message.is_header() {
            if self.po.header.is_some() || !self.po.messages.is_empty() {
                return Err(error(
                    self.start,
                    "an empty msgid after the header".to_owned(),
                ));
            }
            self.po.header = Some(message);
            return Ok(());
        }
        let key = (message.context.clone(), message.id.clone());
        if !self.keys.insert(key) {
            return Err(error(
                self.start,
                format!("duplicate message {:?}", message.id),
            ));
        }
        self.po.messages.push(message);
        Ok(())
    }
}

fn error(line: usize, message: String) -> ParseError {
    ParseError { line, message }
}

/// The contents of a `"…"` string, unescaped.
fn unquote(quoted: &str) -> Result<String, String> {
    let inner = quoted
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .filter(|_| quoted.len() >= 2)
        .ok_or_else(|| format!("expected a quoted string: {quoted}"))?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(match chars.next() {
                Some('n') => '\n',
                Some('t') => '\t',
                Some('r') => '\r',
                Some('a') => '\u{7}',
                Some('b') => '\u{8}',
                Some('f') => '\u{c}',
                Some('v') => '\u{b}',
                Some(c @ ('\\' | '"' | '\'' | '?')) => c,
                other => return Err(format!("unknown escape \\{}", other.unwrap_or(' '))),
            }),
            '"' => return Err(format!("an unescaped quote in {quoted}")),
            c => out.push(c),
        }
    }
    Ok(out)
}

fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn write_message(out: &mut String, message: &Message) {
    for comment in &message.translator_comments {
        if comment.is_empty() {
            out.push_str("#\n");
        } else {
            let _ = writeln!(out, "# {comment}");
        }
    }
    for comment in &message.extracted_comments {
        let _ = writeln!(out, "#. {comment}");
    }
    for reference in &message.references {
        let _ = writeln!(out, "#: {reference}");
    }
    if !message.flags.is_empty() {
        let _ = writeln!(out, "#, {}", message.flags.join(", "));
    }
    for previous in &message.previous {
        let _ = writeln!(out, "#| {previous}");
    }
    if let Some(context) = &message.context {
        write_field(out, "msgctxt", context);
    }
    write_field(out, "msgid", &message.id);
    match &message.plural {
        Some(plural) => {
            write_field(out, "msgid_plural", plural);
            for (index, translation) in message.translations.iter().enumerate() {
                write_field(out, &format!("msgstr[{index}]"), translation);
            }
        }
        None => write_field(
            out,
            "msgstr",
            message.translations.first().map_or("", String::as_str),
        ),
    }
}

/// `keyword "value"`, or one line per `\n`-ended part when there are
/// several, as gettext's tools write them.
fn write_field(out: &mut String, keyword: &str, value: &str) {
    let parts: Vec<&str> = value.split_inclusive('\n').collect();
    if parts.len() > 1 {
        let _ = writeln!(out, "{keyword} \"\"");
        for part in parts {
            let _ = writeln!(out, "{}", quote(part));
        }
    } else {
        let _ = writeln!(out, "{keyword} {}", quote(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# Swedish translations.
msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

# A translator's note.
#. For translators.
#: crates/a.rs
#: crates/b.rs
msgid "Show Caskade"
msgstr "Visa Caskade"

#, fuzzy
msgctxt "folder"
msgid "Home"
msgstr "Hem"

msgid "One game"
msgid_plural "{n} games"
msgstr[0] "Ett spel"
msgstr[1] "{n} spel"

msgid "Two\n"
"lines"
msgstr ""
"Två\n"
"rader"

#~ msgid "Gone"
#~ msgstr "Borta"
"#;

    #[test]
    fn reads_messages_comments_flags_and_plurals() {
        let po = Po::parse(SAMPLE).unwrap();
        assert_eq!(po.header_field("language"), Some("sv"));
        assert_eq!(
            po.header_field("Plural-Forms"),
            Some("nplurals=2; plural=(n != 1);")
        );
        assert_eq!(po.messages.len(), 4);
        let show = &po.messages[0];
        assert_eq!(show.translator_comments, ["A translator's note."]);
        assert_eq!(show.extracted_comments, ["For translators."]);
        assert_eq!(show.references, ["crates/a.rs", "crates/b.rs"]);
        assert_eq!(show.translations, ["Visa Caskade"]);
        let home = &po.messages[1];
        assert!(home.is_fuzzy());
        assert_eq!(home.key(), (Some("folder"), "Home"));
        let games = &po.messages[2];
        assert_eq!(games.plural.as_deref(), Some("{n} games"));
        assert_eq!(games.translations, ["Ett spel", "{n} spel"]);
        assert_eq!(po.messages[3].id, "Two\nlines");
        assert_eq!(po.messages[3].translations, ["Två\nrader"]);
    }

    #[test]
    fn writing_what_was_read_gives_the_same_text() {
        let po = Po::parse(SAMPLE).unwrap();
        let written = po.write();
        assert_eq!(Po::parse(&written).unwrap(), po);
        assert_eq!(Po::parse(&written).unwrap().write(), written);
        // Obsolete entries are dropped.
        assert!(!written.contains("Gone"));
    }

    #[test]
    fn escapes_round_trip() {
        let mut po = Po::default();
        po.messages.push(Message {
            translations: vec!["\"quoted\"\t\\ back".to_owned()],
            ..Message::new(None, "\"quoted\"\t\\ back", None)
        });
        assert_eq!(Po::parse(&po.write()).unwrap(), po);
    }

    #[test]
    fn rejects_broken_files() {
        let cases = [
            (
                "msgid \"a\"\nmsgstr \"b\"\n\nmsgid \"a\"\nmsgstr \"c\"\n",
                "duplicate",
            ),
            ("msgid \"a\"\n", "without msgstr"),
            ("msgid \"a\"\nmsgstr[0] \"b\"\n", "msgid_plural"),
            (
                "msgid \"a\"\nmsgid_plural \"b\"\nmsgstr \"c\"\n",
                "msgstr[n]",
            ),
            ("msgid \"a\nmsgstr \"b\"\n", "quoted"),
            ("msgid \"a\"\nmsgstr \"\\q\"\n", "escape"),
            ("msgid \"a\"\nmsgstr \"b\"\nmsgstr \"c\"\n", "out of place"),
            ("msgfoo \"a\"\n", "unknown keyword"),
            ("\"a\"\n", "outside a message"),
        ];
        for (text, expected) in cases {
            let error = Po::parse(text).unwrap_err();
            assert!(error.message.contains(expected), "{text:?}: {error}");
        }
    }

    #[test]
    fn entries_need_no_blank_line_between_them() {
        let po = Po::parse("msgid \"a\"\nmsgstr \"b\"\nmsgid \"c\"\nmsgstr \"d\"\n").unwrap();
        assert_eq!(po.messages.len(), 2);
    }
}
