//! The host's configuration (STREAMING-PLAN §4.1), written whole before
//! every start: Caskade owns it. Sunshine keeps its configuration, apps,
//! state, credentials and log in one folder ([`HostFiles`]): on Linux
//! `$CONFIGURATION_DIRECTORY/sunshine` (Caskade sets the variable), on
//! Windows `config` beside the program.

use crate::Result;
use std::path::{Path, PathBuf};

/// Where Sunshine keeps its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFiles {
    pub dir: PathBuf,
}

impl HostFiles {
    /// Linux: `CONFIGURATION_DIRECTORY` is `base`; Sunshine adds `sunshine`.
    pub fn linux(base: &Path) -> Self {
        Self {
            dir: base.join("sunshine"),
        }
    }

    /// Windows: `config` beside `sunshine.exe`.
    pub fn windows(program_dir: &Path) -> Self {
        Self {
            dir: program_dir.join("config"),
        }
    }

    pub fn conf(&self) -> PathBuf {
        self.dir.join("sunshine.conf")
    }

    pub fn apps(&self) -> PathBuf {
        self.dir.join("apps.json")
    }

    /// The web credentials' salted hash, which `--creds` writes.
    pub fn state(&self) -> PathBuf {
        self.dir.join("sunshine_state.json")
    }

    /// The certificate the host serves its API with, made at first start;
    /// the API client is pinned to it.
    pub fn cert(&self) -> PathBuf {
        self.dir.join("credentials").join("cacert.pem")
    }
}

/// What Caskade sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostConfig {
    /// The device's name in Caskade: what clients show.
    pub name: String,
}

/// The device name as one line of `sunshine.conf`: printable, at most 64
/// characters, and never empty.
fn clean_name(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect::<String>()
        .trim()
        .to_owned();
    if name.is_empty() {
        "Caskade".to_owned()
    } else {
        name
    }
}

/// `sunshine.conf`: no tray icon or notifications, the web API on this
/// computer only, no UPnP (the stream stays on the local network) and no
/// pre-release notices.
pub fn render(config: &HostConfig) -> String {
    format!(
        "# Written by Caskade, which replaces it at every start.\n\
         sunshine_name = {}\n\
         system_tray = disabled\n\
         origin_web_ui_allowed = pc\n\
         upnp = disabled\n\
         notify_pre_releases = disabled\n",
        clean_name(&config.name)
    )
}

/// An app list with none of Sunshine's defaults ("Desktop", "Steam Big
/// Picture"): Caskade adds its own through the API.
pub const EMPTY_APPS: &str = "{\n  \"env\": {},\n  \"apps\": []\n}\n";

/// Writes `sunshine.conf`, and the app list when there is none yet: once
/// Sunshine runs, the list is its own and changes only through the API.
pub fn write(files: &HostFiles, config: &HostConfig) -> Result<()> {
    std::fs::create_dir_all(&files.dir)?;
    write_atomically(&files.conf(), render(config).as_bytes())?;
    if !files.apps().exists() {
        write_atomically(&files.apps(), EMPTY_APPS.as_bytes())?;
    }
    Ok(())
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    std::fs::write(&part, bytes)?;
    std::fs::rename(&part, path)?;
    Ok(())
}
