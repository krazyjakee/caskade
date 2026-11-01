//! Playtime and last played from Heroic's `store/timestamp.json`:
//! `{ "<app>": { "firstPlayed": ISO-8601, "lastPlayed": ISO-8601,
//! "totalPlayed": minutes } }`.

use super::{super::join, load};
use crate::{env::HostEnv, types::ScanCtx};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Played {
    pub playtime_secs: Option<u64>,
    pub last_played_unix: Option<u64>,
}

pub(super) fn played(env: &HostEnv, dir: &Path, ctx: &ScanCtx) -> BTreeMap<String, Played> {
    let Some(value) = load(env, &join(dir, "store/timestamp.json"), ctx) else {
        return BTreeMap::new();
    };
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(app, record)| {
            let minutes = record
                .get("totalPlayed")
                .and_then(Value::as_f64)
                .filter(|minutes| *minutes > 0.0);
            let played = Played {
                playtime_secs: minutes.map(|minutes| (minutes * 60.0).round() as u64),
                last_played_unix: record
                    .get("lastPlayed")
                    .and_then(Value::as_str)
                    .and_then(unix_seconds),
            };
            (played != Played::default()).then(|| (app.clone(), played))
        })
        .collect()
}

/// Seconds since the Unix epoch for an ISO-8601 UTC time such as
/// `2023-11-14T22:13:20.000Z`. Offsets other than `Z` are not used by
/// Heroic and are rejected.
pub(crate) fn unix_seconds(text: &str) -> Option<u64> {
    let text = text.trim();
    let (date, time) = text.split_once('T')?;
    let mut date = date.splitn(3, '-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (date.next()??, date.next()??, date.next()??);
    let time = time.strip_suffix('Z')?;
    let time = time.split('.').next()?;
    let mut time = time.splitn(3, ':').map(|part| part.parse::<i64>().ok());
    let (hour, minute, second) = (time.next()??, time.next()??, time.next()??);
    let valid = (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute)
        && (0..61).contains(&second);
    if !valid {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    u64::try_from(days * 86_400 + hour * 3_600 + minute * 60 + second).ok()
}

#[cfg(test)]
mod tests {
    use super::unix_seconds;

    #[test]
    fn iso_times() {
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            unix_seconds("2023-11-14T22:13:20.000Z"),
            Some(1_700_000_000)
        );
        assert_eq!(unix_seconds("2024-02-29T12:00:00Z"), Some(1_709_208_000));
        assert_eq!(unix_seconds("2023-11-14T22:13:20+01:00"), None);
        assert_eq!(unix_seconds("2023-13-01T00:00:00Z"), None);
        assert_eq!(unix_seconds("1969-12-31T23:59:59Z"), None);
        assert_eq!(unix_seconds("garbage"), None);
    }
}
