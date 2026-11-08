//! The core's language (#212): the `core` gettext domain, which covers the
//! Rust crates, the Tauri adapter and the tray. See `docs/translating.md`.
//!
//! Mark text people read with the macros below; `just i18n` extracts them
//! into `locales/core.pot`, and each language's `locales/<lang>/core.po`
//! is compiled into the binary (`caskade_i18n::bundled::CORE`).
//!
//! | Macro | gettext | Example |
//! |---|---|---|
//! | [`tr!`](crate::tr) | `gettext` | `tr!("Show Caskade")` |
//! | [`trn!`](crate::trn) | `ngettext` | `trn!("One game", "{n} games", count)` |
//! | [`trp!`](crate::trp) | `pgettext` | `trp!("folder browser place", "Home")` |
//! | [`trnp!`](crate::trnp) | `npgettext` | `trnp!("tray", "One game running", "{n} games running", count)` |
//!
//! The English text is the message id; a context (`trp!`, `trnp!`) tells
//! apart the same English with different meanings. Arguments are named
//! placeholders filled in after the lookup, so translations can reorder
//! them: `tr!("Couldn't start {game}", game = title)`. The plural macros
//! fill `{n}` with the count. Missing and fuzzy translations show English.
//!
//! The core owns the active language: [`set_locale`] at start and when the
//! Language setting changes, and everything that shows text follows
//! [`subscribe`] (the tray rebuilds its menu). Lookups and switches are
//! thread-safe.

use crate::settings::{GeneralSettings, SettingsService};
use caskade_i18n::Translations;
use std::sync::LazyLock;
use tokio::sync::watch;

pub use caskade_i18n::format;
pub use caskade_i18n::locale::SOURCE;

mod message;
mod scope;
mod tasks;
pub use message::{Message, Text, TextError, ToText};
pub use scope::{Scope, scope, scope_catalog};
pub use tasks::translated_tasks;

/// The language setting's value for the OS's languages.
pub const SYSTEM: &str = "system";

/// The core domain's catalogs, compiled in so a release needs no locale
/// files on disk: every `locales/<lang>/core.po`, which `caskade-i18n`'s
/// build script lists.
const CATALOGS: &[(&str, &str)] = caskade_i18n::bundled::CORE;

static CORE: LazyLock<Language> = LazyLock::new(Language::bundled);

/// A domain's catalogs, its active language and who follows it.
struct Language {
    translations: Translations,
    changes: watch::Sender<String>,
}

impl Language {
    fn bundled() -> Self {
        Self {
            translations: Translations::new(CATALOGS.iter().copied()),
            changes: watch::Sender::new(SOURCE.to_owned()),
        }
    }

    fn set(&self, language: &str) -> String {
        let requested = requested(language);
        let mut resolved = String::new();
        // Switching under the channel's lock keeps the active language and
        // the one announced in step when two switches race.
        self.changes.send_if_modified(|current| {
            resolved = self.translations.set_locale(&requested);
            if *current == resolved {
                return false;
            }
            current.clone_from(&resolved);
            true
        });
        resolved
    }

    /// Follows `core.general.language`: switches to it now and each time
    /// it changes, until the settings go away.
    async fn follow_setting(&self, settings: &SettingsService) {
        use tokio::sync::broadcast::error::RecvError;
        // Subscribed before the first read, so no change falls between.
        let mut changes = settings.subscribe();
        self.set(&language_setting(settings).await);
        loop {
            match changes.recv().await {
                Ok(changed) if changed.namespace == "core.general" => {}
                Ok(_) | Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
            self.set(&language_setting(settings).await);
        }
    }
}

/// The tags `language` asks for, most preferred first: the OS's languages
/// for [`SYSTEM`] (or empty).
fn requested(language: &str) -> Vec<String> {
    if language.trim().is_empty() || language == SYSTEM {
        caskade_i18n::locale::system()
    } else {
        vec![language.to_owned()]
    }
}

/// `core.general.language`; [`SYSTEM`] (the default) when the settings
/// can't be read.
async fn language_setting(settings: &SettingsService) -> String {
    settings
        .read_or_default::<GeneralSettings>("core.general")
        .await
        .language
}

/// Switches to the Language setting (`core.general.language`) and returns
/// the language in use. The app calls it before its window loads, so the
/// webview's first render is already in it.
pub async fn apply_setting(settings: &SettingsService) -> String {
    let language = language_setting(settings).await;
    set_locale(&language)
}

/// Follows the Language setting for as long as the settings live: each
/// change switches the core's language, and [`subscribe`] hears of it.
pub async fn follow_setting(settings: SettingsService) {
    CORE.follow_setting(&settings).await;
}

/// What `language` (a tag, or [`SYSTEM`]) would switch to, without
/// switching: the Language setting shows what System means.
pub fn resolve(language: &str) -> String {
    caskade_i18n::locale::negotiate(requested(language), &available())
        .unwrap_or(SOURCE)
        .to_owned()
}

/// Switches the core's language: a BCP 47 tag such as `sv` or `pt-BR`, or
/// [`SYSTEM`] (or empty) for the OS's languages. Falls back through the
/// tag's less specific forms, then English. Returns the language in use;
/// followers of [`subscribe`] hear of a change.
pub fn set_locale(language: &str) -> String {
    let resolved = CORE.set(language);
    tracing::debug!(language, locale = %resolved, "core language");
    resolved
}

/// The language in use: English ([`SOURCE`]) or one with a catalog.
pub fn locale() -> String {
    CORE.translations.locale()
}

/// English, then the languages the core has a catalog for.
pub fn available() -> Vec<&'static str> {
    CORE.translations.available()
}

/// The language in use now and after each change.
pub fn subscribe() -> watch::Receiver<String> {
    CORE.changes.subscribe()
}

/// `id` in the language in use. Prefer [`tr!`](crate::tr), which
/// extraction finds.
pub fn gettext(id: &str) -> String {
    scope::lookup(|translations| translations.gettext(None, id))
}

/// `id` in `context`. Prefer [`trp!`](crate::trp).
pub fn pgettext(context: &str, id: &str) -> String {
    scope::lookup(|translations| translations.gettext(Some(context), id))
}

/// The form for `n`. Prefer [`trn!`](crate::trn).
pub fn ngettext(id: &str, plural: &str, n: u64) -> String {
    scope::lookup(|translations| translations.ngettext(None, id, plural, n))
}

/// The form for `n` in `context`. Prefer [`trnp!`](crate::trnp).
pub fn npgettext(context: &str, id: &str, plural: &str, n: u64) -> String {
    scope::lookup(|translations| translations.ngettext(Some(context), id, plural, n))
}

/// A count for the plural rules; negative and oversized counts take the
/// "many" side.
#[doc(hidden)]
pub fn count(n: impl TryInto<u64>) -> u64 {
    n.try_into().unwrap_or(u64::MAX)
}

/// `gettext`: the English text in the language in use, with named
/// placeholders filled in after the lookup.
///
/// ```
/// let title = "Celeste";
/// assert_eq!(caskade_core::tr!("Show Caskade"), "Show Caskade");
/// assert_eq!(caskade_core::tr!("Couldn't start {game}", game = title), "Couldn't start Celeste");
/// ```
#[macro_export]
macro_rules! tr {
    ($id:literal $(,)?) => {
        $crate::i18n::gettext($id)
    };
    ($id:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::format(
            &$crate::i18n::gettext($id),
            &[$((::core::stringify!($name), &$value as &dyn ::core::fmt::Display)),+],
        )
    };
}

/// `ngettext`: the singular or a plural form for a count, which fills
/// `{n}`.
///
/// ```
/// assert_eq!(caskade_core::trn!("One game", "{n} games", 3), "3 games");
/// ```
#[macro_export]
macro_rules! trn {
    ($id:literal, $plural:literal, $n:expr $(, $name:ident = $value:expr)* $(,)?) => {{
        let n = $n;
        $crate::i18n::format(
            &$crate::i18n::ngettext($id, $plural, $crate::i18n::count(n)),
            &[
                ("n", &n as &dyn ::core::fmt::Display)
                $(, (::core::stringify!($name), &$value as &dyn ::core::fmt::Display))*
            ],
        )
    }};
}

/// `pgettext`: `tr!` with a context, for English that means different
/// things in different places.
///
/// ```
/// assert_eq!(caskade_core::trp!("folder browser place", "Home"), "Home");
/// ```
#[macro_export]
macro_rules! trp {
    ($context:literal, $id:literal $(,)?) => {
        $crate::i18n::pgettext($context, $id)
    };
    ($context:literal, $id:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::format(
            &$crate::i18n::pgettext($context, $id),
            &[$((::core::stringify!($name), &$value as &dyn ::core::fmt::Display)),+],
        )
    };
}

/// `npgettext`: `trn!` with a context.
#[macro_export]
macro_rules! trnp {
    ($context:literal, $id:literal, $plural:literal, $n:expr $(, $name:ident = $value:expr)* $(,)?) => {{
        let n = $n;
        $crate::i18n::format(
            &$crate::i18n::npgettext($context, $id, $plural, $crate::i18n::count(n)),
            &[
                ("n", &n as &dyn ::core::fmt::Display)
                $(, (::core::stringify!($name), &$value as &dyn ::core::fmt::Display))*
            ],
        )
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_catalog_reads_without_problems() {
        let language = Language::bundled();
        assert!(language.translations.check().is_empty());
        for (tag, text) in CATALOGS {
            let problems = caskade_i18n::lint(text).unwrap();
            assert!(problems.is_empty(), "locales/{tag}/core.po: {problems:?}");
        }
    }

    #[test]
    fn switching_changes_lookups_and_tells_followers() {
        let language = Language::bundled();
        let mut changes = language.changes.subscribe();
        assert_eq!(
            language.translations.gettext(None, "Show Caskade"),
            "Show Caskade"
        );

        assert_eq!(language.set("sv-SE"), "sv");
        assert!(changes.has_changed().unwrap());
        assert_eq!(*changes.borrow_and_update(), "sv");
        assert_eq!(
            language.translations.gettext(None, "Show Caskade"),
            "Visa Caskade"
        );
        assert_eq!(
            language
                .translations
                .gettext(Some("folder browser place"), "Home"),
            "Hem"
        );

        // The same language again is no change.
        assert_eq!(language.set("sv"), "sv");
        assert!(!changes.has_changed().unwrap());

        // A language without a catalog shows English.
        assert_eq!(language.set("de-DE"), "en");
        assert_eq!(*changes.borrow_and_update(), "en");
        assert_eq!(
            language.translations.gettext(None, "Show Caskade"),
            "Show Caskade"
        );
    }

    #[test]
    fn untranslated_text_shows_english() {
        let language = Language::bundled();
        language.set("sv");
        assert_eq!(
            language.translations.gettext(None, "Not in any catalog"),
            "Not in any catalog"
        );
        assert_eq!(
            language
                .translations
                .ngettext(None, "One game", "{n} games", 2),
            "{n} games"
        );
    }

    #[test]
    fn resolving_matches_catalogs() {
        assert_eq!(resolve("sv-SE"), "sv");
        assert_eq!(resolve("sv"), "sv");
        // A language Caskade doesn't ship is English.
        assert_eq!(resolve("de"), "en");
        assert_eq!(
            resolve(SYSTEM),
            caskade_i18n::locale::negotiate(caskade_i18n::locale::system(), &available())
                .unwrap_or(SOURCE)
        );
    }

    #[tokio::test]
    async fn the_language_follows_the_setting() {
        let store = crate::db::LibraryStore::in_memory().await.unwrap();
        let settings = SettingsService::new(store);
        settings
            .register(
                crate::settings::SectionDefinition::typed::<GeneralSettings>("core.general", 1)
                    .unwrap(),
            )
            .unwrap();
        let language = std::sync::Arc::new(Language::bundled());
        let mut changes = language.changes.subscribe();
        let follower = tokio::spawn({
            let (language, settings) = (language.clone(), settings.clone());
            async move { language.follow_setting(&settings).await }
        });

        let save = |tag: &'static str| {
            let settings = settings.clone();
            async move {
                let current = settings.get("core.general").await.unwrap();
                let mut value = current.value;
                value["language"] = tag.into();
                settings
                    .set("core.general", value, Some(current.revision))
                    .await
            }
        };
        save("sv").await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while *changes.borrow_and_update() != "sv" {
                changes.changed().await.unwrap();
            }
        })
        .await
        .expect("the language follows the setting");
        assert_eq!(
            language.translations.gettext(None, "Show Caskade"),
            "Visa Caskade"
        );

        save("pt-BR").await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while *changes.borrow_and_update() != "en" {
                changes.changed().await.unwrap();
            }
        })
        .await
        .expect("a language without a catalog is English");

        // Tags and System only: nothing else reaches the setting.
        assert!(save("en\n/unsafe").await.is_err());
        assert!(save("system").await.is_ok());
        follower.abort();
    }

    #[test]
    fn counts_convert_from_any_integer() {
        assert_eq!(count(3_usize), 3);
        assert_eq!(count(-1_i64), u64::MAX);
        assert_eq!(count(7_u8), 7);
    }
}
