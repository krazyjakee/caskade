//! Stable alias paths (INSTALLER-PLAN §7.1, layer 2). Each game gets a short
//! ID and an alias `<base>/<id>` that points at its real directory: a
//! directory junction on Windows (no admin rights needed) or a symlink
//! elsewhere. Installers and games see the alias, so moving a game is a
//! retarget rather than a rewrite. Removing an alias never touches its target.
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum AliasError {
    #[error("invalid alias id {0:?}")]
    InvalidId(String),
    #[error("alias target must be an existing absolute directory: {0}")]
    InvalidTarget(PathBuf),
    #[error("no alias for {0}: junctions cannot point at network shares")]
    NetworkTarget(PathBuf),
    #[error("{0} exists and is not an alias link")]
    NotALink(PathBuf),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Directory holding the aliases, e.g. `%LOCALAPPDATA%\Caskade\g`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct AliasStore {
    base: PathBuf,
}

impl AliasStore {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    /// The per-user application data location for this host, if known.
    pub fn for_host() -> Option<Self> {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let base = if cfg!(windows) {
            var("LOCALAPPDATA")?.join("Caskade")
        } else if cfg!(target_os = "macos") {
            var("HOME")?.join("Library/Application Support/Caskade")
        } else {
            var("XDG_DATA_HOME")
                .or_else(|| var("HOME").map(|home| home.join(".local/share")))?
                .join("caskade")
        };
        Some(Self::new(base.join("g")))
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Stable 12-hex-digit ID for a game key (e.g. root ID + game directory).
    pub fn short_id(key: &str) -> String {
        Sha256::digest(key.as_bytes())[..6]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn path(&self, id: &str) -> Result<PathBuf, AliasError> {
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(AliasError::InvalidId(id.into()));
        }
        Ok(self.base.join(id))
    }

    /// Create the alias, or retarget an existing one, and return its path.
    pub fn link(&self, id: &str, target: &Path) -> Result<PathBuf, AliasError> {
        let alias = self.path(id)?;
        if !target.is_absolute() || !target.is_dir() {
            return Err(AliasError::InvalidTarget(target.into()));
        }
        if !supports_alias(target) {
            return Err(AliasError::NetworkTarget(target.into()));
        }
        fs::create_dir_all(&self.base)?;
        match fs::symlink_metadata(&alias) {
            Ok(_) if self.target(id)?.as_deref() == Some(target) => return Ok(alias),
            Ok(_) => self.remove(id)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        create(target, &alias)?;
        Ok(alias)
    }

    /// Put an alias back exactly as it was (rollback), even when its target
    /// no longer exists, e.g. after the folder was renamed outside Caskade.
    pub fn restore(&self, id: &str, target: &Path) -> Result<PathBuf, AliasError> {
        let alias = self.path(id)?;
        if !target.is_absolute() {
            return Err(AliasError::InvalidTarget(target.into()));
        }
        self.remove(id)?;
        fs::create_dir_all(&self.base)?;
        create(target, &alias)?;
        Ok(alias)
    }

    /// The alias's current target, or `None` when no alias exists.
    pub fn target(&self, id: &str) -> Result<Option<PathBuf>, AliasError> {
        let alias = self.path(id)?;
        match fs::symlink_metadata(&alias) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
            Ok(_) => read(&alias).map(Some),
        }
    }

    /// Remove the alias link only; a missing alias is not an error.
    pub fn remove(&self, id: &str) -> Result<(), AliasError> {
        let alias = self.path(id)?;
        match fs::symlink_metadata(&alias) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
            Ok(_) => {
                read(&alias)?;
                delete(&alias)
            }
        }
    }
}

impl crate::manifest::AliasRemover for AliasStore {
    fn remove_alias(
        &self,
        _: &Path,
        manifest: &crate::manifest::InstallManifest,
    ) -> io::Result<bool> {
        let Some(id) = &manifest.record().alias else {
            return Ok(false);
        };
        let existed = self.target(id).map_err(io::Error::other)?.is_some();
        self.remove(id).map_err(io::Error::other)?;
        Ok(existed)
    }
}

/// Junctions cannot target network shares; such roots get no alias.
pub fn supports_alias(target: &Path) -> bool {
    !cfg!(windows) || {
        let text = target.as_os_str().to_string_lossy();
        let unc = text.strip_prefix(r"\\?\").unwrap_or(&text);
        !(unc.starts_with(r"\\") || unc.starts_with("UNC\\"))
    }
}

#[cfg(windows)]
fn create(target: &Path, alias: &Path) -> Result<(), AliasError> {
    Ok(junction::create(target, alias)?)
}
#[cfg(windows)]
fn read(alias: &Path) -> Result<PathBuf, AliasError> {
    use std::os::windows::fs::FileTypeExt;
    // Not `junction::exists`: it follows the link first, so a junction whose
    // folder was just renamed (a same-volume move, an outside rename) would
    // read as a plain directory (#270). The link's own metadata says whether
    // it is a directory link; `get_target` then refuses any other kind (a
    // directory symlink) with `ErrorKind::Other`.
    if !fs::symlink_metadata(alias)?.file_type().is_symlink_dir() {
        return Err(AliasError::NotALink(alias.into()));
    }
    match junction::get_target(alias) {
        Ok(target) => Ok(target),
        Err(error) if error.kind() == io::ErrorKind::Other => {
            Err(AliasError::NotALink(alias.into()))
        }
        Err(error) => Err(error.into()),
    }
}
#[cfg(windows)]
fn delete(alias: &Path) -> Result<(), AliasError> {
    junction::delete(alias)?;
    Ok(fs::remove_dir(alias)?)
}

#[cfg(unix)]
fn create(target: &Path, alias: &Path) -> Result<(), AliasError> {
    Ok(std::os::unix::fs::symlink(target, alias)?)
}
#[cfg(unix)]
fn read(alias: &Path) -> Result<PathBuf, AliasError> {
    if !fs::symlink_metadata(alias)?.file_type().is_symlink() {
        return Err(AliasError::NotALink(alias.into()));
    }
    Ok(fs::read_link(alias)?)
}
#[cfg(unix)]
fn delete(alias: &Path) -> Result<(), AliasError> {
    Ok(fs::remove_file(alias)?)
}

#[cfg(not(any(unix, windows)))]
fn create(_: &Path, alias: &Path) -> Result<(), AliasError> {
    Err(AliasError::NotALink(alias.into()))
}
#[cfg(not(any(unix, windows)))]
fn read(alias: &Path) -> Result<PathBuf, AliasError> {
    Err(AliasError::NotALink(alias.into()))
}
#[cfg(not(any(unix, windows)))]
fn delete(alias: &Path) -> Result<(), AliasError> {
    Err(AliasError::NotALink(alias.into()))
}
