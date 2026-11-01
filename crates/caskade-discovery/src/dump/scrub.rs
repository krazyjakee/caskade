use super::{DumpError, Result};
use crate::{OperatingSystem, env::HostEnv};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub(super) const ACCOUNT: &str = "76561198000000000";
pub(super) struct Scrubber {
    replacements: Vec<(String, String)>,
    home: PathBuf,
    fixture_fs: Option<PathBuf>,
    canonical_home: String,
}
impl Scrubber {
    pub fn new(env: &HostEnv) -> Self {
        let canonical_home = match env.os() {
            OperatingSystem::Windows => r"C:\Users\fixture",
            OperatingSystem::Macos => "/Users/fixture",
            OperatingSystem::Linux => "/home/fixture",
        }
        .to_owned();
        let mut replacements = vec![(
            env.home().to_string_lossy().into_owned(),
            canonical_home.clone(),
        )];
        for name in ["HOME", "USERPROFILE"] {
            if let Some(value) = env.var(name).and_then(|s| s.into_string().ok()) {
                replacements.push((value, canonical_home.clone()));
            }
        }
        let forms = replacements.clone();
        for (old, new) in forms {
            replacements.push((old.replace('\\', "/"), new.replace('\\', "/")));
            replacements.push((old.replace('\\', "\\\\"), new.replace('\\', "\\\\")));
            replacements.push((
                format!("file://{}", old.replace(' ', "%20")),
                format!("file://{}", new.replace(' ', "%20")),
            ));
        }
        replacements.retain(|(old, _)| !old.is_empty() && old != "/");
        replacements.sort_by_key(|(old, _)| std::cmp::Reverse(old.len()));
        replacements.dedup();
        Self {
            replacements,
            home: env.home().to_owned(),
            fixture_fs: env.fixture_root().map(|_| env.fs_root().to_owned()),
            canonical_home,
        }
    }
    pub fn text(&self, text: &str) -> String {
        let mut text = text.to_owned();
        for (old, new) in &self.replacements {
            let lower = text.to_ascii_lowercase();
            let needle = old.to_ascii_lowercase();
            let mut result = String::new();
            let mut start = 0;
            for (offset, _) in lower.match_indices(&needle) {
                if offset < start {
                    continue;
                }
                let end = offset + old.len();
                if text[end..].chars().next().is_some_and(|c| {
                    !matches!(c, '/' | '\\' | '"' | '\'' | '\0') && !c.is_whitespace()
                }) {
                    continue;
                }
                result.push_str(&text[start..offset]);
                result.push_str(new);
                start = end;
            }
            result.push_str(&text[start..]);
            text = result;
        }
        let mut result = String::new();
        let mut start = 0;
        for (offset, _) in text.match_indices("7656119") {
            if offset < start {
                continue;
            }
            let n = text[offset..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            if n == 17 {
                result.push_str(&text[start..offset]);
                result.push_str(ACCOUNT);
                start = offset + n;
            }
        }
        result.push_str(&text[start..]);
        result
    }
    pub fn field(&self, key: &str, value: &str) -> String {
        let key = key.to_ascii_lowercase();
        if secret(&key) {
            return "<redacted>".into();
        }
        if account(&key) {
            return if key.contains("steam") || key == "lastowner" {
                ACCOUNT.into()
            } else {
                "fixture".into()
            };
        }
        self.text(value)
    }
    pub fn json(&self, value: &mut Value) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    if secret(key) || account(key) {
                        *value =
                            Value::String(self.field(key, value.as_str().unwrap_or("fixture")));
                    } else {
                        self.json(value);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    self.json(value);
                }
            }
            Value::String(text) => *text = self.text(text),
            _ => {}
        }
    }
    pub fn relative(&self, path: &Path) -> Result<PathBuf> {
        let virtual_path = if let Ok(relative) = path.strip_prefix(&self.home) {
            format!(
                "{}/{}",
                self.canonical_home.replace('\\', "/"),
                relative.to_string_lossy().replace('\\', "/")
            )
        } else if let Some(root) = &self.fixture_fs {
            path.strip_prefix(root)
                .map_err(|_| DumpError::Invalid("metadata lies outside fixture filesystem".into()))?
                .to_string_lossy()
                .replace('\\', "/")
        } else {
            path.to_string_lossy().replace('\\', "/")
        };
        let virtual_path = self.text(&virtual_path);
        let virtual_path = if virtual_path.as_bytes().get(1) == Some(&b':') {
            format!(
                "drives/{}/{}",
                virtual_path[..1].to_ascii_lowercase(),
                virtual_path[2..].trim_start_matches('/')
            )
        } else if virtual_path.starts_with("//") {
            format!("unc/{}", virtual_path.trim_start_matches('/'))
        } else {
            virtual_path.trim_start_matches('/').to_owned()
        };
        let mut output = PathBuf::new();
        let mut userdata = false;
        for part in virtual_path.split('/') {
            if part.is_empty() {
                continue;
            }
            if part == "." || part == ".." || part.contains(['\\', ':', '\0']) {
                return Err(DumpError::Invalid("unsafe virtual output path".into()));
            }
            let name = if userdata && part.bytes().all(|c| c.is_ascii_digit()) {
                "424242"
            } else {
                part
            };
            output.push(name);
            userdata = part == "userdata";
        }
        if output
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(DumpError::Invalid("unsafe output path".into()));
        }
        Ok(output)
    }
    pub fn virtual_path(&self, path: &Path) -> Result<String> {
        let relative = self.relative(path)?.to_string_lossy().replace('\\', "/");
        if let Some(tail) = relative.strip_prefix("drives/") {
            let (drive, tail) = tail.split_once('/').unwrap_or((tail, ""));
            Ok(format!(
                "{}:\\{}",
                drive.to_ascii_uppercase(),
                tail.replace('/', "\\")
            ))
        } else if let Some(tail) = relative.strip_prefix("unc/") {
            Ok(format!("\\\\{}", tail.replace('/', "\\")))
        } else {
            Ok(format!("/{relative}"))
        }
    }
    pub fn text_file(&self, text: &str) -> String {
        let mut result = String::new();
        for line in text.lines() {
            let trimmed = line.trim_start();
            let key = trimmed
                .trim_start_matches('"')
                .split(['"', '=', ':'])
                .next()
                .unwrap_or_default();
            if secret(key) || account(key) {
                // Retain the field's structure for VDF/INI/.reg text, replacing
                // only its quoted/string value; malformed sensitive lines are omitted.
                if let Some((prefix, _)) = line.split_once('=') {
                    result.push_str(prefix);
                    result.push_str("=\"");
                    result.push_str(&self.field(key, "fixture"));
                    result.push_str("\"\n");
                } else if let Some(end) = trimmed.strip_prefix('"').and_then(|s| s.find('"')) {
                    result.push('"');
                    result.push_str(&trimmed[1..end + 1]);
                    result.push_str("\" \"");
                    result.push_str(&self.field(key, "fixture"));
                    result.push_str("\"\n");
                }
            } else {
                result.push_str(&self.text(line));
                result.push('\n');
            }
        }
        result
    }
    pub fn validate(&self, bytes: &[u8]) -> Result<()> {
        let text = String::from_utf8_lossy(bytes)
            .to_ascii_lowercase()
            .replace("\\\\", "\\");
        for prefix in ["/home/", "/users/", "\\users\\"] {
            for (offset, _) in text.match_indices(prefix) {
                let user = text[offset + prefix.len()..]
                    .split(|c: char| {
                        matches!(c, '/' | '\\' | '"' | '\0' | '\'') || c.is_whitespace()
                    })
                    .next()
                    .unwrap_or_default();
                if !matches!(user, "fixture" | "public" | "shared") {
                    return Err(DumpError::Invalid(
                        "unrecognized private home path; refusing to publish dump".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}
pub(super) fn secret(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "password",
        "token",
        "secret",
        "cookie",
        "auth",
        "connectcache",
        "email",
    ]
    .iter()
    .any(|word| key.contains(word))
}
pub(super) fn account(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "accountid"
            | "account_id"
            | "steamid"
            | "steam_id"
            | "userid"
            | "user_id"
            | "username"
            | "accountname"
            | "personaname"
            | "autologinuser"
            | "lastowner"
    )
}
