//! Age ratings as a minimum age (#204), for per-account age limits.
//!
//! The metadata server passes on each board's rating as IGDB lists it,
//! `{board, rating}`, in the vocabulary of its README. Each label maps to
//! the youngest age it allows:
//!
//! | Board      | Label → minimum age                                              |
//! |------------|------------------------------------------------------------------|
//! | `pegi`     | `3` 3, `7` 7, `12` 12, `16` 16, `18` 18                          |
//! | `esrb`     | `EC` 3, `E` 6, `E10+` 10, `T` 13, `M` 17, `AO` 18                |
//! | `usk`      | `0` 0, `6` 6, `12` 12, `16` 16, `18` 18                          |
//! | `cero`     | `A` 0, `B` 12, `C` 15, `D` 17, `Z` 18                            |
//! | `acb`      | `G` 0, `PG` 8, `M` 15, `MA15+` 15, `R18+` 18, `X18+` 18, `RC` 18 |
//! | `grac`     | `All` 0, `12` 12, `15` 15, `18` 18                               |
//! | `classind` | `L` 0, `10` 10, `12` 12, `14` 14, `16` 16, `18` 18               |
//!
//! ESRB's `RP` (rating pending) and GRAC's `Testing` say no rating yet, so
//! they count as unrated. ACB's `M` only recommends 15+, but counts as 15;
//! `RC` (refused classification) counts as the strictest. Labels compare
//! without case, and a board's own name in front (`PEGI 18`) is ignored.
//! A board or label not in the table counts as unrated and is logged once.
//!
//! A game's minimum age is the highest of its ratings' ([`min_age`]); a
//! game with no rating that maps is unrated (`None`), never 0. Where one
//! rating is shown, it is the preferred board's, PEGI by default
//! ([`DEFAULT_BOARD`], [`shown`]).

use super::IgdbAgeRating;
use std::{collections::HashSet, sync::Mutex};

/// The board whose rating is shown when the user prefers none.
pub const DEFAULT_BOARD: &str = "pegi";

/// The boards the table knows, in the order a game page lists them.
pub const BOARDS: [&str; 7] = ["pegi", "esrb", "usk", "cero", "acb", "grac", "classind"];

/// The most ratings a game keeps.
pub(crate) const MAX_RATINGS: usize = 32;

/// The minimum age `rating` asks for: `None` when it is no rating (pending)
/// or not in the table.
pub fn rating_age(rating: &IgdbAgeRating) -> Option<u8> {
    match lookup(&rating.board, &rating.rating) {
        Known::Age(age) => Some(age),
        Known::Unrated => None,
        Known::Unknown => {
            unknown_once(&rating.board, &rating.rating);
            None
        }
    }
}

/// The highest minimum age among `ratings`; `None` when none of them maps
/// to one (the game is unrated).
pub fn min_age(ratings: &[IgdbAgeRating]) -> Option<u8> {
    ratings.iter().filter_map(rating_age).max()
}

/// The rating to show: `board`'s, else [`DEFAULT_BOARD`]'s, else the first
/// of [`BOARDS`] the game has, else its first.
pub fn shown<'a>(ratings: &'a [IgdbAgeRating], board: Option<&str>) -> Option<&'a IgdbAgeRating> {
    board
        .into_iter()
        .chain([DEFAULT_BOARD])
        .chain(BOARDS)
        .find_map(|board| ratings.iter().find(|rating| rating.board == board))
        .or_else(|| ratings.first())
}

/// `ratings` trimmed, boards lowercased, without blanks or duplicates, at
/// most [`MAX_RATINGS`].
pub(crate) fn clean(ratings: &[IgdbAgeRating]) -> Vec<IgdbAgeRating> {
    let mut out: Vec<IgdbAgeRating> = Vec::new();
    for rating in ratings {
        let clean = IgdbAgeRating {
            board: rating.board.trim().to_ascii_lowercase(),
            rating: rating.rating.trim().to_owned(),
        };
        let fits = |text: &str| !text.is_empty() && text.len() <= 64 && !text.contains('\0');
        if fits(&clean.board) && fits(&clean.rating) && !out.contains(&clean) {
            out.push(clean);
        }
        if out.len() == MAX_RATINGS {
            break;
        }
    }
    out
}

enum Known {
    Age(u8),
    /// A label that says there is no rating yet.
    Unrated,
    Unknown,
}

fn lookup(board: &str, rating: &str) -> Known {
    let board = board.trim().to_ascii_lowercase();
    let mut label = rating.trim().to_ascii_uppercase();
    // "PEGI 18", "USK 12": the board's own name in front.
    if let Some(rest) = label.strip_prefix(board.to_ascii_uppercase().as_str()) {
        label = rest.trim_start_matches([' ', '-', '_', ':']).to_owned();
    }
    let age = match (board.as_str(), label.as_str()) {
        ("pegi", "3") => 3,
        ("pegi", "7") => 7,
        ("pegi", "12") => 12,
        ("pegi", "16") => 16,
        ("pegi", "18") => 18,
        ("esrb", "RP") => return Known::Unrated,
        ("esrb", "EC") => 3,
        ("esrb", "E") => 6,
        ("esrb", "E10+" | "E10") => 10,
        ("esrb", "T") => 13,
        ("esrb", "M") => 17,
        ("esrb", "AO") => 18,
        ("usk", "0") => 0,
        ("usk", "6") => 6,
        ("usk", "12") => 12,
        ("usk", "16") => 16,
        ("usk", "18") => 18,
        ("cero", "A") => 0,
        ("cero", "B") => 12,
        ("cero", "C") => 15,
        ("cero", "D") => 17,
        ("cero", "Z") => 18,
        ("acb", "G") => 0,
        ("acb", "PG") => 8,
        ("acb", "M") => 15,
        ("acb", "MA15+" | "MA15") => 15,
        ("acb", "R18+" | "R18" | "X18+" | "X18" | "RC") => 18,
        ("grac", "TESTING") => return Known::Unrated,
        ("grac", "ALL") => 0,
        ("grac", "12") => 12,
        ("grac", "15") => 15,
        ("grac", "18") => 18,
        ("classind", "L") => 0,
        ("classind", "10") => 10,
        ("classind", "12") => 12,
        ("classind", "14") => 14,
        ("classind", "16") => 16,
        ("classind", "18") => 18,
        _ => return Known::Unknown,
    };
    Known::Age(age)
}

/// Logs a rating the table doesn't know, the first time it comes up.
fn unknown_once(board: &str, rating: &str) {
    static SEEN: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);
    let mut seen = crate::lock(&SEEN);
    if seen
        .get_or_insert_with(HashSet::new)
        .insert((board.to_owned(), rating.to_owned()))
    {
        tracing::info!(
            board,
            rating,
            "age rating not in the table; counted as unrated"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rating(board: &str, rating: &str) -> IgdbAgeRating {
        IgdbAgeRating {
            board: board.to_owned(),
            rating: rating.to_owned(),
        }
    }

    #[test]
    fn every_known_rating_maps_to_an_age() {
        let table: &[(&str, &[(&str, u8)])] = &[
            (
                "pegi",
                &[("3", 3), ("7", 7), ("12", 12), ("16", 16), ("18", 18)],
            ),
            (
                "esrb",
                &[
                    ("EC", 3),
                    ("E", 6),
                    ("E10+", 10),
                    ("T", 13),
                    ("M", 17),
                    ("AO", 18),
                ],
            ),
            (
                "usk",
                &[("0", 0), ("6", 6), ("12", 12), ("16", 16), ("18", 18)],
            ),
            (
                "cero",
                &[("A", 0), ("B", 12), ("C", 15), ("D", 17), ("Z", 18)],
            ),
            (
                "acb",
                &[
                    ("G", 0),
                    ("PG", 8),
                    ("M", 15),
                    ("MA15+", 15),
                    ("R18+", 18),
                    ("X18+", 18),
                    ("RC", 18),
                ],
            ),
            ("grac", &[("All", 0), ("12", 12), ("15", 15), ("18", 18)]),
            (
                "classind",
                &[
                    ("L", 0),
                    ("10", 10),
                    ("12", 12),
                    ("14", 14),
                    ("16", 16),
                    ("18", 18),
                ],
            ),
        ];
        assert_eq!(table.len(), BOARDS.len());
        for (board, labels) in table {
            assert!(BOARDS.contains(board));
            for (label, age) in *labels {
                assert_eq!(
                    rating_age(&rating(board, label)),
                    Some(*age),
                    "{board} {label}"
                );
            }
        }
    }

    #[test]
    fn labels_compare_loosely() {
        assert_eq!(rating_age(&rating("PEGI", "PEGI 16")), Some(16));
        assert_eq!(rating_age(&rating("esrb", "e10")), Some(10));
        assert_eq!(rating_age(&rating("grac", "ALL")), Some(0));
        assert_eq!(rating_age(&rating(" usk ", " USK 12 ")), Some(12));
    }

    #[test]
    fn pending_and_unknown_ratings_are_unrated() {
        assert_eq!(rating_age(&rating("esrb", "RP")), None);
        assert_eq!(rating_age(&rating("grac", "Testing")), None);
        assert_eq!(rating_age(&rating("pegi", "21")), None);
        assert_eq!(rating_age(&rating("newboard", "18")), None);
        // Logged once, and still unrated the second time.
        assert_eq!(rating_age(&rating("newboard", "18")), None);
    }

    #[test]
    fn a_game_takes_its_highest_rating_and_none_is_unrated() {
        assert_eq!(min_age(&[]), None);
        assert_eq!(min_age(&[rating("esrb", "RP")]), None);
        assert_eq!(
            min_age(&[
                rating("pegi", "16"),
                rating("esrb", "M"),
                rating("cero", "C")
            ]),
            Some(17)
        );
        // Unrated: no rating at all is not "for everyone".
        assert_ne!(min_age(&[rating("odd", "x")]), Some(0));
        assert_eq!(min_age(&[rating("usk", "0")]), Some(0));
    }

    #[test]
    fn the_shown_rating_is_the_preferred_boards_then_pegis() {
        let ratings = [
            rating("esrb", "M"),
            rating("pegi", "18"),
            rating("odd", "x"),
        ];
        assert_eq!(shown(&ratings, None).unwrap().board, "pegi");
        assert_eq!(shown(&ratings, Some("esrb")).unwrap().board, "esrb");
        assert_eq!(shown(&ratings, Some("cero")).unwrap().board, "pegi");
        assert_eq!(shown(&ratings[2..], None).unwrap().board, "odd");
        assert!(shown(&[], None).is_none());
    }

    #[test]
    fn ratings_are_cleaned() {
        let cleaned = clean(&[
            rating(" PEGI ", " 18 "),
            rating("pegi", "18"),
            rating("", "M"),
            rating("esrb", " "),
        ]);
        assert_eq!(cleaned, [rating("pegi", "18")]);
    }
}
