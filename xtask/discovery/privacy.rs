//! Conservative release check over generated and reviewable fixture data.
use super::Result;
use std::path::Path;

const STEAM_ACCOUNT: &str = "76561198000000000";

pub fn check_bytes(path: &Path, data: &[u8]) -> Result<()> {
    check_text(path, &String::from_utf8_lossy(data))?;
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) {
        check_json(path, &value)?;
    }
    // Binary VDF, shell links and registry strings can carry UTF-16. Search
    // both byte alignments; SQLite text may begin at any offset in its page.
    for offset in 0..=1 {
        let units: Vec<u16> = data
            .get(offset..)
            .unwrap_or_default()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|bytes| u16::from_le_bytes(*bytes))
            .collect();
        check_text(path, &String::from_utf16_lossy(&units))?;
    }
    Ok(())
}

fn check_text(path: &Path, text: &str) -> Result<()> {
    // JSON/.reg escaping may double backslashes several times.
    let mut plain = text.to_ascii_lowercase();
    for _ in 0..3 {
        plain = plain.replace("\\\\", "\\");
    }
    for prefix in ["/home/", "/users/", "\\users\\"] {
        for (start, _) in plain.match_indices(prefix) {
            let tail = &plain[start + prefix.len()..];
            let end = tail
                .find(|c: char| matches!(c, '/' | '\\' | '"' | '\'' | '\0') || c.is_whitespace())
                .unwrap_or(tail.len());
            let name = &tail[..end];
            if !matches!(name, "fixture" | "public" | "shared") {
                return Err(format!(
                    "unscrubbed home path in {} (only the synthetic fixture user is allowed)",
                    path.display()
                )
                .into());
            }
        }
    }
    for (start, _) in plain.match_indices("7656119") {
        let number: String = plain[start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if number.len() == 17 && number != STEAM_ACCOUNT {
            return Err(format!("unscrubbed Steam account id in {}", path.display()).into());
        }
    }
    for (start, _) in plain.match_indices("userdata/") {
        let account: String = plain[start + "userdata/".len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if !account.is_empty() && account != "424242" {
            return Err(format!("unscrubbed userdata account in {}", path.display()).into());
        }
    }
    Ok(())
}

fn check_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                let key = key.to_ascii_lowercase();
                if matches!(
                    key.as_str(),
                    "accountid"
                        | "account_id"
                        | "userid"
                        | "user_id"
                        | "username"
                        | "accountname"
                        | "personaname"
                        | "steamid"
                        | "steam_id"
                ) && !value.is_null()
                    && !matches!(
                        value.as_str(),
                        Some("" | "fixture" | "<redacted>" | "76561198000000000" | "424242")
                    )
                    && value.as_u64() != Some(424242)
                {
                    return Err(format!("unscrubbed account field in {}", path.display()).into());
                }
                check_json(path, value)?;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                check_json(path, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
