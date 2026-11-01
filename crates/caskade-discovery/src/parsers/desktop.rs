//! Bounded Desktop Entry specification reader; Exec is tokenized as argv,
//! never interpreted by a shell. Locale selection is explicit, without getenv.
use super::{Limits, ParseError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DesktopEntry {
    pub fields: BTreeMap<String, String>,
}
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<DesktopEntry> {
    let text = limits.text(bytes)?;
    for (n, line) in text.lines().enumerate() {
        limits.check(n + 1, limits.max_nodes, "desktop lines")?;
        limits.check(line.len(), limits.max_string_bytes, "line bytes")?;
    }
    let mut active = false;
    let mut found = false;
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(group) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            active = group == "Desktop Entry";
            if active {
                found = true;
                fields.clear();
            }
            continue;
        }
        if !active {
            continue;
        }
        let (key, raw) = line
            .split_once('=')
            .ok_or_else(|| ParseError::Invalid("desktop key/value".into()))?;
        let key = key.trim();
        if key.is_empty() || key.chars().any(|c| c.is_whitespace() || c == '=') {
            return Err(ParseError::Invalid("desktop key".into()));
        }
        let mut value = String::new();
        let mut chars = raw.trim_start().chars();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                value.push(ch);
                continue;
            }
            value.push(match chars.next() {
                Some('s') => ' ',
                Some('n') => '\n',
                Some('t') => '\t',
                Some('r') => '\r',
                Some('\\') => '\\',
                Some(';') => ';',
                _ => return Err(ParseError::Invalid("desktop string escape".into())),
            });
        }
        fields.insert(key.to_owned(), value);
    }
    if !found {
        return Err(ParseError::Invalid("missing Desktop Entry group".into()));
    }
    Ok(DesktopEntry { fields })
}
impl DesktopEntry {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }
    pub fn localized(&self, key: &str, locale: &str) -> Option<&str> {
        let (locale, modifier) = locale
            .split_once('@')
            .map_or((locale, None), |(l, m)| (l, Some(m)));
        let locale = locale.split('.').next().unwrap_or(locale);
        let language = locale.split('_').next().unwrap_or(locale);
        let mut candidates = Vec::new();
        if let Some(modifier) = modifier {
            candidates.push(format!("{locale}@{modifier}"));
        }
        candidates.push(locale.to_owned());
        if let Some(modifier) = modifier {
            candidates.push(format!("{language}@{modifier}"));
        }
        candidates.push(language.to_owned());
        candidates
            .into_iter()
            .find_map(|l| self.get(&format!("{key}[{l}]")))
            .or_else(|| self.get(key))
    }
    pub fn try_exec(&self) -> Option<&str> {
        self.get("TryExec")
    }
    pub fn hidden(&self) -> bool {
        self.get("Hidden") == Some("true")
    }
    /// Expand only launcher-independent field codes; file/URL placeholders have
    /// no selection and are removed. Replacement strings are not expanded again.
    pub fn argv(&self, locale: &str, desktop_file: Option<&str>) -> Result<Vec<String>> {
        let exec = self
            .get("Exec")
            .ok_or_else(|| ParseError::Invalid("missing Exec".into()))?;
        let tokens = tokenize(exec)?;
        let mut args = Vec::new();
        for token in tokens {
            if token == "%i" {
                if let Some(icon) = self.get("Icon").filter(|s| !s.is_empty()) {
                    args.push("--icon".into());
                    args.push(icon.to_owned());
                }
                continue;
            }
            if matches!(
                token.as_str(),
                "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m"
            ) {
                continue;
            }
            let mut out = String::new();
            let mut chars = token.chars();
            while let Some(ch) = chars.next() {
                if ch != '%' {
                    out.push(ch);
                    continue;
                }
                match chars.next() {
                    Some('%') => out.push('%'),
                    Some('c') => out.push_str(self.localized("Name", locale).unwrap_or("")),
                    Some('k') => out.push_str(desktop_file.unwrap_or("")),
                    Some('f' | 'F' | 'u' | 'U' | 'i') => {
                        return Err(ParseError::Invalid(
                            "field code must be a complete Exec argument".into(),
                        ));
                    }
                    Some('d' | 'D' | 'n' | 'N' | 'v' | 'm') => (),
                    _ => return Err(ParseError::Invalid("unknown Exec field code".into())),
                }
            }
            args.push(out);
        }
        if args.first().is_none_or(|a| a.is_empty() || a.contains('=')) {
            return Err(ParseError::Invalid("invalid Exec program".into()));
        }
        Ok(args)
    }
}
/// Undo Exec's quoting after the desktop string escape pass performed by the dependency.
fn tokenize(text: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut out = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' => {
                let next = chars
                    .next()
                    .ok_or_else(|| ParseError::Invalid("trailing Exec escape".into()))?;
                if quoted && !matches!(next, '"' | '`' | '$' | '\\') {
                    return Err(ParseError::Invalid("Exec quoted escape".into()));
                }
                out.push(next);
                started = true;
            }
            ' ' | '\t' | '\n' if !quoted => {
                if started {
                    args.push(std::mem::take(&mut out));
                    started = false;
                }
            }
            _ => {
                out.push(ch);
                started = true;
            }
        }
    }
    if quoted {
        return Err(ParseError::Invalid("unclosed Exec quote".into()));
    }
    if started {
        args.push(out);
    }
    Ok(args)
}
