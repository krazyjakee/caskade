//! InternetShortcut INI metadata. No URL is opened or environment value expanded.
use super::{Limits, ParseError, Result};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InternetShortcut {
    pub url: String,
    pub working_dir: Option<String>,
    pub icon: Option<String>,
    pub icon_index: Option<i32>,
}
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<InternetShortcut> {
    limits.input(bytes)?;
    let text = if let Some(bytes) = bytes.strip_prefix(&[255, 254]) {
        if bytes.len() % 2 != 0 {
            return Err(ParseError::Encoding);
        }
        String::from_utf16(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| ParseError::Encoding)?
    } else if let Some(bytes) = bytes.strip_prefix(&[254, 255]) {
        if bytes.len() % 2 != 0 {
            return Err(ParseError::Encoding);
        }
        String::from_utf16(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_be_bytes(*b))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| ParseError::Encoding)?
    } else {
        std::str::from_utf8(bytes.strip_prefix(&[239, 187, 191]).unwrap_or(bytes))
            .map_err(|_| ParseError::Encoding)?
            .to_owned()
    };
    if text.contains('\0') {
        return Err(ParseError::Invalid("NUL in URL file".into()));
    }
    let mut active = false;
    let mut url = None;
    let mut working_dir = None;
    let mut icon = None;
    let mut icon_index = None;
    for (n, line) in text.lines().enumerate() {
        limits.check(n + 1, limits.max_nodes, "INI lines")?;
        limits.check(line.len(), limits.max_string_bytes, "line bytes")?;
        let line = line.trim();
        if line.starts_with([';', '#']) || line.is_empty() {
            continue;
        }
        if let Some(group) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            active = group.eq_ignore_ascii_case("InternetShortcut");
            continue;
        }
        if !active {
            continue;
        }
        let (k, v) = line
            .split_once('=')
            .ok_or_else(|| ParseError::Invalid("INI key/value".into()))?;
        match k.trim().to_ascii_lowercase().as_str() {
            "url" => url = Some(v.trim().to_owned()),
            "workingdirectory" => working_dir = Some(v.trim().to_owned()),
            "iconfile" => icon = Some(v.trim().to_owned()),
            "iconindex" => {
                icon_index = Some(
                    v.trim()
                        .parse()
                        .map_err(|_| ParseError::Invalid("IconIndex".into()))?,
                )
            }
            _ => (),
        }
    }
    Ok(InternetShortcut {
        url: url
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ParseError::Invalid("missing shortcut URL".into()))?,
        working_dir,
        icon,
        icon_index,
    })
}
