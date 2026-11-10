//! Numbers, sizes, durations and dates in the language in use, as the
//! webview's `presentation.ts` writes them: [`intl`](crate::intl)'s
//! formats, with the webview's messages around them. Dates are on the
//! user's clock already: the caller converts from UTC.

use crate::intl::{self, Clock, DateTime};
use crate::{format_locale, t, tp};

/// A count or other number, with up to three decimals: `1,234`,
/// `1 234` in Swedish.
pub fn count(value: f64) -> String {
    intl::count(&format_locale(), value)
}

/// A number with between `min` and `max` decimals.
pub fn number(value: f64, min: usize, max: usize) -> String {
    intl::number(&format_locale(), value, min, max)
}

/// A percentage from 0 to 100, rounded: `42%`, `42 %` in Swedish; `—`
/// without one.
pub fn percent(value: Option<f64>) -> String {
    match value {
        Some(value) => intl::percent(&format_locale(), value),
        None => "—".to_owned(),
    }
}

/// A game's or a download's size: `1.5 GB`, `1,5 GB` in Swedish.
pub fn size(bytes: Option<u64>) -> String {
    match bytes {
        Some(bytes) => intl::size(&format_locale(), bytes),
        None => t!(note = "A game whose size isn't known", "Size unknown"),
    }
}

/// Binary bytes, as Storage and Statistics show them: `140 GiB`,
/// `1,8 TiB` in Swedish. The units' symbols are translated messages.
pub fn bytes(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return tp!("size", "Unknown");
    };
    let (size, unit) = intl::binary(&format_locale(), bytes);
    let unit = match unit {
        0 => tp!("byte unit", "B"),
        1 => tp!("byte unit", "KiB"),
        2 => tp!("byte unit", "MiB"),
        3 => tp!("byte unit", "GiB"),
        4 => tp!("byte unit", "TiB"),
        _ => tp!("byte unit", "PiB"),
    };
    t!(
        note = "A size and its unit: 1.5 GiB",
        "{size} {unit}",
        size = size,
        unit = unit
    )
}

/// A length of time: `0m`, `<1m`, `42m`, `3.5h` (`3,5h` in Swedish).
pub fn duration(seconds: u64) -> String {
    let locale = format_locale();
    intl::duration(&locale, seconds).unwrap_or_else(|| {
        t!(
            note = "Under a minute of playtime: <1m",
            "<{duration}",
            duration = intl::one_minute(&locale)
        )
    })
}

/// A game's total playtime: `3.5h played`, or `Not played` before any.
pub fn playtime(seconds: u64) -> String {
    if seconds == 0 {
        return t!(note = "A game's playtime, before any", "Not played");
    }
    t!(
        note = "A game's total playtime: 3.5h played",
        "{duration} played",
        duration = duration(seconds)
    )
}

/// A day: `Oct 28, 2026`, `28 okt. 2026` in Swedish.
pub fn day(at: DateTime) -> String {
    intl::day(&format_locale(), at)
}

/// A day and time: `Oct 28, 2026, 2:05 PM`.
pub fn date_time(at: DateTime) -> String {
    intl::date_time(&format_locale(), at)
}

/// When the last play started; `Never played` without one.
pub fn last_played(at: Option<DateTime>) -> String {
    match at {
        Some(at) => date_time(at),
        None => t!(note = "A game's last play, before any", "Never played"),
    }
}

/// A month of a year: `Jun 2026`, `juni 2026` in Swedish.
pub fn month(year: i32, month: u8) -> String {
    intl::month(&format_locale(), year, month)
}

/// A time of day: `14:05`, or `02:05 PM` where the language (or the Time
/// format setting) uses 12 hours.
pub fn time(at: DateTime, clock: Clock, seconds: bool) -> String {
    intl::time(&format_locale(), at, clock, seconds)
}

/// "Today", "Yesterday", "3 days ago", or the day from 60 days back.
/// `standalone: false` leaves the language's own case for inside a
/// sentence.
pub fn relative_day(at: DateTime, days: u64, standalone: bool) -> String {
    intl::relative_day(&format_locale(), days, standalone).unwrap_or_else(|| day(at))
}
