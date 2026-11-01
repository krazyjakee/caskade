//! Reading a catalogue document: bounded, record by record. Only a document
//! that isn't a catalogue at all (too large, not JSON, another format) is
//! refused whole; a bad record or release is left out with the reason, so
//! it can't hide the good ones.

use super::{
    AddonKind, AddonOs, AppMode, CATALOGUE_FORMAT, Catalogue, CatalogueArtifact, CatalogueEntry,
    CatalogueMedia, CatalogueProblem, CatalogueRelease,
};
use crate::{Error, Result};
use serde_json::{Map, Value};
use std::collections::HashSet;
use url::Url;

/// The largest catalogue read.
pub const MAX_CATALOGUE_BYTES: usize = 4 << 20;
/// The largest signature read.
pub const MAX_SIGNATURE_BYTES: usize = 4 << 10;
const MAX_PACKAGES: usize = 1000;
const MAX_RELEASES: usize = 50;
const MAX_SCREENSHOTS: usize = 8;
const MAX_CATEGORIES: usize = 8;
const MAX_PERMISSIONS: usize = 32;
/// As the plugin host's folder installs (`INSTALL_BYTES_MAX`).
const MAX_ARTIFACT_BYTES: u64 = 512 << 20;
/// The provider interface the catalogue never offers (custom metadata
/// sources are out of scope).
const RESERVED_PROVIDER: &str = "metadata-provider";

type Fail = String;

fn text(
    record: &Map<String, Value>,
    key: &str,
    max: usize,
    required: bool,
) -> std::result::Result<String, Fail> {
    match record.get(key) {
        None | Some(Value::Null) if !required => Ok(String::new()),
        Some(Value::String(value)) => {
            let value = value.trim();
            if required && value.is_empty() {
                return Err(format!("“{key}” is empty"));
            }
            if value.chars().count() > max {
                return Err(format!("“{key}” is longer than {max} characters"));
            }
            if value
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
            {
                return Err(format!("“{key}” has control characters"));
            }
            Ok(value.to_owned())
        }
        _ => Err(format!("“{key}” is missing or not text")),
    }
}

fn list<'a>(
    record: &'a Map<String, Value>,
    key: &str,
    max: usize,
) -> std::result::Result<&'a [Value], Fail> {
    match record.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(items)) if items.len() <= max => Ok(items),
        Some(Value::Array(_)) => Err(format!("“{key}” has more than {max} items")),
        _ => Err(format!("“{key}” isn't a list")),
    }
}

fn words(
    record: &Map<String, Value>,
    key: &str,
    max: usize,
    len: usize,
) -> std::result::Result<Vec<String>, Fail> {
    list(record, key, max)?
        .iter()
        .map(|item| match item {
            Value::String(word) if !word.trim().is_empty() && word.len() <= len => {
                Ok(word.trim().to_owned())
            }
            _ => Err(format!("“{key}” has an item that isn't short text")),
        })
        .collect()
}

/// An add-on id as manifests have them: `dev.example.stats`.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        })
}

/// `url` resolved against the catalogue's address: HTTPS, or the
/// catalogue's own scheme and host (a local server while developing).
fn resolve(base: &Url, url: &str) -> std::result::Result<String, Fail> {
    let resolved = base
        .join(url)
        .map_err(|_| format!("“{url}” isn't an address"))?;
    let same_origin = resolved.scheme() == base.scheme()
        && resolved.host_str() == base.host_str()
        && resolved.port_or_known_default() == base.port_or_known_default();
    if resolved.scheme() != "https" && !same_origin {
        return Err(format!("“{url}” isn't an HTTPS address"));
    }
    if !resolved.username().is_empty() || resolved.password().is_some() {
        return Err(format!("“{url}” carries credentials"));
    }
    Ok(resolved.into())
}

fn object<'a>(value: &'a Value, what: &str) -> std::result::Result<&'a Map<String, Value>, Fail> {
    value
        .as_object()
        .ok_or_else(|| format!("{what} isn't an object"))
}

fn release(
    kind: AddonKind,
    value: &Value,
    base: &Url,
) -> std::result::Result<(semver::Version, CatalogueRelease), Fail> {
    let record = object(value, "a release")?;
    let version_text = text(record, "version", 64, true)?;
    let version = semver::Version::parse(&version_text)
        .map_err(|_| format!("version “{version_text}” isn't a semantic version"))?;
    let api = match (kind, record.get("api")) {
        (AddonKind::Plugin, Some(Value::String(api))) => {
            semver::VersionReq::parse(api)
                .map_err(|_| format!("plugin API range “{api}” can't be read"))?;
            Some(api.trim().to_owned())
        }
        (AddonKind::Plugin, _) => return Err("a plugin release has no plugin API range".into()),
        (AddonKind::Theme, None | Some(Value::Null)) => None,
        (AddonKind::Theme, Some(_)) => return Err("a theme release has a plugin API range".into()),
    };
    let os = list(record, "os", 3)?
        .iter()
        .map(|item| {
            serde_json::from_value::<AddonOs>(item.clone())
                .map_err(|_| format!("unknown OS {item}"))
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let modes = list(record, "modes", 2)?
        .iter()
        .map(|item| {
            serde_json::from_value::<AppMode>(item.clone())
                .map_err(|_| format!("unknown mode {item}"))
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let provides = words(record, "provides", 16, 64)?;
    if provides
        .iter()
        .any(|provider| provider == RESERVED_PROVIDER)
    {
        return Err("it provides metadata, which the catalogue doesn't offer".into());
    }
    let published = match text(record, "published", 10, false)? {
        date if date.is_empty() => None,
        date => {
            crate::model::validation::validate_date(&date)
                .map_err(|_| format!("published date “{date}” isn't YYYY-MM-DD"))?;
            Some(date)
        }
    };
    let artifact = object(record.get("artifact").unwrap_or(&Value::Null), "“artifact”")?;
    let sha256 = text(artifact, "sha256", 64, true)?.to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("the artifact's SHA-256 isn't 64 hex digits".into());
    }
    let size = artifact
        .get("size")
        .and_then(Value::as_u64)
        .filter(|size| (1..=MAX_ARTIFACT_BYTES).contains(size))
        .ok_or("the artifact's size is missing or larger than 512 MiB")?;
    let url = resolve(base, &text(artifact, "url", 2048, true)?)?;
    Ok((
        version,
        CatalogueRelease {
            version: version_text,
            api,
            os,
            modes,
            permissions: words(record, "permissions", MAX_PERMISSIONS, 64)?,
            notes: text(record, "notes", 8000, false)?,
            published,
            artifact: CatalogueArtifact { url, sha256, size },
        },
    ))
}

fn entry(
    value: &Value,
    base: &Url,
    problems: &mut Vec<CatalogueProblem>,
) -> std::result::Result<CatalogueEntry, (Option<String>, Fail)> {
    let record = object(value, "a package").map_err(|fail| (None, fail))?;
    let id = record
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| valid_id(id))
        .map(str::to_owned)
        .ok_or((None, "a package has no valid id".to_owned()))?;
    let fail = |message: Fail| (Some(id.clone()), message);
    let kind = match record.get("kind").and_then(Value::as_str) {
        Some("plugin") => AddonKind::Plugin,
        Some("theme") => AddonKind::Theme,
        _ => return Err(fail("its kind isn't plugin or theme".into())),
    };
    let screenshots = list(record, "screenshots", MAX_SCREENSHOTS)
        .map_err(fail)?
        .iter()
        .map(|item| {
            let media = object(item, "a screenshot")?;
            Ok(CatalogueMedia {
                url: resolve(base, &text(media, "url", 2048, true)?)?,
                alt: text(media, "alt", 200, false)?,
            })
        })
        .collect::<std::result::Result<Vec<_>, Fail>>()
        .map_err(fail)?;
    let categories = words(record, "categories", MAX_CATEGORIES, 32).map_err(fail)?;
    if categories
        .iter()
        .any(|category| category.eq_ignore_ascii_case("metadata"))
    {
        return Err(fail(
            "metadata sources aren't offered in the catalogue".into(),
        ));
    }
    let homepage = match text(record, "homepage", 2048, false).map_err(fail)? {
        page if page.is_empty() => None,
        page => Some(resolve(base, &page).map_err(fail)?),
    };
    let mut releases: Vec<(semver::Version, CatalogueRelease)> = Vec::new();
    for value in list(record, "releases", MAX_RELEASES).map_err(fail)? {
        match release(kind, value, base) {
            Ok((version, release)) if releases.iter().any(|(known, _)| *known == version) => {
                problems.push(CatalogueProblem {
                    id: Some(id.clone()),
                    message: format!("release {} is listed twice", release.version),
                });
            }
            Ok(release) => releases.push(release),
            Err(message) => problems.push(CatalogueProblem {
                id: Some(id.clone()),
                message: format!("a release was left out: {message}"),
            }),
        }
    }
    if releases.is_empty() {
        return Err(fail("it has no usable release".into()));
    }
    releases.sort_by(|a, b| b.0.cmp(&a.0));
    let deprecated = match record.get("deprecated") {
        Some(Value::String(why)) if !why.trim().is_empty() => {
            Some(why.trim().chars().take(500).collect())
        }
        _ => None,
    };
    Ok(CatalogueEntry {
        kind,
        name: text(record, "name", 80, true).map_err(fail)?,
        author: text(record, "author", 80, true).map_err(fail)?,
        summary: text(record, "summary", 200, true).map_err(fail)?,
        description: text(record, "description", 8000, false).map_err(fail)?,
        license: text(record, "license", 64, true).map_err(fail)?,
        homepage,
        categories,
        screenshots,
        releases: releases.into_iter().map(|(_, release)| release).collect(),
        deprecated,
        id,
    })
}

/// Reads a catalogue document fetched from `base` (relative addresses in
/// it resolve against `base`).
pub fn parse(bytes: &[u8], base: &Url) -> Result<Catalogue> {
    if bytes.len() > MAX_CATALOGUE_BYTES {
        return Err(Error::Invalid(crate::tr!(
            "the catalogue is larger than {mib} MiB",
            mib = MAX_CATALOGUE_BYTES >> 20
        )));
    }
    let document: Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::Invalid(crate::tr!("the catalogue isn't readable JSON")))?;
    let Some(document) = document.as_object() else {
        return Err(Error::Invalid(crate::tr!(
            "the catalogue isn't readable JSON"
        )));
    };
    if document.get("format").and_then(Value::as_u64) != Some(u64::from(CATALOGUE_FORMAT)) {
        return Err(Error::Invalid(crate::tr!(
            "the catalogue is in a format this Caskade can't read; update Caskade"
        )));
    }
    let Some(Value::Array(packages)) = document.get("packages") else {
        return Err(Error::Invalid(crate::tr!(
            "the catalogue lists no packages"
        )));
    };
    let mut catalogue = Catalogue {
        generated: document
            .get("generated")
            .and_then(Value::as_str)
            .map(|text| text.chars().take(64).collect()),
        ..Default::default()
    };
    if packages.len() > MAX_PACKAGES {
        catalogue.problems.push(CatalogueProblem {
            id: None,
            message: format!("only the first {MAX_PACKAGES} packages are read"),
        });
    }
    let mut seen = HashSet::new();
    for value in packages.iter().take(MAX_PACKAGES) {
        let mut problems = Vec::new();
        match entry(value, base, &mut problems) {
            Ok(entry) if !seen.insert(entry.id.clone()) => problems.push(CatalogueProblem {
                id: Some(entry.id),
                message: "the id is listed twice; the first one stays".into(),
            }),
            Ok(entry) => catalogue.entries.push(entry),
            Err((id, message)) => problems.push(CatalogueProblem {
                id,
                message: format!("left out: {message}"),
            }),
        }
        catalogue.problems.extend(problems);
    }
    Ok(catalogue)
}
