//! Numbers, sizes, durations, dates and times as the webview shows them
//! (`apps/desktop/src/api/presentation.ts`, which uses the browser's
//! `Intl`), for the native UI. The output matches `Intl`'s for each
//! language with data here: English (US, and the UK's dates and 24-hour
//! clock) and Swedish. `intl_oracle.rs`, generated from Node's `Intl` by
//! `scripts/intl-oracle.mjs`, checks it. Another language falls back to US
//! English until its row is added.
//!
//! Messages around these (`<1m`, `3.5h played`, `Never played`) are the
//! UI's, translated with the rest of its text.

/// A date and time on the user's clock: the caller converts from UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: i32,
    /// 1 to 12.
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// The Time format setting (`core.general.timeFormat`): the language's own
/// clock, or 12 or 24 hours whatever the language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clock {
    #[default]
    System,
    H12,
    H23,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateOrder {
    /// `Oct 28, 2026`.
    MonthDayYear,
    /// `28 Oct 2026`.
    DayMonthYear,
}

/// One language's (and region's) way of writing these.
#[derive(Debug)]
struct Data {
    decimal: &'static str,
    group: &'static str,
    minus: &'static str,
    /// Between a percentage and its sign.
    percent: &'static str,
    months: [&'static str; 12],
    order: DateOrder,
    /// Between the date and the time.
    date_time: &'static str,
    /// The clock the language uses, when the setting doesn't say.
    twelve_hour: bool,
    am: &'static str,
    pm: &'static str,
    today: &'static str,
    yesterday: &'static str,
    /// Two days ago, where the language has a word for it.
    day_before_yesterday: Option<&'static str>,
    /// `{n}` days ago, for two or more.
    days_ago: &'static str,
    last_week: &'static str,
    weeks_ago: &'static str,
}

const EN_MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

static EN_US: Data = Data {
    decimal: ".",
    group: ",",
    minus: "-",
    percent: "",
    months: EN_MONTHS,
    order: DateOrder::MonthDayYear,
    date_time: ", ",
    twelve_hour: true,
    am: "AM",
    pm: "PM",
    today: "today",
    yesterday: "yesterday",
    day_before_yesterday: None,
    days_ago: "{n} days ago",
    last_week: "last week",
    weeks_ago: "{n} weeks ago",
};

static EN_GB: Data = Data {
    decimal: ".",
    group: ",",
    minus: "-",
    percent: "",
    months: [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sept", "Oct", "Nov", "Dec",
    ],
    order: DateOrder::DayMonthYear,
    date_time: ", ",
    twelve_hour: false,
    am: "am",
    pm: "pm",
    today: "today",
    yesterday: "yesterday",
    day_before_yesterday: None,
    days_ago: "{n} days ago",
    last_week: "last week",
    weeks_ago: "{n} weeks ago",
};

static SV: Data = Data {
    decimal: ",",
    group: "\u{a0}",
    minus: "\u{2212}",
    percent: "\u{a0}",
    months: [
        "jan.", "feb.", "mars", "apr.", "maj", "juni", "juli", "aug.", "sep.", "okt.", "nov.",
        "dec.",
    ],
    order: DateOrder::DayMonthYear,
    date_time: " ",
    twelve_hour: false,
    am: "fm",
    pm: "em",
    today: "i dag",
    yesterday: "i går",
    day_before_yesterday: Some("i förrgår"),
    days_ago: "för {n} dagar sedan",
    last_week: "förra veckan",
    weeks_ago: "för {n} veckor sedan",
};

/// Regions whose English writes dates day first and uses a 24-hour clock,
/// as `Intl` has them like the UK's.
const UK_LIKE: [&str; 3] = ["GB", "IE", "NZ"];

fn data(locale: &str) -> &'static Data {
    let mut parts = locale.split(['-', '_']);
    let language = parts.next().unwrap_or("").to_ascii_lowercase();
    let region = parts
        .find(|part| part.len() == 2)
        .map(str::to_ascii_uppercase);
    match language.as_str() {
        "sv" => &SV,
        "en" if region
            .as_deref()
            .is_some_and(|region| UK_LIKE.contains(&region)) =>
        {
            &EN_GB
        }
        _ => &EN_US,
    }
}

/// Whether `locale` has data here, rather than US English's.
pub fn supported(locale: &str) -> bool {
    let language = locale.split(['-', '_']).next().unwrap_or("");
    matches!(language.to_ascii_lowercase().as_str(), "en" | "sv")
}

/// `value` rounded half away from zero to `digits` decimals, as `Intl`
/// rounds, as integer and fraction digits.
fn digits(value: f64, min: usize, max: usize) -> (bool, String, String) {
    let negative = value < 0.0;
    let scale = 10f64.powi(max as i32);
    let scaled = (value.abs() * scale).round();
    let text = format!("{scaled:.0}");
    let text = if text.len() <= max {
        format!("{}{text}", "0".repeat(max + 1 - text.len()))
    } else {
        text
    };
    let (whole, fraction) = text.split_at(text.len() - max);
    let mut fraction = fraction.to_owned();
    while fraction.len() > min && fraction.ends_with('0') {
        fraction.pop();
    }
    let zero = scaled == 0.0;
    (negative && !zero, whole.to_owned(), fraction)
}

fn group(whole: &str, separator: &str) -> String {
    let mut out = String::with_capacity(whole.len() + whole.len() / 3);
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            out.push_str(separator);
        }
        out.push(c);
    }
    out
}

/// `value` with between `min` and `max` decimals, grouped: `1,234.5`,
/// `1 234,5` in Swedish.
pub fn number(locale: &str, value: f64, min: usize, max: usize) -> String {
    let data = data(locale);
    let (negative, whole, fraction) = digits(value, min, max);
    let mut out = String::new();
    if negative {
        out.push_str(data.minus);
    }
    out.push_str(&group(&whole, data.group));
    if !fraction.is_empty() {
        out.push_str(data.decimal);
        out.push_str(&fraction);
    }
    out
}

/// A count: up to three decimals, as `Intl.NumberFormat`'s default.
pub fn count(locale: &str, value: f64) -> String {
    number(locale, value, 0, 3)
}

/// A percentage from 0 to 100, rounded: `42%`, `42 %` in Swedish.
pub fn percent(locale: &str, value: f64) -> String {
    let data = data(locale);
    format!("{}{}%", number(locale, value, 0, 0), data.percent)
}

const SIZE_UNITS: [&str; 5] = ["kB", "MB", "GB", "TB", "PB"];

/// A game's or a download's size: `1.5 GB`, `1,5 GB` in Swedish; `0.0 kB`
/// at the least. (Units of 1024, named as the webview names them.)
pub fn size(locale: &str, bytes: u64) -> String {
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < SIZE_UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{} {}", number(locale, value, 1, 1), SIZE_UNITS[unit])
}

/// Binary bytes' number part and unit index (B, KiB, MiB, …): the unit's
/// symbol is a translated message, so the UI joins them.
pub fn binary(locale: &str, bytes: u64) -> (String, usize) {
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < 5 {
        value /= 1024.0;
        unit += 1;
    }
    let digits = if unit == 0 || value >= 100.0 { 0 } else { 1 };
    (number(locale, value, digits, digits), unit)
}

/// A length of time: `0m`, `42m`, `3.5h` (`3,5h` in Swedish). Under a
/// minute is `None`: the UI shows its `<1m` message.
pub fn duration(locale: &str, seconds: u64) -> Option<String> {
    match seconds {
        0 => Some("0m".to_owned()),
        1..60 => None,
        60..3600 => Some(format!("{}m", number(locale, (seconds / 60) as f64, 0, 0))),
        _ => {
            let hours = (seconds as f64 / 360.0).round() / 10.0;
            Some(format!("{}h", number(locale, hours, 1, 1)))
        }
    }
}

/// The `1m` inside the `<1m` message.
pub fn one_minute(locale: &str) -> String {
    format!("{}m", number(locale, 1.0, 0, 0))
}

/// A day: `Oct 28, 2026`, `28 Oct 2026` in the UK, `28 okt. 2026` in
/// Swedish (`dateStyle: "medium"`).
pub fn day(locale: &str, date: DateTime) -> String {
    let data = data(locale);
    let month = data.months[usize::from(date.month.clamp(1, 12)) - 1];
    match data.order {
        DateOrder::MonthDayYear => format!("{month} {}, {}", date.day, date.year),
        DateOrder::DayMonthYear => format!("{} {month} {}", date.day, date.year),
    }
}

/// A month of a year: `Jun 2026`, `juni 2026` in Swedish.
pub fn month(locale: &str, year: i32, month: u8) -> String {
    let data = data(locale);
    format!(
        "{} {year}",
        data.months[usize::from(month.clamp(1, 12)) - 1]
    )
}

fn twelve(data: &Data, clock: Clock) -> bool {
    match clock {
        Clock::System => data.twelve_hour,
        Clock::H12 => true,
        Clock::H23 => false,
    }
}

/// A time of day, two digits each: `02:05 PM`, `14:05`; `seconds` adds
/// them (`hour: "2-digit", minute: "2-digit"`).
pub fn time(locale: &str, at: DateTime, clock: Clock, seconds: bool) -> String {
    let data = data(locale);
    let twelve = twelve(data, clock);
    let hour = if twelve {
        match at.hour % 12 {
            0 => 12,
            hour => hour,
        }
    } else {
        at.hour
    };
    let mut out = format!("{hour:02}:{:02}", at.minute);
    if seconds {
        out.push_str(&format!(":{:02}", at.second));
    }
    if twelve {
        out.push(' ');
        out.push_str(if at.hour < 12 { data.am } else { data.pm });
    }
    out
}

/// A day and time: `Oct 28, 2026, 2:05 PM`, `28 okt. 2026 14:05`
/// (`dateStyle: "medium", timeStyle: "short"`).
pub fn date_time(locale: &str, at: DateTime) -> String {
    let data = data(locale);
    let time = if data.twelve_hour {
        let hour = match at.hour % 12 {
            0 => 12,
            hour => hour,
        };
        let half = if at.hour < 12 { data.am } else { data.pm };
        format!("{hour}:{:02} {half}", at.minute)
    } else {
        format!("{:02}:{:02}", at.hour, at.minute)
    };
    format!("{}{}{time}", day(locale, at), data.date_time)
}

/// How long ago a day was, `days` before today: `today`, `yesterday`,
/// `3 days ago`, `last week`, `5 weeks ago` (`Intl.RelativeTimeFormat`
/// with `numeric: "auto"`); `None` from 60 days, where the UI shows the
/// date. `standalone` capitalizes it, as the start of a label.
pub fn relative_day(locale: &str, days: u64, standalone: bool) -> Option<String> {
    let data = data(locale);
    let text = match days {
        0 => data.today.to_owned(),
        1 => data.yesterday.to_owned(),
        2 if data.day_before_yesterday.is_some() => {
            data.day_before_yesterday.unwrap_or_default().to_owned()
        }
        2..14 => data
            .days_ago
            .replace("{n}", &number(locale, days as f64, 0, 0)),
        14..60 => {
            let weeks = days / 7;
            if weeks == 1 {
                data.last_week.to_owned()
            } else {
                data.weeks_ago
                    .replace("{n}", &number(locale, weeks as f64, 0, 0))
            }
        }
        _ => return None,
    };
    Some(if standalone {
        let mut chars = text.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect())
            .unwrap_or_default()
    } else {
        text
    })
}

/// The CLDR plural category of `n` in `locale`, for messages' plurals:
/// English and Swedish (and an unknown language) have `one` for 1 and
/// `other` for the rest.
pub fn plural_category(locale: &str, n: f64) -> &'static str {
    let language = locale.split(['-', '_']).next().unwrap_or("");
    match language {
        "fr" | "pt" if n.fract() == 0.0 && (n == 0.0 || n == 1.0) => "one",
        "fr" | "pt" => "other",
        _ if n == 1.0 => "one",
        _ => "other",
    }
}

#[cfg(test)]
#[path = "intl_oracle.rs"]
mod oracle;
