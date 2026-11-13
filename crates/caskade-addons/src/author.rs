//! Building catalogue packages and the catalogue itself, for publishing
//! (#184). `pack` turns a plugin or theme folder into the archive an
//! install unpacks, the same bytes every time for the same files; `build`
//! packs every package a source file lists and writes `catalogue.json`
//! with each archive's SHA-256 and size beside them. Neither signs
//! anything: signing and uploading are the maintainer's steps
//! (`docs/plugins/publishing.md`). Messages are for the maintainer's
//! terminal and aren't translated.

use super::{AddonKind, CATALOGUE_FORMAT};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

/// What a package may hold at its top level, by kind. Sources, tests and
/// build output other than `dist/` stay out.
const PLUGIN_FILES: &[&str] = &[
    "plugin.json",
    "dist",
    "locales",
    "assets",
    "README.md",
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
    "CHANGELOG.md",
];
const THEME_FILES: &[&str] = &[
    "theme.json",
    "tokens.json",
    "theme.css",
    "assets",
    "previews",
    "locales",
    "README.md",
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
];

/// File names that hold credentials, refused anywhere in a package.
const SECRET_NAMES: &[&str] = &[
    ".env",
    ".npmrc",
    ".netrc",
    ".pypirc",
    "id_rsa",
    "id_ed25519",
    "credentials",
    "credentials.json",
];
const SECRET_EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx", "keystore", "jks"];
/// Text that only credentials contain: private keys, minisign and Tauri
/// signing keys, and the token prefixes of the usual hosts.
const SECRET_MARKERS: &[&str] = &[
    "PRIVATE KEY-----",
    "minisign encrypted secret key",
    "rsign encrypted secret key",
    "ghp_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "AKIA",
    "sk_live_",
];

/// The largest source package `pack` reads, as installs allow.
const MAX_PACKAGE_BYTES: u64 = 512 << 20;

/// A packed add-on: its archive and what its manifest says.
#[derive(Debug, Clone)]
pub struct Packed {
    pub kind: AddonKind,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    /// A plugin's API range.
    pub api: Option<String>,
    /// What a plugin asks for, as the catalogue shows it.
    pub permissions: Vec<String>,
    /// The provider contracts a plugin's manifest contributes.
    pub provides: Vec<String>,
    pub archive: Vec<u8>,
}

impl Packed {
    pub fn sha256(&self) -> String {
        format!("{:x}", Sha256::digest(&self.archive))
    }

    /// `<id>-<version>.zip`.
    pub fn file_name(&self) -> String {
        format!("{}-{}.zip", self.id, self.version)
    }
}

/// A plugin manifest's permissions as catalogue words: `library.read`,
/// `library.write`, `network:<host>`, `fs:<folder>` and `process`.
pub fn permission_words(manifest: &Value) -> Vec<String> {
    let Some(permissions) = manifest.get("permissions") else {
        return Vec::new();
    };
    let mut words = Vec::new();
    match permissions.get("library").and_then(Value::as_str) {
        Some("read") => words.push("library.read".to_owned()),
        Some("write") => words.push("library.write".to_owned()),
        _ => {}
    }
    let list = |key: &str| {
        permissions
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    words.extend(
        list("network")
            .into_iter()
            .map(|host| format!("network:{host}")),
    );
    words.extend(list("fs").into_iter().map(|folder| format!("fs:{folder}")));
    if permissions.get("process").and_then(Value::as_bool) == Some(true) {
        words.push("process".to_owned());
    }
    words
}

fn manifest_name(kind: AddonKind) -> &'static str {
    match kind {
        AddonKind::Plugin => "plugin.json",
        AddonKind::Theme => "theme.json",
    }
}

/// Packs `folder` as an add-on of `kind`: the files a package may hold,
/// under one top folder named for its id, in name order with fixed times
/// and modes, so the same files give the same archive. Refuses links,
/// files that look like credentials and a manifest without an id or
/// version.
pub fn pack(folder: &Path, kind: AddonKind) -> Result<Packed, String> {
    let manifest_path = folder.join(manifest_name(kind));
    let manifest: Value = std::fs::read(&manifest_path)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))
        .and_then(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|error| format!("{}: {error}", manifest_path.display()))
        })?;
    let text = |key: &str| {
        manifest
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default()
    };
    let (id, version) = (text("id"), text("version"));
    if id.is_empty() || version.is_empty() {
        return Err(format!(
            "{} needs an id and a version",
            manifest_path.display()
        ));
    }
    let allowed = match kind {
        AddonKind::Plugin => PLUGIN_FILES,
        AddonKind::Theme => THEME_FILES,
    };
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for name in allowed {
        collect(folder, Path::new(name), &mut files, &mut total)?;
    }
    if kind == AddonKind::Plugin && super::package::contributes_metadata(&manifest) {
        return Err(format!(
            "{}: it contributes {}, and the catalogue doesn't offer metadata sources",
            manifest_path.display(),
            super::validate::RESERVED_PROVIDER
        ));
    }
    if kind == AddonKind::Plugin && !text("ui").is_empty() && draws_ui(&text("api")) {
        return Err(format!(
            "{}: plugin API 0.2 plugins draw their UI from their backend; remove “ui” (#109)",
            manifest_path.display()
        ));
    }
    if kind == AddonKind::Plugin {
        // What the manifest points at must be in the package.
        for key in ["ui", "backend"] {
            let path = text(key);
            if !path.is_empty() && !files.contains_key(&path) {
                return Err(format!(
                    "{}: “{key}” names {path}, which isn't built; build the plugin first",
                    folder.display()
                ));
            }
        }
    }
    let archive = zip(&id, &files)?;
    Ok(Packed {
        kind,
        name: text("name"),
        description: text("description"),
        author: text("author"),
        api: (kind == AddonKind::Plugin)
            .then(|| text("api"))
            .filter(|api| !api.is_empty()),
        permissions: if kind == AddonKind::Plugin {
            permission_words(&manifest)
        } else {
            Vec::new()
        },
        provides: manifest
            .pointer("/contributes/providers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|provider| provider.as_str().map(str::to_owned))
            .collect(),
        id,
        version,
        archive,
    })
}

/// Adds `relative` under `root` to `files` (by `/`-separated path), files
/// and folders alike; a missing name is skipped.
fn collect(
    root: &Path,
    relative: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
    total: &mut u64,
) -> Result<(), String> {
    let path = root.join(relative);
    let Ok(meta) = std::fs::symlink_metadata(&path) else {
        return Ok(());
    };
    let shown = relative.display();
    if meta.file_type().is_symlink() {
        return Err(format!("{shown} is a link; packages hold files only"));
    }
    let name = relative
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name.starts_with('.') && name != "." {
        // Hidden files (.DS_Store, .git, editor state) never ship, but a
        // credential file is an error rather than silently left out.
        if SECRET_NAMES.iter().any(|secret| name.starts_with(secret)) {
            return Err(format!("{shown} looks like credentials; remove it"));
        }
        return Ok(());
    }
    if meta.is_dir() {
        let mut children: Vec<_> = std::fs::read_dir(&path)
            .map_err(|error| format!("{shown}: {error}"))?
            .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
            .collect();
        children.sort();
        for child in children {
            collect(root, &relative.join(child), files, total)?;
        }
        return Ok(());
    }
    let lower = name.to_ascii_lowercase();
    let extension = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    if SECRET_NAMES.contains(&lower.as_str()) || SECRET_EXTENSIONS.contains(&extension) {
        return Err(format!("{shown} looks like credentials; remove it"));
    }
    *total += meta.len();
    if *total > MAX_PACKAGE_BYTES {
        return Err("the package is larger than 512 MiB".to_owned());
    }
    let bytes = std::fs::read(&path).map_err(|error| format!("{shown}: {error}"))?;
    if let Some(marker) = SECRET_MARKERS
        .iter()
        .find(|marker| contains(&bytes, marker.as_bytes()))
    {
        return Err(format!(
            "{shown} contains “{marker}”, which looks like a credential; remove it"
        ));
    }
    let key = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    files.insert(key, bytes);
    Ok(())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn zip(top: &str, files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(9))
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(0o644);
        for (path, bytes) in files {
            writer
                .start_file(format!("{top}/{path}"), options)
                .map_err(|error| error.to_string())?;
            writer.write_all(bytes).map_err(|error| error.to_string())?;
        }
        writer.finish().map_err(|error| error.to_string())?;
    }
    Ok(cursor.into_inner())
}

/// The source of a catalogue: what each package's manifest doesn't say.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub format: u32,
    /// The catalogue's `generated` time; otherwise `SOURCE_DATE_EPOCH`,
    /// otherwise now.
    #[serde(default)]
    pub generated: Option<String>,
    pub packages: Vec<SourcePackage>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePackage {
    /// The package folder, relative to the source file.
    pub path: PathBuf,
    pub kind: AddonKind,
    pub summary: String,
    /// Defaults to the manifest's description.
    #[serde(default)]
    pub description: Option<String>,
    pub license: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub screenshots: Vec<SourceMedia>,
    /// Empty: every OS.
    #[serde(default)]
    pub os: Vec<String>,
    /// Empty: both modes.
    #[serde(default)]
    pub modes: Vec<String>,
    /// This release's notes.
    #[serde(default)]
    pub notes: String,
    /// This release's date, `YYYY-MM-DD`.
    #[serde(default)]
    pub published: Option<String>,
    /// A reason, to deprecate the package.
    #[serde(default)]
    pub deprecated: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMedia {
    /// An image file, relative to the source file: PNG, JPEG or WebP.
    pub file: PathBuf,
    pub alt: String,
}

/// What `build` wrote.
#[derive(Debug, Default)]
pub struct Built {
    /// `(id, version, archive file, sha256)` of each package packed now.
    pub packages: Vec<(String, String, String, String)>,
    /// Releases carried over from the previous catalogue.
    pub carried: usize,
}

/// Packs every package `source` lists into `<out>/<id>-<version>.zip`,
/// copies their screenshots to `<out>/<id>-<n>.<ext>` and writes
/// `<out>/catalogue.json`: one flat folder, as a GitHub release holds its
/// assets, so the folder uploads as it is.
/// With `previous` (the published `catalogue.json`), each package keeps
/// its earlier releases, newest first, so installed copies can still be
/// matched; a version already published with other bytes is refused.
pub fn build(source: &Path, out: &Path, previous: Option<&Path>) -> Result<Built, String> {
    let at = |error: &dyn std::fmt::Display| format!("{}: {error}", source.display());
    let spec: Source =
        serde_json::from_slice(&std::fs::read(source).map_err(|e| at(&e))?).map_err(|e| at(&e))?;
    if spec.format != CATALOGUE_FORMAT {
        return Err(at(&format!(
            "format {} isn't {CATALOGUE_FORMAT}",
            spec.format
        )));
    }
    let base = source.parent().unwrap_or(Path::new("."));
    let previous: Map<String, Value> = match previous {
        Some(path) => {
            let doc: Value = serde_json::from_slice(
                &std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?,
            )
            .map_err(|e| format!("{}: {e}", path.display()))?;
            doc.get("packages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|package| {
                    Some((package.get("id")?.as_str()?.to_owned(), package.clone()))
                })
                .collect()
        }
        None => Map::new(),
    };
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut built = Built::default();
    let mut packages = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for package in &spec.packages {
        let folder = base.join(&package.path);
        let packed = pack(&folder, package.kind)?;
        if !ids.insert(packed.id.clone()) {
            return Err(format!("{} is listed twice", packed.id));
        }
        let file = packed.file_name();
        std::fs::write(out.join(&file), &packed.archive).map_err(|e| e.to_string())?;
        let mut screenshots = Vec::new();
        for (index, shot) in package.screenshots.iter().enumerate() {
            let from = base.join(&shot.file);
            let extension = from
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp") {
                return Err(format!(
                    "{}: screenshots are PNG, JPEG or WebP",
                    from.display()
                ));
            }
            let name = format!("{}-{}.{extension}", packed.id, index + 1);
            std::fs::copy(&from, out.join(&name))
                .map_err(|e| format!("{}: {e}", from.display()))?;
            screenshots.push(json!({"url": name, "alt": shot.alt}));
        }
        let mut release = json!({
            "version": packed.version,
            "os": package.os,
            "modes": package.modes,
            "permissions": packed.permissions,
            "provides": packed.provides,
            "notes": package.notes,
            "artifact": {
                "url": file,
                "sha256": packed.sha256(),
                "size": packed.archive.len(),
            },
        });
        if let Some(api) = &packed.api {
            release["api"] = json!(api);
        }
        if let Some(published) = &package.published {
            release["published"] = json!(published);
        }
        let mut releases = vec![release];
        if let Some(old) = previous.get(&packed.id) {
            for old in old
                .get("releases")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if old.get("version").and_then(Value::as_str) == Some(&packed.version) {
                    let same = old.pointer("/artifact/sha256").and_then(Value::as_str)
                        == Some(&packed.sha256());
                    if !same {
                        return Err(format!(
                            "{} {} is already published with other contents; raise its version",
                            packed.id, packed.version
                        ));
                    }
                    continue;
                }
                releases.push(old.clone());
                built.carried += 1;
            }
        }
        releases.sort_by_key(|release| {
            std::cmp::Reverse(
                release
                    .get("version")
                    .and_then(Value::as_str)
                    .and_then(|v| semver::Version::parse(v).ok()),
            )
        });
        releases.truncate(50);
        built.packages.push((
            packed.id.clone(),
            packed.version.clone(),
            file,
            packed.sha256(),
        ));
        let mut entry = json!({
            "id": packed.id,
            "kind": package.kind,
            "name": packed.name,
            "author": packed.author,
            "summary": package.summary,
            "description": package.description.clone().unwrap_or(packed.description.clone()),
            "license": package.license,
            "categories": package.categories,
            "screenshots": screenshots,
            "deprecated": package.deprecated,
            "releases": releases,
        });
        if let Some(homepage) = &package.homepage {
            entry["homepage"] = json!(homepage);
        }
        packages.push(entry);
    }
    let generated = spec.generated.clone().unwrap_or_else(|| {
        let seconds = std::env::var("SOURCE_DATE_EPOCH")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_secs() as i64)
            });
        utc(seconds)
    });
    let catalogue = json!({
        "format": CATALOGUE_FORMAT,
        "generated": generated,
        "packages": packages,
    });
    let mut text = serde_json::to_string_pretty(&catalogue).map_err(|e| e.to_string())?;
    text.push('\n');
    // The app's own reading: a record it would leave out fails the build.
    let base = url::Url::parse("https://catalogue.invalid/catalogue.json").expect("a URL");
    let read = super::parse(text.as_bytes(), &base).map_err(|e| e.to_string())?;
    if !read.problems.is_empty() {
        return Err(read
            .problems
            .iter()
            .map(|problem| match &problem.id {
                Some(id) => format!("{id}: {}", problem.message),
                None => problem.message.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"));
    }
    std::fs::write(out.join("catalogue.json"), text).map_err(|e| e.to_string())?;
    Ok(built)
}

/// `seconds` since the epoch as `YYYY-MM-DDTHH:MM:SSZ`.
pub(crate) fn utc(seconds: i64) -> String {
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, day) = caskade_core::civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// A release whose archive checked out and unpacked, for the adapter's
/// own plugin and theme checks.
#[derive(Debug, Clone)]
pub struct CheckedRelease {
    pub entry: super::CatalogueEntry,
    pub release: super::CatalogueRelease,
    /// The unpacked package.
    pub root: PathBuf,
}

/// What `check` found.
#[derive(Debug, Default)]
pub struct Checked {
    pub catalogue: super::Catalogue,
    pub releases: Vec<CheckedRelease>,
    /// What would stop the catalogue or a package from being offered.
    pub problems: Vec<String>,
    /// Allowed, but worth a look.
    pub warnings: Vec<String>,
}

/// The largest screenshot `check` reads.
const MAX_MEDIA_BYTES: u64 = 8 << 20;

/// Checks a staged or published catalogue as the app will read it: from
/// `location` (a path, or an `http(s)` address), its signature against
/// `public_key` when given, every record valid (one the app would leave
/// out is a problem here), and every release's archive reachable, of its
/// size and SHA-256, safe to unpack and holding the manifest it claims.
/// Screenshots must be reachable images. Archives unpack under `work`.
pub async fn check(
    location: &str,
    public_key: Option<&str>,
    work: &Path,
) -> Result<Checked, String> {
    let origin = if location.starts_with("http://") || location.starts_with("https://") {
        url::Url::parse(location).map_err(|e| format!("{location}: {e}"))?
    } else {
        let path = std::fs::canonicalize(location).map_err(|e| format!("{location}: {e}"))?;
        url::Url::from_file_path(&path).map_err(|()| format!("{location}: not a file path"))?
    };
    let service = super::CatalogueService::new(work.join("cache"), origin.as_str(), public_key)
        .map_err(|e| e.to_string())?;
    let mut checked = Checked::default();
    if origin.scheme() == "file" {
        let path = origin.to_file_path().expect("a file URL");
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let signature = match public_key {
            Some(_) => {
                let sig = path.with_extension("json.sig");
                Some(std::fs::read(&sig).map_err(|e| format!("{}: {e}", sig.display()))?)
            }
            None => None,
        };
        service
            .accept(&bytes, signature.as_deref(), 0)
            .await
            .map_err(|e| e.to_string())?;
    } else {
        #[cfg(feature = "net")]
        {
            let status = service.refresh().await;
            if let Some(error) = status.error {
                return Err(error);
            }
        }
        #[cfg(not(feature = "net"))]
        return Err("checking a published catalogue needs the net feature".to_owned());
    }
    if public_key.is_none() {
        checked
            .warnings
            .push("no public key given: the signature wasn't checked".to_owned());
    }
    checked.catalogue = service.catalogue();
    for problem in &checked.catalogue.problems {
        checked.problems.push(match &problem.id {
            Some(id) => format!("{id}: {}", problem.message),
            None => problem.message.clone(),
        });
    }
    let entries = checked.catalogue.entries.clone();
    for entry in &entries {
        if entry.screenshots.is_empty() {
            checked
                .warnings
                .push(format!("{}: no screenshots", entry.id));
        }
        for (index, shot) in entry.screenshots.iter().enumerate() {
            match fetch(&shot.url, MAX_MEDIA_BYTES).await {
                Ok(bytes) if image::load_from_memory(&bytes).is_ok() => {}
                Ok(_) => checked.problems.push(format!(
                    "{}: screenshot {} isn't a PNG, JPEG or WebP image",
                    entry.id,
                    index + 1
                )),
                Err(error) => checked.problems.push(format!(
                    "{}: screenshot {}: {error}",
                    entry.id,
                    index + 1
                )),
            }
        }
        for release in &entry.releases {
            let at = format!("{} {}", entry.id, release.version);
            let bytes = match fetch(&release.artifact.url, release.artifact.size + 1).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    checked.problems.push(format!("{at}: {error}"));
                    continue;
                }
            };
            let dir = work.join("downloads");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let archive = dir.join(format!("{}.zip", release.artifact.sha256));
            std::fs::write(&archive, &bytes).map_err(|e| e.to_string())?;
            let unpacked = super::package::verify(&archive, release)
                .and_then(|()| {
                    super::package::extract(
                        &archive,
                        &work
                            .join("staging")
                            .join(format!("{}-{}", entry.id, release.version)),
                    )
                })
                .and_then(|root| {
                    super::package::check_identity(entry, release, &root).map(|()| root)
                });
            match unpacked {
                Ok(root) => checked.releases.push(CheckedRelease {
                    entry: entry.clone(),
                    release: release.clone(),
                    root,
                }),
                Err(error) => checked.problems.push(format!("{at}: {error}")),
            }
        }
    }
    Ok(checked)
}

/// The bytes at `url` (a file or an HTTP address), refusing more than
/// `max`.
async fn fetch(url: &str, max: u64) -> Result<Vec<u8>, String> {
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    if parsed.scheme() == "file" {
        let path = parsed
            .to_file_path()
            .map_err(|()| "not a file path".to_owned())?;
        let size = std::fs::metadata(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .len();
        if size > max {
            return Err(format!(
                "{} is larger than its catalogue size",
                path.display()
            ));
        }
        return std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()));
    }
    #[cfg(feature = "net")]
    {
        let client = reqwest::Client::builder()
            .user_agent(concat!("Caskade/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| e.to_string())?;
        let response = client
            .get(parsed)
            .send()
            .await
            .map_err(|e| format!("unreachable: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "the server answered {}",
                response.status().as_u16()
            ));
        }
        if response.content_length().is_some_and(|length| length > max) {
            return Err("larger than its catalogue size".to_owned());
        }
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() as u64 > max {
            return Err("larger than its catalogue size".to_owned());
        }
        Ok(bytes.to_vec())
    }
    #[cfg(not(feature = "net"))]
    Err("fetching over HTTP needs the net feature".to_owned())
}

/// Whether a plugin's `api` range targets plugin API 0.2 or later, whose
/// UI its backend draws (#109).
fn draws_ui(api: &str) -> bool {
    semver::VersionReq::parse(api).is_ok_and(|api| api.matches(&semver::Version::new(0, 2, 0)))
}
