//! Messages kept for later (#214): a stored notification keeps its message
//! id and arguments, not the text, and is translated each time it is read,
//! so a language switch changes earlier ones too.
//!
//! Make one with [`msg!`](crate::msg), [`msgn!`](crate::msgn),
//! [`msgp!`](crate::msgp) or [`msgnp!`](crate::msgnp), which take the same
//! arguments as the `tr!` macros and are extracted the same way. An
//! argument is plain text (a game's title, a cause from a dependency) or a
//! message of its own, translated with the rest.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Display;

/// A message translated when it is shown. Serializes to JSON for storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// The English text: the message id.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plural: Option<String>,
    /// The count that picks the plural form and fills `{n}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, Text>,
}

/// Text that is either shown as it is or translated: a message's argument,
/// or a stored text that may not be a message (a dependency's error).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Text {
    Plain(String),
    Message(Message),
}

impl Message {
    /// Prefer the macros, which extraction finds.
    #[doc(hidden)]
    pub fn new(context: Option<&str>, id: &str, plural: Option<&str>, n: Option<u64>) -> Self {
        Self {
            context: context.map(str::to_owned),
            id: id.to_owned(),
            plural: plural.map(str::to_owned),
            n,
            args: BTreeMap::new(),
        }
    }

    /// Fills `{name}` with `value`.
    pub fn arg(mut self, name: &str, value: impl Into<Text>) -> Self {
        self.args.insert(name.to_owned(), value.into());
        self
    }

    /// The message in the language in use, its arguments too.
    pub fn render(&self) -> String {
        let template = match (&self.plural, self.n, &self.context) {
            (Some(plural), Some(n), Some(context)) => {
                super::npgettext(context, &self.id, plural, n)
            }
            (Some(plural), Some(n), None) => super::ngettext(&self.id, plural, n),
            (_, _, Some(context)) => super::pgettext(context, &self.id),
            (_, _, None) => super::gettext(&self.id),
        };
        self.fill(&template, Text::render)
    }

    /// The message in English, without a lookup: what is stored beside it,
    /// for logs and for comparing two messages.
    pub fn source(&self) -> String {
        let template = match (&self.plural, self.n) {
            (Some(plural), Some(n)) if n != 1 => plural.as_str(),
            _ => self.id.as_str(),
        };
        self.fill(template, Text::source)
    }

    fn fill(&self, template: &str, text: impl Fn(&Text) -> String) -> String {
        let values: Vec<(&str, String)> = self
            .n
            .map(|n| ("n", n.to_string()))
            .into_iter()
            .chain(
                self.args
                    .iter()
                    .map(|(name, value)| (name.as_str(), text(value))),
            )
            .collect();
        let args: Vec<(&str, &dyn Display)> = values
            .iter()
            .map(|(name, value)| (*name, value as &dyn Display))
            .collect();
        super::format(template, &args)
    }
}

impl Text {
    /// In the language in use.
    pub fn render(&self) -> String {
        match self {
            Self::Plain(text) => text.clone(),
            Self::Message(message) => message.render(),
        }
    }

    /// In English.
    pub fn source(&self) -> String {
        match self {
            Self::Plain(text) => text.clone(),
            Self::Message(message) => message.source(),
        }
    }
}

impl Text {
    /// For a text column: a message as its JSON, plain text as it is.
    pub fn store(&self) -> String {
        match self {
            Self::Plain(text) => text.clone(),
            Self::Message(message) => {
                serde_json::to_string(message).unwrap_or_else(|_| message.source())
            }
        }
    }

    /// What [`Text::store`] wrote. Anything else, such as the English a
    /// column held before messages were stored, is plain text.
    pub fn load(stored: &str) -> Self {
        if stored.starts_with('{')
            && let Ok(message) = serde_json::from_str::<Message>(stored)
        {
            return Self::Message(message);
        }
        Self::Plain(stored.to_owned())
    }

    /// An error's text to keep: the message a [`TextError`] inside it
    /// carries, else its text as it reads now.
    pub fn of_error(error: &crate::Error) -> Self {
        if let crate::Error::Io(io) = error
            && let Some(text) = io
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<TextError>())
        {
            return text.0.clone();
        }
        Self::Plain(error.to_string())
    }
}

/// An error whose text is a message, rendered each time it is shown and
/// kept as its message when stored ([`Text::of_error`]). Put it in an
/// `io::Error` to keep the error's kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextError(pub Text);

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.render())
    }
}

impl std::error::Error for TextError {}

impl From<Message> for TextError {
    fn from(message: Message) -> Self {
        Self(Text::Message(message))
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Self::Plain(text)
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        Self::Plain(text.to_owned())
    }
}

impl From<Message> for Text {
    fn from(message: Message) -> Self {
        Self::Message(message)
    }
}

/// A message argument: anything `Display` is plain text; a [`Message`] is
/// translated with the message it fills.
pub trait ToText {
    fn to_text(&self) -> Text;
}

impl<T: Display + ?Sized> ToText for T {
    fn to_text(&self) -> Text {
        Text::Plain(self.to_string())
    }
}

impl ToText for Message {
    fn to_text(&self) -> Text {
        Text::Message(self.clone())
    }
}

impl ToText for Text {
    fn to_text(&self) -> Text {
        self.clone()
    }
}

/// [`tr!`](crate::tr) for later: a [`Message`] that is translated when it
/// is shown.
///
/// ```
/// let title = caskade_core::msg!("{game} added", game = "Celeste");
/// assert_eq!(title.render(), "Celeste added");
/// ```
#[macro_export]
macro_rules! msg {
    ($id:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::Message::new(::core::option::Option::None, $id, ::core::option::Option::None, ::core::option::Option::None)
            $(.arg(::core::stringify!($name), $crate::i18n::ToText::to_text(&$value)))*
    };
}

/// [`trn!`](crate::trn) for later.
///
/// ```
/// assert_eq!(caskade_core::msgn!("One game added", "{n} games added", 3).render(), "3 games added");
/// ```
#[macro_export]
macro_rules! msgn {
    ($id:literal, $plural:literal, $n:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::Message::new(
            ::core::option::Option::None,
            $id,
            ::core::option::Option::Some($plural),
            ::core::option::Option::Some($crate::i18n::count($n)),
        )
        $(.arg(::core::stringify!($name), $crate::i18n::ToText::to_text(&$value)))*
    };
}

/// [`trp!`](crate::trp) for later.
#[macro_export]
macro_rules! msgp {
    ($context:literal, $id:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::Message::new(::core::option::Option::Some($context), $id, ::core::option::Option::None, ::core::option::Option::None)
            $(.arg(::core::stringify!($name), $crate::i18n::ToText::to_text(&$value)))*
    };
}

/// [`trnp!`](crate::trnp) for later.
#[macro_export]
macro_rules! msgnp {
    ($context:literal, $id:literal, $plural:literal, $n:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::Message::new(
            ::core::option::Option::Some($context),
            $id,
            ::core::option::Option::Some($plural),
            ::core::option::Option::Some($crate::i18n::count($n)),
        )
        $(.arg(::core::stringify!($name), $crate::i18n::ToText::to_text(&$value)))*
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "{game} added"
msgstr "{game} lades till"

msgid "One game added"
msgid_plural "{n} games added"
msgstr[0] "Ett spel lades till"
msgstr[1] "{n} spel lades till"

msgid "Couldn't install: {why}"
msgstr "Kunde inte installera: {why}"

msgid "The disk is full."
msgstr "Disken är full."
"#;

    // Built without the macros, so extraction doesn't list test-only
    // messages in the template; the doc examples cover the macros.
    fn message(id: &str) -> Message {
        Message::new(None, id, None, None)
    }

    fn plural(id: &str, plural: &str, n: u64) -> Message {
        Message::new(None, id, Some(plural), Some(n))
    }

    #[test]
    fn messages_render_in_the_language_shown_and_keep_their_english() {
        let added = message("{game} added").arg("game", "Celeste");
        let many = plural("One game added", "{n} games added", 3);
        let nested = message("Couldn't install: {why}").arg("why", message("The disk is full."));
        assert_eq!(added.render(), "Celeste added");
        {
            let _sv = super::super::scope_catalog("sv", SV);
            assert_eq!(added.render(), "Celeste lades till");
            assert_eq!(many.render(), "3 spel lades till");
            assert_eq!(nested.render(), "Kunde inte installera: Disken är full.");
            // English stays English whatever is shown.
            assert_eq!(nested.source(), "Couldn't install: The disk is full.");
        }
        assert_eq!(many.render(), "3 games added");
        assert_eq!(
            plural("One game added", "{n} games added", 1).source(),
            "One game added"
        );
    }

    #[test]
    fn messages_round_trip_through_json() {
        let failed = Message::new(Some("notification"), "{game} couldn't {what}", None, None)
            .arg("game", "Celeste")
            .arg("what", message("install"));
        let json = serde_json::to_string(&failed).unwrap();
        assert_eq!(serde_json::from_str::<Message>(&json).unwrap(), failed);
        let plain: Text = serde_json::from_str(r#""disk full""#).unwrap();
        assert_eq!(plain, Text::Plain("disk full".into()));
        let text: Text = serde_json::from_str(&json).unwrap();
        assert_eq!(text, Text::Message(failed));
    }

    #[test]
    fn stored_text_is_translated_when_read_and_english_from_before_stays() {
        let added = message("{game} added").arg("game", "Celeste");
        let stored = Text::Message(added.clone()).store();
        assert_eq!(Text::load(&stored), Text::Message(added));
        // A column written before messages were stored.
        assert_eq!(
            Text::load("GitHub answered 502"),
            Text::Plain("GitHub answered 502".into())
        );
        assert_eq!(Text::load("{not json"), Text::Plain("{not json".into()));

        // An error carrying a message keeps it, and reads in the language shown.
        let error = crate::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            TextError::from(message("Couldn't install: {why}").arg("why", "disk full")),
        ));
        let kept = Text::of_error(&error).store();
        assert_eq!(error.to_string(), "Couldn't install: disk full");
        let _sv = super::super::scope_catalog("sv", SV);
        assert_eq!(error.to_string(), "Kunde inte installera: disk full");
        assert_eq!(
            Text::load(&kept).render(),
            "Kunde inte installera: disk full"
        );
        assert_eq!(
            Text::of_error(&crate::Error::Invalid("plain".into())),
            Text::Plain("plain".into())
        );
    }
}
