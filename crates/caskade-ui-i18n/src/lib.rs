//! The native UI's text (#91), free of any UI toolkit. The native UI shares
//! the webview's `app` and `ui` domains: the same English, contexts and
//! translations (`locales/<lang>/app.po`, `ui.po`), so no translator work
//! is lost when the webview goes.
//!
//! - [`t!`], [`tn!`], [`tp!`] and [`tnp!`] look a message up (the `app`
//!   domain, then `ui`) and fill it in, in the webview's message syntax:
//!   `{name}` placeholders, `#` for a plural's count, and
//!   `{n, plural, …}` / `{x, select, …}` inside a message. `note = "…"`
//!   first gives translators a comment. [`rich`] keeps numbered tags
//!   (`<0>…</0>`) for the UI to turn into elements (the native app's
//!   `trans!`).
//! - [`set_language`] switches the language; in debug builds `en-XA` is
//!   the pseudo-locale (accented, bracketed, longer).
//! - [`format`] writes numbers, sizes, durations and dates as the webview
//!   does, in the language in use.
//! - [`set_observer`]: called on every lookup, so a UI can re-render what
//!   read the language when it changes.
//!
//! `just i18n` lists these crates' messages in `locales/native.json`;
//! `packages/i18n` adds them to the `app` and `ui` templates.

use caskade_i18n::{Translations, locale, message, pseudo, translations};
use std::sync::{LazyLock, OnceLock, RwLock};

pub use caskade_i18n::intl;
pub use caskade_i18n::message::Value;
pub use caskade_i18n::rich::Node;

pub mod format;

/// The `app` domain's catalogs: the webview's, which the native UI shares.
const APP_CATALOGS: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/app_catalogs.rs"));
/// The `ui` domain's: the shared components'.
const UI_CATALOGS: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/ui_catalogs.rs"));

static APP: LazyLock<Translations> =
    LazyLock::new(|| Translations::new(APP_CATALOGS.iter().copied()));
static UI: LazyLock<Translations> =
    LazyLock::new(|| Translations::new(UI_CATALOGS.iter().copied()));

/// The language in use.
#[derive(Debug, Clone)]
struct State {
    /// The language as asked for (`sv`, `en-GB`, `en-XA`).
    language: String,
    pseudo: bool,
}

static STATE: RwLock<State> = RwLock::new(State {
    language: String::new(),
    pseudo: false,
});

static OBSERVER: OnceLock<fn()> = OnceLock::new();

/// Runs `observer` on every lookup: a UI reads its language state there,
/// so what looked text up re-renders when the language changes.
pub fn set_observer(observer: fn()) {
    let _ = OBSERVER.set(observer);
}

fn observe() {
    if let Some(observer) = OBSERVER.get() {
        observer();
    }
}

fn state() -> State {
    STATE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Whether `language` is the pseudo-locale, which debug builds offer.
pub fn is_pseudo(language: &str) -> bool {
    cfg!(debug_assertions) && language.eq_ignore_ascii_case(pseudo::LOCALE)
}

/// Switches the native UI to `language` (a BCP 47 tag, as the core
/// resolves the Language setting; `en-XA` in debug builds). Returns the
/// language its catalogs are in.
pub fn set_language(language: &str) -> String {
    let pseudo = is_pseudo(language);
    let catalogs = if pseudo { "en" } else { language };
    let resolved = APP.set_locale([catalogs]);
    UI.set_locale([catalogs]);
    *STATE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = State {
        language: language.to_owned(),
        pseudo,
    };
    if pseudo {
        pseudo::LOCALE.to_owned()
    } else {
        resolved
    }
}

/// How much of each language is translated, from 0 to 1, as of the build
/// (every domain's messages, as the webview's `translated`).
const COMPLETENESS: &[(&str, f64)] = include!(concat!(env!("OUT_DIR"), "/completeness.rs"));

/// How much of `tag` is translated, from 0 to 1; `None` for English and a
/// language without catalogs.
pub fn translated(tag: &str) -> Option<f64> {
    COMPLETENESS
        .iter()
        .find(|(known, _)| *known == tag)
        .map(|(_, share)| *share)
}

/// A language by its own name ("Svenska"), or its tag without data for it,
/// as the webview's `languageName`.
pub fn language_name(tag: &str) -> String {
    intl::language_name(tag).map_or_else(|| tag.to_owned(), str::to_owned)
}

/// The language in use, as asked for.
pub fn language() -> String {
    observe();
    let state = state();
    if state.language.is_empty() {
        locale::SOURCE.to_owned()
    } else {
        state.language
    }
}

/// The language numbers and dates follow: the language's, in the OS's
/// region when the OS speaks it (English in the UK has the UK's dates), as
/// the webview's `formatLocale` does.
pub fn format_locale() -> String {
    let language = language();
    if is_pseudo(&language) {
        return "en".to_owned();
    }
    if language.contains('-') {
        return language;
    }
    locale::system()
        .into_iter()
        .find(|tag| {
            tag.split(['-', '_'])
                .next()
                .is_some_and(|first| first.eq_ignore_ascii_case(&language))
        })
        .unwrap_or(language)
}

struct Numbers(String);

impl message::Locale for Numbers {
    fn category(&self, n: f64) -> &'static str {
        intl::plural_category(&self.0, n)
    }

    fn number(&self, n: f64) -> String {
        intl::count(&self.0, n)
    }
}

fn render(tokens: &[message::Token], args: &[(&str, &dyn Value)], count: Option<f64>) -> String {
    let pseudo = state().pseudo;
    let piece = |text: &str| {
        if pseudo {
            pseudo::localize(text)
        } else {
            text.to_owned()
        }
    };
    message::render(tokens, args, count, &Numbers(format_locale()), &piece)
}

/// The translation of `id` (in `context`): the `app` domain's, else the
/// `ui` domain's, else the English.
fn lookup(context: Option<&str>, id: &str) -> String {
    APP.find(context, id)
        .or_else(|| UI.find(context, id))
        .unwrap_or_else(|| id.to_owned())
}

/// A message, filled in. Prefer [`t!`] and [`tp!`], which extraction
/// finds.
pub fn text(context: Option<&str>, id: &str, args: &[(&str, &dyn Value)]) -> String {
    observe();
    render(&message::parse(&lookup(context, id)), args, None)
}

/// A gettext plural's form for `n`, filled in (`#` is `n`). Prefer [`tn!`]
/// and [`tnp!`].
pub fn plural(
    context: Option<&str>,
    id: &str,
    plural: &str,
    n: f64,
    args: &[(&str, &dyn Value)],
) -> String {
    observe();
    let whole = n.max(0.0).round() as u64;
    let form = APP
        .find_plural(context, id, whole)
        .or_else(|| UI.find_plural(context, id, whole))
        .unwrap_or_else(|| translations::english(id, plural, whole).to_owned());
    render(&message::parse_counted(&form), args, Some(n))
}

/// A message with numbered tags, as a tree for the UI to turn into
/// elements. The native app's `trans!` calls it.
pub fn rich(context: Option<&str>, id: &str, args: &[(&str, &dyn Value)]) -> Vec<Node> {
    caskade_i18n::rich::parse(&text(context, id, args))
}

/// A message: `t!("Quick menu")`, `t!("Stop {title}", title = name)`;
/// `note = "…"` first is a comment for translators.
#[macro_export]
macro_rules! t {
    (note = $note:literal, $($rest:tt)+) => { $crate::t!($($rest)+) };
    ($id:literal $(,)?) => { $crate::text(None, $id, &[]) };
    ($id:literal, $($name:tt = $value:expr),+ $(,)?) => {
        $crate::text(None, $id, &[$((stringify!($name), &$value as &dyn $crate::Value)),+])
    };
}

/// A message in a context, for English that means different things in
/// different places: `tp!("size", "Unknown")`.
#[macro_export]
macro_rules! tp {
    (note = $note:literal, $($rest:tt)+) => { $crate::tp!($($rest)+) };
    ($context:literal, $id:literal $(,)?) => { $crate::text(Some($context), $id, &[]) };
    ($context:literal, $id:literal, $($name:tt = $value:expr),+ $(,)?) => {
        $crate::text(Some($context), $id, &[$((stringify!($name), &$value as &dyn $crate::Value)),+])
    };
}

/// A count's message: `tn!("# game", "# games", n)`; `#` is the count.
#[macro_export]
macro_rules! tn {
    (note = $note:literal, $($rest:tt)+) => { $crate::tn!($($rest)+) };
    ($id:literal, $plural:literal, $n:expr $(,)?) => {
        $crate::plural(None, $id, $plural, ($n) as f64, &[])
    };
    ($id:literal, $plural:literal, $n:expr, $($name:tt = $value:expr),+ $(,)?) => {
        $crate::plural(None, $id, $plural, ($n) as f64, &[$((stringify!($name), &$value as &dyn $crate::Value)),+])
    };
}

/// A count's message in a context.
#[macro_export]
macro_rules! tnp {
    (note = $note:literal, $($rest:tt)+) => { $crate::tnp!($($rest)+) };
    ($context:literal, $id:literal, $plural:literal, $n:expr $(,)?) => {
        $crate::plural(Some($context), $id, $plural, ($n) as f64, &[])
    };
    ($context:literal, $id:literal, $plural:literal, $n:expr, $($name:tt = $value:expr),+ $(,)?) => {
        $crate::plural(Some($context), $id, $plural, ($n) as f64, &[$((stringify!($name), &$value as &dyn $crate::Value)),+])
    };
}

#[cfg(test)]
mod tests;
