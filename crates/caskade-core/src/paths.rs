//! Per-OS application directories (`directories`), overridable for tests
//! and portable installs.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const QUALIFIER: &str = "dev";
const ORGANIZATION: &str = "Caskade";
const APPLICATION: &str = "Caskade";

/// Where Caskade keeps its database, configuration, caches and logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppPaths {
    /// Durable state: the library database, plugin data.
    pub data: PathBuf,
    /// User configuration.
    pub config: PathBuf,
    /// Re-creatable files: artwork thumbnails, download cache.
    pub cache: PathBuf,
    /// Rolling log files.
    pub logs: PathBuf,
    /// The storage location registered on first start: `~/Games/Caskade`.
    pub games: PathBuf,
}

impl AppPaths {
    /// The platform's standard locations.
    pub fn standard() -> Result<Self> {
        let dirs = directories::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .ok_or(Error::NoAppDirs)?;
        Ok(Self {
            data: dirs.data_dir().to_path_buf(),
            config: dirs.config_dir().to_path_buf(),
            cache: dirs.cache_dir().to_path_buf(),
            logs: dirs.data_local_dir().join("logs"),
            games: directories::UserDirs::new().map_or_else(
                || dirs.data_dir().join("games"),
                |user| user.home_dir().join("Games").join(APPLICATION),
            ),
        })
    }

    /// Everything under one root (tests, portable mode).
    pub fn under(root: &Path) -> Self {
        Self {
            data: root.join("data"),
            config: root.join("config"),
            cache: root.join("cache"),
            logs: root.join("logs"),
            games: root.join("games"),
        }
    }

    /// The SQLite library database.
    pub fn database(&self) -> PathBuf {
        self.data.join("library.sqlite")
    }

    /// Creates every directory.
    pub fn create(&self) -> Result<()> {
        for dir in [&self.data, &self.config, &self.cache, &self.logs] {
            // Name the folder: startup shows this to people (#160).
            std::fs::create_dir_all(dir).map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    crate::tr!(
                        "couldn't create “{dir}”: {error}",
                        dir = dir.display(),
                        error = error
                    ),
                )
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooted_paths_are_created() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(temp.path());
        paths.create().unwrap();
        assert!(
            paths.data.is_dir()
                && paths.config.is_dir()
                && paths.cache.is_dir()
                && paths.logs.is_dir()
        );
        assert_eq!(paths.database(), temp.path().join("data/library.sqlite"));
        AppPaths::standard().unwrap();
    }
}
