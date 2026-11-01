use super::super::Region;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::OnceLock};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscPart {
    pub number: u32,
    pub label: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedName {
    pub title: String,
    pub properties: Vec<String>,
    pub regions: Vec<String>,
    pub part: Option<DiscPart>,
}
/// No-Intro/Redump filename conventions, independent of platform detection.
pub fn parse_name(path: &Path, regions: &[Region]) -> ParsedName {
    static GROUPS: OnceLock<Regex> = OnceLock::new();
    static PART: OnceLock<Regex> = OnceLock::new();
    let groups = GROUPS.get_or_init(|| Regex::new(r"\([^)]*\)|\[[^\]]*\]").unwrap());
    let part_re = PART.get_or_init(|| {
        Regex::new(r"(?i)^(disc|disk|side)\s*([0-9]+|[a-z])(?:\s*of\s*[0-9]+)?(?:\s|,|$)").unwrap()
    });
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let properties: Vec<_> = groups
        .find_iter(stem)
        .map(|m| m.as_str()[1..m.as_str().len() - 1].trim().to_string())
        .collect();
    let mut part = None;
    let mut found = Vec::new();
    for prop in &properties {
        if let Some(c) = part_re.captures(prop) {
            let value = &c[2];
            let number = value.parse::<u32>().ok().or_else(|| {
                value
                    .bytes()
                    .next()
                    .filter(|b| value.len() == 1 && b.is_ascii_alphabetic())
                    .map(|b| (b.to_ascii_uppercase() - b'A' + 1) as u32)
            });
            if let Some(number) = number.filter(|n| *n > 0) {
                let label = if c[1].eq_ignore_ascii_case("side") {
                    format!("Side {}", value.to_ascii_uppercase())
                } else {
                    format!("Disc {number}")
                };
                part = Some(DiscPart { number, label });
            }
        }
        for region in regions {
            if region.codes.iter().any(|code| {
                prop.eq_ignore_ascii_case(code)
                    || prop
                        .split([',', '+', '/'])
                        .any(|p| p.trim().eq_ignore_ascii_case(code))
            }) && !found.contains(&region.id)
            {
                found.push(region.id.clone());
            }
        }
    }
    let cleaned = groups
        .replace_all(stem, "")
        .replace('_', " ")
        .replace(['™', '®', '©'], "");
    let mut title = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let (main, subtitle) = title.split_once(" - ").unwrap_or((&title, ""));
    if main.to_lowercase().ends_with(", the") {
        title = format!(
            "The {}{}",
            main[..main.len() - 5].trim(),
            if subtitle.is_empty() {
                String::new()
            } else {
                format!(" - {subtitle}")
            }
        );
    }
    found.sort();
    ParsedName {
        title,
        properties,
        regions: found,
        part,
    }
}
