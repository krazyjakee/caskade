//! HTTP for achievement services: the store accounts' client, with the
//! answer's status and body kept so a provider can read the service's own
//! errors (a private profile, a game without achievements). Waits, server
//! failures and unreachable services become [`ProviderError`]s here.

use crate::store_accounts::{ProviderError, http};
use reqwest::{RequestBuilder, StatusCode};

pub(crate) use http::{client, unexpected, unreadable};

/// At most this much of an answer is read.
const MAX_BODY: usize = 32 * 1024 * 1024;

/// Sends `request`: its status and body when the service answered,
/// `RateLimited` for 429, `Offline` for 5xx or no answer. `store` names the
/// service in errors; URLs (they carry keys) never appear.
pub(crate) async fn send(
    request: RequestBuilder,
    store: &str,
) -> Result<(StatusCode, Vec<u8>), ProviderError> {
    let response = http::send(request).await?;
    let status = response.status();
    if status.is_server_error() {
        return Err(ProviderError::Offline(http::answered(store, status)));
    }
    Ok((status, http::read_body(response, store, MAX_BODY).await?))
}

/// An ISO 8601 time (`2018-03-10T21:21:51+0000`, `…Z`, `2024-01-31 18:05:00`
/// as UTC) as Unix milliseconds.
pub(crate) fn iso_ms(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, rest) = text.split_once(['T', ' '])?;
    let mut parts = date.split('-').map(|part| part.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    // The time, then an offset: `Z`, `+hh:mm`, `+hhmm` or none.
    let split = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let (time, offset) = rest.split_at(split);
    let mut time = time.split(':').map(|part| {
        part.split('.')
            .next()
            .and_then(|whole| whole.parse::<i64>().ok())
    });
    let (hh, mm, ss) = (
        time.next()??,
        time.next()??,
        time.next().flatten().unwrap_or(0),
    );
    let offset_minutes = match offset.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let digits: String = offset[1..].chars().filter(char::is_ascii_digit).collect();
            let hours: i64 = digits.get(..2)?.parse().ok()?;
            let minutes: i64 = digits.get(2..4).unwrap_or("0").parse().unwrap_or(0);
            let total = hours * 60 + minutes;
            if sign == '-' { -total } else { total }
        }
        _ => 0,
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((((days * 24 + hh) * 60 + mm - offset_minutes) * 60 + ss) * 1000)
}

#[cfg(test)]
mod tests {
    #[test]
    fn iso_times_with_and_without_offsets() {
        assert_eq!(super::iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(super::iso_ms("1970-01-01 00:00:00"), Some(0));
        assert_eq!(
            super::iso_ms("2024-02-29 12:30:15"),
            Some(1_709_209_815_000)
        );
        assert_eq!(super::iso_ms("1970-01-01T01:00:00+0100"), Some(0));
        assert_eq!(super::iso_ms("1969-12-31T23:30:00-00:30"), Some(0));
        assert_eq!(
            super::iso_ms("2024-02-29T12:30:15+0000"),
            Some(1_709_209_815_000)
        );
        assert_eq!(super::iso_ms("soon"), None);
    }
}
