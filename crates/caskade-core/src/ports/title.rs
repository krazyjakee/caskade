//! Titles as matching keys: "The Legend of Zelda: Majora's Mask",
//! "Legend of Zelda, The - Majora's Mask" and "legend of zelda majoras
//! mask" are one key.

use regex::Regex;
use std::{collections::BTreeSet, sync::LazyLock};

const ROMAN: [(&str, &str); 9] = [
    ("ii", "2"),
    ("iii", "3"),
    ("iv", "4"),
    ("v", "5"),
    ("vi", "6"),
    ("vii", "7"),
    ("viii", "8"),
    ("ix", "9"),
    ("x", "10"),
];

static BRACKETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(.*?\)|\[.*?\]").expect("valid pattern"));
static TRAILING_THE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*), the\b").expect("valid pattern"));
/// Words in a game file's label that describe the file, not the game.
static LABEL_NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:roms?|discs?|disks?|images?|dumps?|iso|game|data|files?|copy|original|cartridge|cart|ntsc(?:-[uj])?|pal|usa|us|europe|eu|japan|jp|version|retail|z64|n64|big-endian|byte-swapped|legally|owned|supported|revision|rev|or|your)\b",
    )
    .expect("valid pattern")
});

/// The matching key of a title; empty when nothing is left (a title in a
/// script other than Latin).
pub fn key(title: &str) -> String {
    let decomposed: String = icu_normalizer::DecomposingNormalizer::new_nfkd()
        .normalize(title)
        .chars()
        // Accents decompose into combining marks: "é" is "e" and a mark.
        .filter(|character| !('\u{300}'..='\u{36f}').contains(character))
        .flat_map(char::to_lowercase)
        .collect();
    let text = BRACKETS.replace_all(&decomposed, " ");
    let text = text.replace('&', " and ").replace(['\'', '’'], "");
    let text = TRAILING_THE.replace(&text, "the $1");
    let words: Vec<&str> = text
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let words = match words.split_first() {
        Some((&"the", rest)) if !rest.is_empty() => rest,
        _ => &words[..],
    };
    words
        .iter()
        .map(|word| {
            ROMAN
                .iter()
                .find(|(roman, _)| roman == word)
                .map_or(*word, |(_, digit)| digit)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Adds a catalogue title's keys: the whole title and its " / " parts to
/// `keys`, and the halves of "Series: Subtitle" (two words or more) to
/// `parts`.
pub fn keys_of(title: &str, keys: &mut BTreeSet<String>, parts: &mut BTreeSet<String>) {
    let whole = key(title);
    if !whole.is_empty() {
        keys.insert(whole);
    }
    for piece in title.split(" / ") {
        let piece_key = key(piece);
        if !piece_key.is_empty() {
            keys.insert(piece_key);
        }
        halves(piece, parts);
    }
}

/// The halves of "Series: Subtitle" and "Series - Subtitle" with two words
/// or more.
pub fn halves(title: &str, out: &mut BTreeSet<String>) {
    for separator in [": ", " - ", " – "] {
        if let Some((head, tail)) = title.split_once(separator) {
            for half in [head, tail] {
                let half = key(half);
                if half.split(' ').count() >= 2 {
                    out.insert(half);
                }
            }
        }
    }
}

/// The game a file's label names ("Majora's Mask US ROM" is "majoras
/// mask"), when it names one.
pub fn label_key(label: &str) -> Option<String> {
    let lowered = label.to_lowercase();
    let without = BRACKETS.replace_all(&lowered, " ");
    let without = LABEL_NOISE.replace_all(&without, " ");
    let key = key(&without);
    (key.len() >= 4).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_of_a_title_share_a_key() {
        assert_eq!(
            key("The Legend of Zelda: Majora's Mask"),
            "legend of zelda majoras mask"
        );
        assert_eq!(
            key("Legend of Zelda, The - Majora's Mask"),
            "legend of zelda majoras mask"
        );
        assert_eq!(key("Pokémon Platinum"), "pokemon platinum");
        assert_eq!(key("Final Fantasy VIII"), "final fantasy 8");
        assert_eq!(key("Sonic & Knuckles"), "sonic and knuckles");
        assert_eq!(
            key("Sonic 3 A.I.R. (Angel Island Revisited)"),
            "sonic 3 a i r"
        );
        assert_eq!(key("ゼルダの伝説"), "");
    }

    #[test]
    fn titles_give_whole_keys_and_halves() {
        let (mut keys, mut parts) = (BTreeSet::new(), BTreeSet::new());
        keys_of("Pokémon Red / Blue / Yellow", &mut keys, &mut parts);
        assert!(keys.contains("pokemon red"));
        assert!(keys.contains("blue"));
        let (mut keys, mut parts) = (BTreeSet::new(), BTreeSet::new());
        keys_of("Doom 3: BFG Edition", &mut keys, &mut parts);
        assert_eq!(keys, BTreeSet::from(["doom 3 bfg edition".to_owned()]));
        assert_eq!(
            parts,
            BTreeSet::from(["bfg edition".to_owned(), "doom 3".to_owned()])
        );
    }

    #[test]
    fn labels_name_their_game() {
        assert_eq!(
            label_key("Majora's Mask US ROM").as_deref(),
            Some("majoras mask")
        );
        assert_eq!(
            label_key("Sonic 3 & Knuckles ROM").as_deref(),
            Some("sonic 3 and knuckles")
        );
        assert_eq!(label_key("Disc image (USA)"), None);
        assert_eq!(label_key("Original game files"), None);
    }
}
