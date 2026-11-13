//! Caskade's own updates without Tauri (#88), for the native app: the same
//! release feed (`latest.json` on GitHub releases, `app_update::ENDPOINT`)
//! and the same minisign key the Tauri app's updater checks. A release is
//! taken only when it's newer, its bundle's signature verifies against the
//! key, and the version the signature carries is the announced one.
//!
//! What a copy installs depends on how it was installed ([`Bundle`]): an
//! AppImage replaces itself, a deb goes over this one through one polkit
//! prompt (`app_update::deb`), the NSIS installer runs passively and
//! restarts Caskade, and the macOS app is swapped in place. A copy that
//! isn't one of those (a development build) says why it can't update.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;

/// The release feed's own size limit: `latest.json` is a few lines.
const FEED_LIMIT: u64 = 1024 * 1024;
/// A bundle's: Caskade is a few hundred megabytes at most.
const BUNDLE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// The key Caskade's releases are signed with, shared with release tooling.
pub fn public_key() -> Option<String> {
    Some(include_str!("../updater.pub").trim().to_owned())
}

/// How this copy of Caskade was installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bundle {
    AppImage,
    Deb,
    Nsis,
    App,
}

impl Bundle {
    /// This copy's, from where it runs; `None` for a development build.
    pub fn of_this_copy() -> Option<Self> {
        Self::of(&std::env::current_exe().ok()?, |name| {
            std::env::var_os(name)
        })
    }

    /// The bundle executable `exe` runs from, with the environment read
    /// through `var`.
    fn of(exe: &Path, var: impl Fn(&str) -> Option<OsString>) -> Option<Self> {
        if cfg!(target_os = "linux") && appimage(exe, var).is_some() {
            return Some(Self::AppImage);
        }
        if cfg!(target_os = "linux") && exe.starts_with("/usr/bin") {
            return Some(Self::Deb);
        }
        if cfg!(windows) && exe.with_file_name("uninstall.exe").is_file() {
            return Some(Self::Nsis);
        }
        if cfg!(target_os = "macos") && exe.to_string_lossy().contains(".app/Contents/MacOS/") {
            return Some(Self::App);
        }
        None
    }

    /// The feed's keys for this bundle on this machine, the most specific
    /// first.
    pub fn platforms(self) -> Vec<String> {
        let (os, arch) = (
            match std::env::consts::OS {
                "macos" => "darwin",
                other => other,
            },
            std::env::consts::ARCH,
        );
        let kind = match self {
            Self::AppImage => "appimage",
            Self::Deb => "deb",
            Self::Nsis => "nsis",
            Self::App => "app",
        };
        let mut keys = vec![format!("{os}-{arch}-{kind}")];
        // The plain key is the AppImage on Linux, NSIS on Windows, the app on
        // macOS; a deb has only its own.
        if self != Self::Deb {
            keys.push(format!("{os}-{arch}"));
        }
        keys
    }
}

#[derive(Debug, Deserialize)]
struct Feed {
    version: String,
    notes: Option<String>,
    pub_date: Option<String>,
    platforms: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Deserialize)]
struct Entry {
    signature: String,
    url: String,
}

/// A newer release, for this bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
    pub url: String,
    /// The bundle's signature, as the feed has it (base64 of minisign's).
    pub signature: String,
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("Caskade/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| error.to_string())
}

async fn body(response: reqwest::Response, limit: u64) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(caskade_core::tr!("The download is larger than expected."));
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err(caskade_core::tr!("The download is larger than expected."));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// The release the feed at `endpoint` offers this bundle, when it's newer
/// than `current`.
pub async fn find(
    endpoint: &str,
    current: &str,
    bundle: Bundle,
) -> Result<Option<Release>, String> {
    let response = client()?
        .get(endpoint)
        .timeout(CHECK_TIMEOUT)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| caskade_core::tr!("Couldn't check for updates: {error}", error = error))?;
    let feed: Feed =
        serde_json::from_slice(&body(response, FEED_LIMIT).await?).map_err(|error| {
            caskade_core::tr!("The update feed isn't readable: {error}", error = error)
        })?;
    let newer = match (
        semver::Version::parse(feed.version.trim_start_matches('v')),
        semver::Version::parse(current.trim_start_matches('v')),
    ) {
        (Ok(offered), Ok(current)) => offered > current,
        _ => {
            return Err(caskade_core::tr!(
                "The update feed's version isn't readable."
            ));
        }
    };
    if !newer {
        return Ok(None);
    }
    let Some(entry) = bundle
        .platforms()
        .iter()
        .find_map(|key| feed.platforms.get(key))
        .cloned()
    else {
        return Err(caskade_core::tr!(
            "The new version has no download for this system yet."
        ));
    };
    Ok(Some(Release {
        version: feed.version,
        notes: feed.notes.filter(|notes| !notes.trim().is_empty()),
        date: feed.pub_date,
        url: entry.url,
        signature: entry.signature,
    }))
}

/// Downloads `release`'s bundle, telling `progress` the bytes so far and
/// the total when known, and verifies it against `public_key`.
pub async fn download(
    release: &Release,
    public_key: &str,
    timeout: Duration,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    let response = client()?
        .get(&release.url)
        .timeout(timeout)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| {
            caskade_core::tr!("Couldn't download the update: {error}", error = error)
        })?;
    let total = response.content_length();
    if total.is_some_and(|length| length > BUNDLE_LIMIT) {
        return Err(caskade_core::tr!("The download is larger than expected."));
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        caskade_core::tr!("Couldn't download the update: {error}", error = error)
    })? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 > BUNDLE_LIMIT {
            return Err(caskade_core::tr!("The download is larger than expected."));
        }
        progress(bytes.len() as u64, total);
    }
    verify(&bytes, &release.signature, public_key, &release.version)?;
    Ok(bytes)
}

fn base64_text(text: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

/// Whether `bytes` are what `signature` (the feed's, base64) signs with
/// `public_key` (base64, as the Tauri config has it), for `version`.
pub fn verify(
    bytes: &[u8],
    signature: &str,
    public_key: &str,
    version: &str,
) -> Result<(), String> {
    let refused =
        |why: String| caskade_core::tr!("The update's signature doesn't match: {why}", why = why);
    let key = minisign_verify::PublicKey::decode(&base64_text(public_key).map_err(refused)?)
        .map_err(|error| refused(error.to_string()))?;
    let signature = minisign_verify::Signature::decode(&base64_text(signature).map_err(refused)?)
        .map_err(|error| refused(error.to_string()))?;
    key.verify(bytes, &signature, false)
        .map_err(|error| refused(error.to_string()))?;
    // Only now is the trusted comment trusted: the global signature covers
    // it. A signature for another version is a replayed old release.
    let signed = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"));
    match signed {
        Some(signed) if signed.trim_start_matches('v') != version.trim_start_matches('v') => {
            Err(refused(format!("it's for version {signed}")))
        }
        _ => Ok(()),
    }
}

/// Installs a verified `bundle` download. The AppImage and the macOS app are
/// replaced in place (the new version runs after a restart); the deb is
/// installed over this one; NSIS's installer is started passively and
/// restarts Caskade itself, so the caller exits.
pub fn install(bundle: Bundle, bytes: &[u8], version: &str) -> Result<(), String> {
    match bundle {
        Bundle::AppImage => {
            let target = std::env::current_exe()
                .ok()
                .and_then(|exe| appimage(&exe, |name| std::env::var_os(name)))
                .ok_or_else(|| caskade_core::tr!("This copy isn't an AppImage."))?;
            replace_appimage(&target, bytes)
        }
        #[cfg(target_os = "linux")]
        Bundle::Deb => crate::commands::app_update::deb::install(bytes, version),
        Bundle::Nsis => run_installer(bytes),
        Bundle::App => replace_app(bytes),
        #[cfg(not(target_os = "linux"))]
        Bundle::Deb => {
            let _ = version;
            Err(caskade_core::tr!("This copy can't install a deb."))
        }
    }
}

/// The AppImage file executable `exe` runs from, with the environment read
/// through `var`. An `APPIMAGE` another AppImage passed down names that one,
/// not Caskade (`caskade_core::program`).
fn appimage(exe: &Path, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let program = caskade_core::program::from(exe.to_path_buf(), var);
    (program != exe).then_some(program)
}

/// Replaces the AppImage at `target` with `bytes`, keeping its mode; the
/// running one keeps going from its mount.
pub fn replace_appimage(target: &Path, bytes: &[u8]) -> Result<(), String> {
    if !bytes.starts_with(b"\x7fELF") {
        return Err(caskade_core::tr!("The update isn't an AppImage."));
    }
    let partial = target.with_extension("update-partial");
    std::fs::write(&partial, bytes).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(target)
            .map(|metadata| metadata.permissions().mode())
            .unwrap_or(0o755);
        std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(mode | 0o111))
            .map_err(|error| error.to_string())?;
    }
    std::fs::rename(&partial, target).map_err(|error| {
        let _ = std::fs::remove_file(&partial);
        error.to_string()
    })
}

/// Windows: starts the NSIS installer passively (`/P`) as an update
/// (`/UPDATE`); it closes Caskade and starts the new version.
fn run_installer(bytes: &[u8]) -> Result<(), String> {
    if !bytes.starts_with(b"MZ") {
        return Err(caskade_core::tr!("The update isn't an installer."));
    }
    let folder = std::env::temp_dir().join(format!("caskade-update-{}", std::process::id()));
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let installer = folder.join("caskade-setup.exe");
    std::fs::write(&installer, bytes).map_err(|error| error.to_string())?;
    std::process::Command::new(&installer)
        .args(["/P", "/UPDATE", "/R"])
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// macOS: unpacks the new `Caskade.app` (a `.app.tar.gz`) beside this one
/// and swaps it in.
#[cfg(target_os = "macos")]
fn replace_app(bytes: &[u8]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let app = exe
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .ok_or_else(|| caskade_core::tr!("This copy isn't in an app bundle."))?
        .to_path_buf();
    let parent = app.parent().ok_or_else(|| "no parent".to_owned())?;
    let staging = parent.join(format!(".caskade-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    tar::Archive::new(flate2::read::GzDecoder::new(bytes))
        .unpack(&staging)
        .map_err(|error| error.to_string())?;
    let unpacked = std::fs::read_dir(&staging)
        .map_err(|error| error.to_string())?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .ok_or_else(|| caskade_core::tr!("The update has no app in it."))?;
    let old = parent.join(format!(".caskade-old-{}", std::process::id()));
    std::fs::rename(&app, &old).map_err(|error| error.to_string())?;
    if let Err(error) = std::fs::rename(&unpacked, &app) {
        let _ = std::fs::rename(&old, &app);
        return Err(error.to_string());
    }
    let _ = std::fs::remove_dir_all(&old);
    let _ = std::fs::remove_dir_all(&staging);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn replace_app(_bytes: &[u8]) -> Result<(), String> {
    Err(caskade_core::tr!("This copy isn't a macOS app."))
}

#[cfg(test)]
#[path = "self_update_tests.rs"]
mod tests;
