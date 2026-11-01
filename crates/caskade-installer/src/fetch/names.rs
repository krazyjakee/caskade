use std::{collections::BTreeMap, ffi::OsStr};

use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderMap};
use url::Url;

use super::{FetchError, Result};

pub fn safe_file_name(name: &str) -> Result<String> {
    let path = crate::safety::sanitize_path(name)?;
    if path.components().count() != 1
        || path.as_os_str() != OsStr::new(name)
        || name.eq_ignore_ascii_case(".state")
    {
        return Err(FetchError::InvalidRequest {
            message: "download name must be one safe path component".into(),
        });
    }
    Ok(name.into())
}

/// RFC 6266/5987: filename* has precedence; quoted separators and backslash
/// escapes are parsed before validating the resulting single path component.
pub fn disposition_name(value: &str) -> Result<Option<String>> {
    let mut parts = vec![];
    let mut part = String::new();
    let (mut quoted, mut escaped) = (false, false);
    for ch in value.chars() {
        if escaped {
            part.push(ch);
            escaped = false;
        } else if quoted && ch == '\\' {
            escaped = true;
            part.push(ch);
        } else if ch == '"' {
            quoted = !quoted;
            part.push(ch);
        } else if ch == ';' && !quoted {
            parts.push(std::mem::take(&mut part));
        } else {
            part.push(ch);
        }
    }
    if quoted || escaped {
        return Ok(None);
    }
    parts.push(part);
    let mut params = BTreeMap::new();
    for part in parts.into_iter().skip(1) {
        let Some((key, raw)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if !matches!(key.as_str(), "filename" | "filename*") {
            continue;
        }
        if params.contains_key(&key) {
            return Ok(None);
        }
        let raw = raw.trim();
        let value = if let Some(quoted) = raw
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
        {
            let mut value = String::new();
            let mut chars = quoted.chars();
            while let Some(ch) = chars.next() {
                value.push(if ch == '\\' {
                    chars.next().unwrap_or(ch)
                } else {
                    ch
                });
            }
            value
        } else {
            raw.to_owned()
        };
        params.insert(key, value);
    }
    if let Some(encoded) = params.get("filename*") {
        let fields: Vec<_> = encoded.splitn(3, '\'').collect();
        if fields.len() == 3
            && let Ok(bytes) = percent_decode(fields[2])
        {
            let name = if fields[0].eq_ignore_ascii_case("utf-8") {
                String::from_utf8(bytes).ok()
            } else if fields[0].eq_ignore_ascii_case("iso-8859-1") {
                Some(bytes.into_iter().map(char::from).collect())
            } else {
                None
            };
            if let Some(name) = name {
                return safe_file_name(&name).map(Some);
            }
        }
    }
    params
        .get("filename")
        .map(|name| safe_file_name(name))
        .transpose()
}

pub(super) fn percent_decode(value: &str) -> Result<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut decoded = vec![];
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let byte = bytes
                .get(index + 1..index + 3)
                .and_then(|pair| std::str::from_utf8(pair).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                .ok_or_else(|| FetchError::InvalidRequest {
                    message: "invalid filename percent encoding".into(),
                })?;
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Ok(decoded)
}

pub(super) fn response_name(
    headers: &HeaderMap,
    final_url: &Url,
    hint: Option<&str>,
) -> Result<String> {
    if let Some(value) = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|header| header.to_str().ok())
        && let Some(name) = disposition_name(value)?
    {
        return Ok(name);
    }
    if let Some(name) = hint {
        return safe_file_name(name);
    }
    if let Some(segment) = final_url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        && !segment.is_empty()
    {
        let name = String::from_utf8(percent_decode(segment)?).map_err(|_| {
            FetchError::InvalidRequest {
                message: "download URL name is not UTF-8".into(),
            }
        })?;
        return safe_file_name(&name);
    }
    let extension = match headers
        .get(CONTENT_TYPE)
        .and_then(|header| header.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
    {
        "application/zip" => "zip",
        "application/x-7z-compressed" => "7z",
        "application/x-tar" => "tar",
        "application/gzip" => "tar.gz",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => "exe",
        "application/x-apple-diskimage" => "dmg",
        _ => "bin",
    };
    Ok(format!("download.{extension}"))
}
