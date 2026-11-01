//! Titles in the language's alphabetical order (#218): the collation every
//! library connection has, the letters the alphabet bar and console paging
//! step through, and short search words that fold case and accents.
use icu_collator::{
    Collator, CollatorBorrowed, CollatorPreferences,
    options::{CollatorOptions, Strength},
};
use icu_locale_core::Locale;
use icu_normalizer::DecomposingNormalizerBorrowed;
use std::{
    cmp::Ordering,
    collections::HashMap,
    sync::{Arc, LazyLock, RwLock},
};
use tokio::sync::watch;

/// The collation's name in SQL: `ORDER BY g.sort_title COLLATE caskade_title`.
pub(super) const TITLE: &str = "caskade_title";

/// The order of titles in the core's language (`i18n::subscribe`), or in
/// a fixed one (tests). In Swedish Å, Ä and Ö come after Z; in German Ä
/// goes with A. Nothing indexes the collation, so a language switch
/// changes the next query's order and nothing stored.
pub(super) struct TitleOrder {
    fixed: Option<String>,
    language: watch::Receiver<String>,
    collators: RwLock<Arc<Collators>>,
}

impl std::fmt::Debug for TitleOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TitleOrder")
            .field("fixed", &self.fixed)
            .finish_non_exhaustive()
    }
}

impl TitleOrder {
    /// `language` fixes the order's language; `None` follows the core's.
    pub(super) fn new(language: Option<&str>) -> Self {
        let receiver = crate::i18n::subscribe();
        let current = language.map_or_else(|| receiver.borrow().clone(), str::to_owned);
        Self {
            fixed: language.map(str::to_owned),
            language: receiver,
            collators: RwLock::new(Arc::new(Collators::new(&current))),
        }
    }

    /// The collators for the language in use now.
    pub(super) fn current(&self) -> Arc<Collators> {
        let followed;
        let wanted: &str = match &self.fixed {
            Some(language) => language,
            None => {
                followed = self.language.borrow();
                &followed
            }
        };
        if let Ok(collators) = self.collators.read()
            && collators.locale == wanted
        {
            return collators.clone();
        }
        let fresh = Arc::new(Collators::new(wanted));
        if let Ok(mut collators) = self.collators.write() {
            *collators = fresh.clone();
        }
        fresh
    }

    /// The collation SQLite calls.
    pub(super) fn compare(&self, a: &str, b: &str) -> Ordering {
        self.current().order.compare(a, b)
    }
}

/// One language's collators: the full order, and whether two letters are
/// the same letter of its alphabet (accents and case aside where the
/// language ignores them).
pub(super) struct Collators {
    locale: String,
    order: CollatorBorrowed<'static>,
    letters: CollatorBorrowed<'static>,
}

impl Collators {
    fn new(locale: &str) -> Self {
        let preferences = Locale::try_from_str(locale)
            .map(|locale| CollatorPreferences::from(&locale))
            .unwrap_or_default();
        let collator = |strength| {
            let mut options = CollatorOptions::default();
            options.strength = strength;
            Collator::try_new(preferences, options)
                .or_else(|error| {
                    tracing::warn!(%error, locale, "no collation for the language; using the root order");
                    Collator::try_new(CollatorPreferences::default(), options)
                })
                .expect("the root collation is compiled in")
        };
        Self {
            locale: locale.to_owned(),
            order: collator(None),
            letters: collator(Some(Strength::Primary)),
        }
    }

    /// The letter `title` is filed under: its first letter in capitals,
    /// as the A to Z letter it counts as in the language where it is one
    /// (É is E in English, Ä is A in German, but Å is Å in Swedish), or
    /// `#` for digits, symbols and scripts without capitals.
    fn letter(&self, title: &str, seen: &mut HashMap<char, String>) -> String {
        let Some(first) = title.trim_start().chars().next() else {
            return "#".into();
        };
        let upper = first.to_uppercase().next().unwrap_or(first);
        seen.entry(upper)
            .or_insert_with(|| {
                if upper.to_lowercase().eq(std::iter::once(upper)) {
                    return "#".into();
                }
                if upper.is_ascii_uppercase() {
                    return upper.into();
                }
                let mut buffer = [0; 4];
                let text: &str = upper.encode_utf8(&mut buffer);
                ('A'..='Z')
                    .find(|letter| {
                        self.letters.compare(text, letter.encode_utf8(&mut [0; 4]))
                            == Ordering::Equal
                    })
                    .unwrap_or(upper)
                    .into()
            })
            .clone()
    }

    /// Each letter's first title in `titles` (sorted by this order), in
    /// that order; `#` once, at the first title it files.
    pub(super) fn letters<'a>(
        &self,
        titles: impl IntoIterator<Item = &'a str>,
    ) -> Vec<(String, u64)> {
        let mut seen = HashMap::new();
        let mut letters: Vec<(String, u64)> = Vec::new();
        for (offset, title) in titles.into_iter().enumerate() {
            let letter = self.letter(title, &mut seen);
            if !letters.iter().any(|(known, _)| *known == letter) {
                letters.push((letter, offset as u64));
            }
        }
        letters
    }
}

/// Every Latin letter, by the letter it is without case or accents: `e`
/// has e, E, é, É, è, ê, ë, …
static VARIANTS: LazyLock<HashMap<char, String>> = LazyLock::new(|| {
    let mut variants: HashMap<char, String> = HashMap::new();
    let latin = ('A'..='Z')
        .chain('a'..='z')
        .chain('\u{c0}'..='\u{24f}')
        .chain('\u{1e00}'..='\u{1eff}');
    for letter in latin {
        if let Some(base) = fold(letter)
            && base != letter
        {
            variants
                .entry(base)
                .or_insert_with(|| base.into())
                .push(letter);
        }
    }
    variants
});

/// `letter` without case or accents, when that is another letter.
fn fold(letter: char) -> Option<char> {
    let mut buffer = [0; 4];
    let decomposed =
        DecomposingNormalizerBorrowed::new_nfd().normalize(letter.encode_utf8(&mut buffer));
    let base = decomposed.chars().next()?;
    let lower = base.to_lowercase().next()?;
    (lower.is_alphabetic()).then_some(lower)
}

/// A GLOB pattern that finds `word` anywhere, case and accents aside, as
/// the trigram index does for words of three letters or more (FTS5 can't
/// look up shorter ones): each letter matches its capital and its accented
/// forms, so "é" finds "Pokémon" and "Élan". GLOB has no escape character;
/// a bracket makes `*`, `?` and `[` literal.
pub(super) fn folded_glob(word: &str) -> String {
    let mut pattern = String::from("*");
    for letter in word.chars() {
        let variants = fold(letter).and_then(|base| VARIANTS.get(&base));
        match (variants, letter) {
            (Some(variants), _) => {
                pattern.push('[');
                pattern.push_str(variants);
                pattern.push(']');
            }
            (None, '*' | '?' | '[') => {
                pattern.push('[');
                pattern.push(letter);
                pattern.push(']');
            }
            (None, _) => pattern.push(letter),
        }
    }
    pattern.push('*');
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_words_match_every_case_and_accent() {
        let pattern = folded_glob("é");
        for letter in ['e', 'E', 'é', 'É', 'è', 'ê', 'ë'] {
            assert!(pattern.contains(letter), "{letter} in {pattern}");
        }
        assert!(!pattern.contains('a'));
        assert_eq!(folded_glob("1?"), "*1[?]*");
        assert_eq!(folded_glob("%_"), "*%_*");
    }

    #[test]
    fn letters_follow_the_language() {
        let titles = ["007", "Apa", "Élan", "Zork", "Åsa", "Ärlig", "Öga"];
        let at = |letter: &str, offset: u64| (letter.to_owned(), offset);
        let swedish = Collators::new("sv");
        assert_eq!(
            swedish.letters(titles),
            [
                at("#", 0),
                at("A", 1),
                at("E", 2),
                at("Z", 3),
                at("Å", 4),
                at("Ä", 5),
                at("Ö", 6),
            ]
        );
        let german = Collators::new("de");
        let mut seen = HashMap::new();
        assert_eq!(german.letter("Ärger", &mut seen), "A");
        assert_eq!(german.letter("ゼルダ", &mut seen), "#");
        assert_eq!(german.order.compare("Ärger", "Bach"), Ordering::Less);
        assert_eq!(swedish.order.compare("Ärger", "Zork"), Ordering::Greater);
    }
}
