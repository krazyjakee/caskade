//! A language for one thread, for tests: lookups on the thread use it
//! until its guard drops, while the app-wide language stays as it is, so
//! tests running side by side never see each other's language.

use caskade_i18n::Translations;
use std::cell::RefCell;

thread_local! {
    static SCOPED: RefCell<Option<Translations>> = const { RefCell::new(None) };
}

/// Runs a lookup in this thread's language, else the app's.
pub(super) fn lookup<T>(f: impl FnOnce(&Translations) -> T) -> T {
    SCOPED.with(|scoped| match scoped.borrow().as_ref() {
        Some(translations) => f(translations),
        None => f(&super::CORE.translations),
    })
}

/// Puts the thread's previous language back when dropped.
#[must_use = "the language applies until the guard drops"]
pub struct Scope {
    previous: Option<Translations>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        SCOPED.with(|scoped| *scoped.borrow_mut() = previous);
    }
}

fn enter(translations: Translations, language: &str) -> Scope {
    translations.set_locale([language]);
    Scope {
        previous: SCOPED.with(|scoped| scoped.replace(Some(translations))),
    }
}

/// Lookups on this thread use the bundled catalog for `language` (`sv`)
/// until the guard drops. For tests; a `#[tokio::test]` runs on one thread.
#[doc(hidden)]
pub fn scope(language: &str) -> Scope {
    enter(Translations::new(super::CATALOGS.iter().copied()), language)
}

/// Lookups on this thread use `po`, a test catalog for `language`, until
/// the guard drops.
#[doc(hidden)]
pub fn scope_catalog(language: &str, po: &'static str) -> Scope {
    enter(Translations::new([(language, po)]), language)
}
