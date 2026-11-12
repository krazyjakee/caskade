//! How the Store shows IGDB release dates (the webview's
//! `views/store/releases.ts`), honestly: only as much of a date as IGDB
//! knows. A release's `date` is the UTC day IGDB files it under; for month,
//! quarter and year precision it only stands in for the span, so the label
//! shows the span. Days are shown as the UTC day, never shifted into the
//! local time zone.

use caskade_core::igdb::{IgdbCatalogPage, IgdbGame, IgdbReleaseDate, ReleasePrecision};
use caskade_ui_i18n::t;
use std::collections::{HashMap, HashSet};

fn tbd() -> String {
    t!(
        note = "A release date that isn't known yet (to be determined)",
        "TBD"
    )
}

/// `YYYY-MM-DD` as the language writes a day; the text as it came when it
/// isn't one.
pub fn day(date: &str) -> String {
    let mut parts = date.splitn(3, '-').map(|part| part.parse::<u32>().ok());
    match (
        parts.next().flatten(),
        parts.next().flatten(),
        parts.next().flatten(),
    ) {
        (Some(year), Some(month @ 1..=12), Some(day @ 1..=31)) => {
            caskade_ui_i18n::format::day(caskade_ui_i18n::intl::DateTime {
                year: year as i32,
                month: month as u8,
                day: day as u8,
                hour: 0,
                minute: 0,
                second: 0,
            })
        }
        _ => date.to_owned(),
    }
}

/// When the release is, in the app's language: "Mar 14, 2024", "Jun 2027",
/// "Q1 2027", "2026" or "TBD".
pub fn release_when(release: &IgdbReleaseDate) -> String {
    let known = match release.precision {
        ReleasePrecision::Day => release
            .date
            .as_deref()
            .filter(|date| {
                let parts: Vec<u32> = date.split('-').filter_map(|part| part.parse().ok()).collect();
                matches!(parts.as_slice(), [year, month, day] if *year > 0 && *month > 0 && *day > 0)
            })
            .map(day),
        ReleasePrecision::Month => match (release.year, release.month) {
            (Some(year), Some(month @ 1..=12)) => Some(caskade_ui_i18n::format::month(year, month)),
            _ => None,
        },
        ReleasePrecision::Quarter => match (release.year, release.quarter) {
            (Some(year), Some(quarter)) => Some(t!(
                note = "A release date known to the quarter of a year: Q1 2027",
                "Q{quarter} {year}",
                quarter = quarter,
                year = year
            )),
            _ => None,
        },
        ReleasePrecision::Year => release.year.map(|year| year.to_string()),
        ReleasePrecision::Tbd => return tbd(),
    };
    known.unwrap_or_else(|| {
        release
            .human
            .as_deref()
            .map(str::trim)
            .filter(|human| !human.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(tbd)
    })
}

/// What a release's status says about it, where IGDB says anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseKind {
    Released,
    EarlyAccess,
    Cancelled,
    InDevelopment,
    Other,
}

impl ReleaseKind {
    /// Its `data-release` name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Released => "released",
            Self::EarlyAccess => "early-access",
            Self::Cancelled => "cancelled",
            Self::InDevelopment => "in-development",
            Self::Other => "other",
        }
    }
}

pub fn release_kind(release: &IgdbReleaseDate) -> Option<ReleaseKind> {
    let status = release.status.as_deref()?.trim().to_lowercase();
    if status.is_empty() {
        return None;
    }
    Some(if status.starts_with("cancel") {
        ReleaseKind::Cancelled
    } else if status.contains("early access") {
        ReleaseKind::EarlyAccess
    } else if status == "full release" {
        ReleaseKind::Released
    } else if status == "alpha" || status == "beta" {
        ReleaseKind::InDevelopment
    } else {
        ReleaseKind::Other
    })
}

/// IGDB's status in the language in use; one Caskade doesn't know as it
/// came.
pub fn status_name(status: &str) -> String {
    match status.trim().to_lowercase().as_str() {
        "released" => t!(note = "A game's or release's status on IGDB", "Released"),
        "full release" => t!(
            note = "A game's or release's status on IGDB",
            "Full release"
        ),
        "alpha" => t!(note = "A game's or release's status on IGDB", "Alpha"),
        "beta" => t!(note = "A game's or release's status on IGDB", "Beta"),
        "early access" => t!(note = "A game release's status", "Early access"),
        "offline" => t!(note = "A game's or release's status on IGDB", "Offline"),
        "cancelled" => t!(note = "A game release's status", "Cancelled"),
        "rumored" => t!(note = "A game's or release's status on IGDB", "Rumoured"),
        "delisted" => t!(note = "A game's or release's status on IGDB", "Delisted"),
        _ => status.to_owned(),
    }
}

/// A release's region in the language in use; an unknown one with its
/// words capitalised.
pub fn region_name(region: Option<&str>) -> Option<String> {
    let region = region?;
    Some(match region {
        "worldwide" => t!(note = "A game release's region", "Worldwide"),
        "europe" => t!(note = "A game release's region", "Europe"),
        "north_america" => t!(note = "A game release's region", "North America"),
        "australia" => t!(note = "A game release's region", "Australia"),
        "new_zealand" => t!(note = "A game release's region", "New Zealand"),
        "japan" => t!(note = "A game release's region", "Japan"),
        "china" => t!(note = "A game release's region", "China"),
        "asia" => t!(note = "A game release's region", "Asia"),
        "korea" => t!(note = "A game release's region", "Korea"),
        "brazil" => t!(note = "A game release's region", "Brazil"),
        other => other
            .split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            })
            .collect::<Vec<String>>()
            .join(" "),
    })
}

/// "PS5 · Europe": the platform and region a release is for, where known.
pub fn release_where(release: &IgdbReleaseDate) -> String {
    let platform = release.platform.as_ref().and_then(|platform| {
        platform
            .abbreviation
            .clone()
            .filter(|name| !name.is_empty())
            .or_else(|| platform.name.clone())
    });
    let region = match release.region.as_deref() {
        Some("worldwide") => None,
        region => region_name(region),
    };
    [platform, region]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The platforms line of a card: abbreviations, at most three and a count.
pub fn platforms_line(game: &IgdbGame) -> String {
    let names: Vec<String> = game
        .platforms
        .iter()
        .filter_map(|platform| {
            platform
                .abbreviation
                .clone()
                .filter(|name| !name.is_empty())
                .or_else(|| platform.name.clone())
        })
        .collect();
    if names.len() <= 3 {
        names.join(" · ")
    } else {
        format!("{} +{}", names[..3].join(" · "), names.len() - 3)
    }
}

/// A catalogue game as a list shows it: with the release that listed it,
/// on the release lists.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub game: IgdbGame,
    pub release: Option<IgdbReleaseDate>,
}

/// The pages' games without repeats, each with the release that listed it
/// first: release pages count release rows, so a game can come up again on
/// a later page for another platform.
pub fn listed_games(pages: &[IgdbCatalogPage]) -> Vec<Listed> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for page in pages {
        let mut releases: HashMap<u64, IgdbReleaseDate> = HashMap::new();
        for listed in &page.releases {
            releases
                .entry(listed.game)
                .or_insert_with(|| listed.release.clone());
        }
        for game in &page.games {
            if seen.insert(game.id) {
                out.push(Listed {
                    game: game.clone(),
                    release: releases.get(&game.id).cloned(),
                });
            }
        }
    }
    out
}

/// A game's releases in the order to list them: dated ones by date, then
/// the undated.
pub fn sorted_releases(game: &IgdbGame) -> Vec<IgdbReleaseDate> {
    let mut releases = game.release_dates.clone();
    releases.sort_by(|a, b| match (&a.date, &b.date) {
        (Some(a), Some(b)) => a.cmp(b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    releases
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(precision: ReleasePrecision) -> IgdbReleaseDate {
        IgdbReleaseDate {
            precision,
            ..IgdbReleaseDate::default()
        }
    }

    #[test]
    fn a_release_shows_only_as_much_of_its_date_as_igdb_knows() {
        let quarter = IgdbReleaseDate {
            year: Some(2027),
            quarter: Some(1),
            ..release(ReleasePrecision::Quarter)
        };
        assert_eq!(release_when(&quarter), "Q1 2027");
        let year = IgdbReleaseDate {
            year: Some(2026),
            ..release(ReleasePrecision::Year)
        };
        assert_eq!(release_when(&year), "2026");
        assert_eq!(release_when(&release(ReleasePrecision::Tbd)), "TBD");
        // A precision without its parts falls back to IGDB's own words.
        let broken = IgdbReleaseDate {
            human: Some("Spring".into()),
            ..release(ReleasePrecision::Month)
        };
        assert_eq!(release_when(&broken), "Spring");
    }

    #[test]
    fn statuses_and_regions_read_as_the_webview_reads_them() {
        let with = |status: &str| IgdbReleaseDate {
            status: Some(status.into()),
            ..IgdbReleaseDate::default()
        };
        assert_eq!(
            release_kind(&with("Cancelled")),
            Some(ReleaseKind::Cancelled)
        );
        assert_eq!(
            release_kind(&with("Early Access")),
            Some(ReleaseKind::EarlyAccess)
        );
        assert_eq!(
            release_kind(&with("Full Release")),
            Some(ReleaseKind::Released)
        );
        assert_eq!(
            release_kind(&with("Beta")),
            Some(ReleaseKind::InDevelopment)
        );
        assert_eq!(release_kind(&with("")), None);
        assert_eq!(
            region_name(Some("north_america")).as_deref(),
            Some("North America")
        );
        assert_eq!(
            region_name(Some("south_pole")).as_deref(),
            Some("South Pole")
        );
        assert_eq!(status_name("Something new"), "Something new");
    }
}
