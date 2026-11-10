//! Why an app-data backup failed. Backups live in `caskade-backup`; their
//! error stays here so [`crate::error`] can classify it.

use std::path::PathBuf;

/// Why a backup failed. The text is for people.
#[derive(Debug)]
#[non_exhaustive]
pub enum BackupError {
    /// Another backup or restore is running.
    Busy,
    Cancelled,
    /// The destination folder isn't there (a drive not connected).
    DestinationMissing(PathBuf),
    NoSpace {
        needed: u64,
        available: u64,
    },
    /// The archive or the snapshot isn't what it should be.
    Damaged(String),
    /// A newer Caskade wrote it.
    Newer,
    Io(std::io::Error),
    Store(crate::Error),
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::Busy => crate::tr!("Another backup or restore is under way."),
            Self::Cancelled => crate::tr!("The backup was cancelled."),
            Self::DestinationMissing(path) => crate::tr!(
                "The backup folder “{path}” isn't there. Connect its drive or choose another folder.",
                path = path.display()
            ),
            Self::NoSpace { needed, available } => crate::tr!(
                "There isn't enough free space for the backup: it needs about {needed} MB and {available} MB are free.",
                needed = needed.div_ceil(1024 * 1024),
                available = available / (1024 * 1024)
            ),
            Self::Damaged(why) => crate::tr!("The backup is damaged: {why}.", why = why),
            Self::Newer => {
                crate::tr!("This backup was made by a newer version of Caskade.")
            }
            Self::Io(error) => error.to_string(),
            Self::Store(error) => error.to_string(),
        })
    }
}

impl std::error::Error for BackupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for BackupError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<crate::Error> for BackupError {
    fn from(error: crate::Error) -> Self {
        Self::Store(error)
    }
}

impl From<serde_json::Error> for BackupError {
    fn from(error: serde_json::Error) -> Self {
        Self::Io(std::io::Error::other(error))
    }
}
