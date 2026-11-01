use std::collections::BTreeMap;

use reqwest::{
    Client, Response, StatusCode,
    header::{ACCEPT_ENCODING, HeaderMap, HeaderName, HeaderValue, IF_RANGE, LOCATION, RANGE},
};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::{FetchError, Result};

pub(super) fn cancelled(token: &CancellationToken) -> Result<()> {
    if token.is_cancelled() {
        Err(FetchError::Cancelled)
    } else {
        Ok(())
    }
}

pub(super) fn redirect_target(current: &Url, location: &str, redirects: usize) -> Result<Url> {
    if redirects >= 10 {
        return Err(FetchError::RedirectLimit);
    }
    let mut target = current
        .join(location)
        .map_err(|_| FetchError::InvalidResponse {
            message: "invalid redirect location".into(),
        })?;
    if !matches!(target.scheme(), "http" | "https") {
        return Err(FetchError::UnsupportedScheme);
    }
    if current.scheme() == "https" && target.scheme() == "http" {
        return Err(FetchError::InsecureRedirect);
    }
    target.set_fragment(None);
    Ok(target)
}

pub(super) async fn get(
    client: &Client,
    source: &Url,
    headers: &BTreeMap<String, String>,
    resume: Option<(u64, &str)>,
    token: &CancellationToken,
    grant: Option<&[String]>,
) -> Result<Response> {
    let mut url = source.clone();
    url.set_fragment(None);
    if let Some(grant) = grant {
        super::grant::check(grant, &url)?;
    }
    let mut custom = HeaderMap::new();
    for (name, value) in headers {
        let name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| FetchError::InvalidRequest {
                message: "invalid download header name".into(),
            })?;
        if matches!(
            name.as_str(),
            "host" | "range" | "if-range" | "content-length" | "accept-encoding"
        ) {
            return Err(FetchError::InvalidRequest {
                message: "resolver attempted to replace a controlled HTTP header".into(),
            });
        }
        custom.insert(
            name,
            HeaderValue::from_str(value).map_err(|_| FetchError::InvalidRequest {
                message: "invalid download header value".into(),
            })?,
        );
    }
    for redirects in 0..=10 {
        cancelled(token)?;
        let mut request = client
            .get(url.clone())
            .headers(custom.clone())
            .header(ACCEPT_ENCODING, "identity");
        if let Some((offset, validator)) = resume {
            request = request
                .header(RANGE, format!("bytes={offset}-"))
                .header(IF_RANGE, validator);
        }
        let response = tokio::select! {
            _ = token.cancelled() => return Err(FetchError::Cancelled),
            response = request.send() => response.map_err(|error| {
                match super::grant::refused(&error) {
                    Some(host) => FetchError::NotGranted { host },
                    None => FetchError::http(error),
                }
            })?,
        };
        if matches!(
            response.status(),
            StatusCode::MOVED_PERMANENTLY
                | StatusCode::FOUND
                | StatusCode::SEE_OTHER
                | StatusCode::TEMPORARY_REDIRECT
                | StatusCode::PERMANENT_REDIRECT
        ) {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| FetchError::InvalidResponse {
                    message: "redirect has no valid Location header".into(),
                })?;
            let next = redirect_target(&url, location, redirects)?;
            if let Some(grant) = grant {
                super::grant::check(grant, &next)?;
            }
            if next.origin() != url.origin() {
                custom.clear();
            }
            url = next;
        } else {
            return Ok(response);
        }
    }
    Err(FetchError::RedirectLimit)
}

pub(super) fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

pub(super) fn is_html(headers: &HeaderMap) -> bool {
    header(headers, "content-type").is_some_and(|value| {
        matches!(
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "text/html" | "application/xhtml+xml"
        )
    })
}

pub(super) fn html_bytes(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let text = String::from_utf8_lossy(bytes)
        .trim_start()
        .to_ascii_lowercase();
    text.starts_with("<!doctype html")
        || text.starts_with("<html")
        || text.starts_with("<head")
        || text.starts_with("<body")
}

pub(super) fn range(value: &str) -> Result<(u64, u64, Option<u64>)> {
    let invalid = || FetchError::InvalidResponse {
        message: "invalid Content-Range".into(),
    };
    let (span, total) = value
        .strip_prefix("bytes ")
        .and_then(|value| value.split_once('/'))
        .ok_or_else(invalid)?;
    let (start, end) = span.split_once('-').ok_or_else(invalid)?;
    let start: u64 = start.parse().map_err(|_| invalid())?;
    let end: u64 = end.parse().map_err(|_| invalid())?;
    let total: Option<u64> = if total == "*" {
        None
    } else {
        Some(total.parse().map_err(|_| invalid())?)
    };
    if end < start || total.is_some_and(|total| end >= total) {
        return Err(invalid());
    }
    Ok((start, end, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_reject_tls_downgrade_and_limit_chains() {
        let secure = Url::parse("https://example.com/releases/game.zip").unwrap();
        assert!(matches!(
            redirect_target(&secure, "http://example.com/game.zip", 0),
            Err(FetchError::InsecureRedirect)
        ));
        assert!(matches!(
            redirect_target(&secure, "/next", 10),
            Err(FetchError::RedirectLimit)
        ));
        assert_eq!(
            redirect_target(&secure, "../next.zip", 9).unwrap().as_str(),
            "https://example.com/next.zip"
        );
        assert!(matches!(
            redirect_target(&secure, "file:///etc/passwd", 0),
            Err(FetchError::UnsupportedScheme)
        ));
    }

    #[test]
    fn malformed_content_ranges_are_not_appended() {
        assert_eq!(range("bytes 4-9/10").unwrap(), (4, 9, Some(10)));
        for value in [
            "bytes 10-4/11",
            "bytes 0-10/10",
            "bytes */10",
            "items 0-4/5",
            "bytes bad",
        ] {
            assert!(range(value).is_err());
        }
    }
}
