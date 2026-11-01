//! Free-space queries are injected so tests can model exhausted disks.

use std::{io, path::Path};

pub trait DiskSpace: Send + Sync {
    fn available_bytes(&self, path: &Path) -> io::Result<u64>;
}

/// Production edge adapter; queries the filesystem containing `path`.
#[derive(Debug, Default, Clone, Copy)]
pub struct FileSystemDiskSpace;

impl DiskSpace for FileSystemDiskSpace {
    fn available_bytes(&self, path: &Path) -> io::Result<u64> {
        fs2::available_space(path)
    }
}
