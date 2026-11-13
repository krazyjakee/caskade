//! umu-launcher bootstrap. When no `umu-run` is on the
//! host, the pinned release zipapp is installed user-locally into the app
//! data dir. umu downloads its Steam Runtime and UMU-Proton itself on first
//! use; callers show that first run as its own job.
//!
//! The download is bounded: connect and stall timeouts, two retries and a size
//! cap, so an offline host fails within about a minute and never hangs.
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

const UMU_VERSION: &str = "1.4.4";
const UMU_URL: &str = "https://github.com/Open-Wine-Components/umu-launcher/releases/download/1.4.4/umu-launcher-1.4.4-zipapp.tar";
const UMU_SHA256: &str = "eb590691841f7fad3fc3ad8fd5db4ccb87849fe7948e62b28ece7a4ee48cc851";
/// The release archive is ~420 KiB; refuse anything far larger.
const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;
#[cfg(feature = "fetch")]
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
/// How long the server may send nothing, before or during the body.
#[cfg(feature = "fetch")]
const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
#[cfg(feature = "fetch")]
const RETRIES: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum UmuError {
    #[error("umu-launcher archive checksum mismatch (expected {UMU_SHA256})")]
    Checksum,
    #[error("umu-launcher archive is invalid: {0}")]
    Invalid(String),
    #[error("umu-launcher download failed: {0}")]
    Download(String),
    #[error("umu-launcher download cancelled")]
    Cancelled,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Where the pinned umu-launcher lives, e.g. `~/.local/share/caskade/umu/1.4.4`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UmuBootstrap {
    dir: PathBuf,
    url: String,
}

impl UmuBootstrap {
    pub fn new(app_data: impl AsRef<Path>) -> Self {
        Self {
            dir: app_data.as_ref().join("umu").join(UMU_VERSION),
            url: UMU_URL.into(),
        }
    }

    /// Download the pinned archive from `url` instead of GitHub (a mirror,
    /// or a test server). The checksum stays pinned.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    /// `$XDG_DATA_HOME/caskade` (or `~/.local/share/caskade`).
    pub fn for_host() -> Option<Self> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })?;
        Some(Self::new(base.join("caskade")))
    }

    pub fn umu_run(&self) -> PathBuf {
        self.dir.join("umu/umu-run")
    }

    pub fn installed(&self) -> Option<PathBuf> {
        Some(self.umu_run()).filter(|path| path.is_file())
    }

    /// Verify the pinned checksum, then unpack regular files below `umu/`
    /// into a fresh directory that replaces any previous attempt atomically.
    pub fn install_archive(&self, archive: &[u8]) -> Result<PathBuf, UmuError> {
        if archive.len() as u64 > MAX_ARCHIVE_BYTES {
            return Err(UmuError::Invalid("archive too large".into()));
        }
        if format!("{:x}", Sha256::digest(archive)) != UMU_SHA256 {
            return Err(UmuError::Checksum);
        }
        let parent = self
            .dir
            .parent()
            .ok_or_else(|| UmuError::Invalid("no parent".into()))?;
        fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new()
            .prefix(".umu-")
            .tempdir_in(parent)?;
        let mut tar = tar::Archive::new(io::Cursor::new(archive));
        let mut unpacked = 0u64;
        for entry in tar.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            let safe = path
                .components()
                .all(|component| matches!(component, Component::Normal(_)));
            if !safe || !path.starts_with("umu") {
                return Err(UmuError::Invalid(format!(
                    "unexpected entry {}",
                    path.display()
                )));
            }
            let kind = entry.header().entry_type();
            let target = staging.path().join(&path);
            if kind.is_dir() {
                fs::create_dir_all(&target)?;
                continue;
            }
            // Convenience links (e.g. umu_run.py) are not needed; never
            // create links from a downloaded archive.
            if kind.is_symlink() || kind.is_hard_link() {
                continue;
            }
            if !kind.is_file() {
                return Err(UmuError::Invalid(format!(
                    "non-file entry {}",
                    path.display()
                )));
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut bytes = vec![];
            entry
                .by_ref()
                .take(MAX_ARCHIVE_BYTES)
                .read_to_end(&mut bytes)?;
            unpacked += bytes.len() as u64;
            if unpacked > 4 * MAX_ARCHIVE_BYTES {
                return Err(UmuError::Invalid("archive expands too far".into()));
            }
            fs::write(&target, bytes)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = entry.header().mode().unwrap_or(0o644) & 0o755;
                fs::set_permissions(&target, fs::Permissions::from_mode(mode | 0o600))?;
            }
        }
        if !staging.path().join("umu/umu-run").is_file() {
            return Err(UmuError::Invalid("archive has no umu/umu-run".into()));
        }
        // Another process may have finished the same install meanwhile; its
        // copy is as good as this one.
        if let Some(path) = self.installed() {
            return Ok(path);
        }
        let staging = staging.keep();
        let replaced = (|| {
            if self.dir.exists() {
                fs::remove_dir_all(&self.dir)?;
            }
            fs::rename(&staging, &self.dir)
        })();
        match replaced {
            Ok(()) => Ok(self.umu_run()),
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                self.installed().ok_or(UmuError::Io(error))
            }
        }
    }

    /// Download the pinned release when missing; returns `umu-run`.
    #[cfg(feature = "fetch")]
    pub async fn ensure(&self) -> Result<PathBuf, UmuError> {
        self.ensure_with(&tokio_util::sync::CancellationToken::new())
            .await
    }

    /// [`ensure`](Self::ensure) that stops when `cancel` fires. Concurrent
    /// calls in this process share one download.
    #[cfg(feature = "fetch")]
    pub(crate) async fn ensure_with(
        &self,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<PathBuf, UmuError> {
        if let Some(path) = self.installed() {
            return Ok(path);
        }
        static INSTALLING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _installing = tokio::select! {
            _ = cancel.cancelled() => return Err(UmuError::Cancelled),
            guard = INSTALLING.lock() => guard,
        };
        if let Some(path) = self.installed() {
            return Ok(path);
        }
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(STALL_TIMEOUT)
            .use_rustls_tls()
            .build()
            .map_err(|error| UmuError::Download(crate::fetch::describe_http_error(error)))?;
        let mut failures = 0;
        let archive = loop {
            let result = tokio::select! {
                _ = cancel.cancelled() => return Err(UmuError::Cancelled),
                result = self.download(&client) => result,
            };
            match result {
                Ok(archive) => break archive,
                Err((_, true)) if failures < RETRIES => {
                    let delay = std::time::Duration::from_millis(500) * 2_u32.pow(failures);
                    failures += 1;
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(UmuError::Cancelled),
                        _ = tokio::time::sleep(delay) => (),
                    }
                }
                Err((error, _)) => return Err(error),
            }
        };
        let this = self.clone();
        caskade_jobs::blocking("Preparing compatibility tools", move || {
            this.install_archive(&archive)
        })
        .await
        .map_err(|error| UmuError::Download(error.to_string()))?
    }

    /// One attempt; the flag says whether another may succeed.
    #[cfg(feature = "fetch")]
    async fn download(&self, client: &reqwest::Client) -> Result<Vec<u8>, (UmuError, bool)> {
        use futures_util::StreamExt;
        let transport = |error| {
            (
                UmuError::Download(crate::fetch::describe_http_error(error)),
                true,
            )
        };
        let response = client.get(&self.url).send().await.map_err(transport)?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            return Err((
                UmuError::Download(format!("the server returned HTTP {status}")),
                matches!(status, 408 | 429 | 500..=599),
            ));
        }
        let too_large = || (UmuError::Invalid("archive too large".into()), false);
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ARCHIVE_BYTES)
        {
            return Err(too_large());
        }
        let mut archive = Vec::new();
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(transport)?;
            if (archive.len() + chunk.len()) as u64 > MAX_ARCHIVE_BYTES {
                return Err(too_large());
            }
            archive.extend_from_slice(&chunk);
        }
        Ok(archive)
    }
}
