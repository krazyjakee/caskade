//! `theme.json`: who the theme is and what it builds on.
use crate::{Problem, is_image};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ThemeManifest {
    /// Reverse-DNS, like plugin ids: `caskade.default`, `dev.example.neon`.
    pub id: String,
    pub name: String,
    /// `major.minor.patch`.
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    /// The theme this one restyles on top of, usually `caskade.default`.
    #[serde(default)]
    pub extends: Option<String>,
    /// Colour schemes it provides, the first being its default:
    /// `dark`, `light`.
    #[serde(default = "dark")]
    pub modes: Vec<String>,
    /// Screenshots for the theme picker, relative to the theme folder.
    #[serde(default)]
    pub previews: Vec<String>,
}

fn dark() -> Vec<String> {
    vec!["dark".to_owned()]
}

pub(crate) fn valid_id(id: &str) -> bool {
    let mut parts = id.split('.');
    let first = parts.next().unwrap_or_default();
    let rest: Vec<&str> = parts.collect();
    let segment = |part: &str| {
        !part.is_empty()
            && part.len() <= 64
            && part.starts_with(|c: char| c.is_ascii_lowercase())
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    id.len() <= 128 && segment(first) && !rest.is_empty() && rest.iter().all(|part| segment(part))
}

fn valid_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 9 && part.chars().all(|c| c.is_ascii_digit())
        })
}

pub(crate) fn parse(bytes: &[u8], files: &[String]) -> Result<ThemeManifest, Vec<Problem>> {
    const FILE: &str = "theme.json";
    let manifest: ThemeManifest =
        serde_json::from_slice(bytes).map_err(|error| vec![Problem::json(FILE, &error)])?;
    let mut problems = Vec::new();
    let mut fail = |message: String| problems.push(Problem::new(FILE, message));
    if !valid_id(&manifest.id) {
        fail(format!(
            "id \"{}\" must be reverse-DNS: lowercase dot-separated names, like dev.example.neon",
            manifest.id
        ));
    }
    if manifest.name.trim().is_empty() || manifest.name.chars().count() > 64 {
        fail("name must be 1 to 64 characters".to_owned());
    }
    if !valid_version(&manifest.version) {
        fail(format!(
            "version \"{}\" must be major.minor.patch",
            manifest.version
        ));
    }
    if let Some(extends) = &manifest.extends {
        if !valid_id(extends) {
            fail(format!("extends \"{extends}\" is not a theme id"));
        } else if *extends == manifest.id {
            fail("a theme can't extend itself".to_owned());
        }
    }
    if manifest.modes.is_empty() {
        fail("modes must list at least one colour scheme".to_owned());
    }
    for mode in &manifest.modes {
        if mode != "dark" && mode != "light" {
            fail(format!("mode \"{mode}\" is not dark or light"));
        }
    }
    for preview in &manifest.previews {
        if !is_image(preview) {
            fail(format!("preview \"{preview}\" must be an image"));
        } else if !files.contains(preview) {
            fail(format!("preview \"{preview}\" doesn't exist in the theme"));
        }
    }
    if problems.is_empty() {
        Ok(manifest)
    } else {
        Err(problems)
    }
}
