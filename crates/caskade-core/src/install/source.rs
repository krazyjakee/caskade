//! Retire only the unchanged, explicitly selected local source, after commit.
use super::*;
use crate::settings::SourceHandling;
use cap_std::fs::Dir;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

pub(super) struct SourceCleanup {
    path: PathBuf,
    snapshot: std::io::Result<BTreeMap<PathBuf, String>>,
    handling: SourceHandling,
}
impl SourceCleanup {
    pub async fn capture(
        service: &InstallService,
        prepared: &pipeline::input::Prepared,
        original: &str,
        handling: SourceHandling,
        cancel: CancellationToken,
    ) -> Result<Option<Self>> {
        if handling == SourceHandling::Keep || prepared.downloaded {
            return Ok(None);
        }
        let caskade_installer::Input::Path(selected) = &prepared.source else {
            return Ok(None);
        };
        let path = selected.clone();
        let original = PathBuf::from(original);
        let mut protected: Vec<PathBuf> = service
            .inner
            .store
            .roots()
            .await?
            .into_iter()
            .map(|r| r.path)
            .collect();
        if let Some(aliases) = &service.inner.options.aliases {
            protected.push(aliases.base().to_path_buf());
        }
        let value = caskade_jobs::blocking(crate::tr!("Checking install source"), move || {
            let snapshot = (|| {
                if fs::symlink_metadata(&original)?.is_symlink() {
                    return Err(io::Error::other(
                        "selected source is a symlink; kept its target",
                    ));
                }
                let source = fs::canonicalize(&path)?;
                if source != path || source.parent().is_none() {
                    return Err(io::Error::other("source identity/path is not canonical"));
                }
                for root in protected {
                    let root = canonical_protected(&root)?;
                    if source.starts_with(&root) || root.starts_with(&source) {
                        return Err(io::Error::other(
                            "source overlaps a storage root or alias store",
                        ));
                    }
                }
                snapshot(&source, &cancel)
            })();
            Self {
                path,
                snapshot,
                handling,
            }
        })
        .await
        .map_err(|e| Error::Invalid(e.to_string()))?;
        Ok(Some(value))
    }
    pub async fn finish(self, cancel: CancellationToken) -> Option<caskade_installer::Warning> {
        let path = self.path.clone();
        let result = caskade_jobs::blocking(
            crate::tr!("Cleaning install source"),
            move || -> io::Result<()> {
                let before = self.snapshot?;
                if fs::canonicalize(&self.path)? != self.path
                    || snapshot(&self.path, &cancel)? != before
                {
                    return Err(io::Error::other(
                        "source changed during installation; kept the source",
                    ));
                }
                if cancel.is_cancelled() {
                    return Err(io::Error::other("source cleanup cancelled"));
                }
                match self.handling {
                    SourceHandling::Keep => Ok(()),
                    SourceHandling::Trash => trash::delete(&self.path).map_err(io::Error::other),
                    SourceHandling::Delete => {
                        let parent = Dir::open_ambient_dir(
                            self.path
                                .parent()
                                .ok_or_else(|| io::Error::other("source has no parent"))?,
                            cap_std::ambient_authority(),
                        )?;
                        let name = self
                            .path
                            .file_name()
                            .ok_or_else(|| io::Error::other("source has no filename"))?;
                        if parent.symlink_metadata(name)?.is_dir() {
                            parent.remove_dir_all(name)
                        } else {
                            parent.remove_file(name)
                        }
                    }
                }
            },
        )
        .await
        .map_err(io::Error::other)
        .and_then(|r| r);
        result.err().map(|e| caskade_installer::Warning {
            code: "source_retained".into(),
            message: crate::tr!(
                "Installed successfully; source cleanup did not complete: {error}",
                error = e
            ),
            path: Some(path),
        })
    }
}

// Resolve existing ancestors even when an alias directory has not been
// created yet. This also normalizes Windows verbatim prefixes consistently.
fn canonical_protected(path: &Path) -> io::Result<PathBuf> {
    let mut existing = path;
    let mut tail = Vec::new();
    loop {
        match fs::canonicalize(existing) {
            Ok(mut path) => {
                for part in tail.into_iter().rev() {
                    path.push(part);
                }
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tail.push(existing.file_name().ok_or(error)?);
                existing = existing
                    .parent()
                    .ok_or_else(|| io::Error::other("protected path has no existing ancestor"))?;
            }
            Err(error) => return Err(error),
        }
    }
}
fn snapshot(path: &Path, cancel: &CancellationToken) -> io::Result<BTreeMap<PathBuf, String>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Err(io::Error::other("source contains a symlink"));
    }
    let mut stamps = BTreeMap::new();
    let mut bytes = 0;
    let parent = Dir::open_ambient_dir(
        path.parent()
            .ok_or_else(|| io::Error::other("source has no parent"))?,
        cap_std::ambient_authority(),
    )?;
    walk(
        &parent,
        Path::new(
            path.file_name()
                .ok_or_else(|| io::Error::other("source has no filename"))?,
        ),
        Path::new(""),
        &mut stamps,
        &mut bytes,
        cancel,
    )?;
    Ok(stamps)
}
fn walk(
    parent: &Dir,
    name: &Path,
    relative: &Path,
    stamps: &mut BTreeMap<PathBuf, String>,
    bytes: &mut u64,
    cancel: &CancellationToken,
) -> io::Result<()> {
    if cancel.is_cancelled() {
        return Err(io::Error::other("cancelled"));
    }
    if relative.components().count() > 64 || stamps.len() >= 100_000 {
        return Err(io::Error::other("source inventory exceeds cleanup limits"));
    }
    let metadata = parent.symlink_metadata(name)?;
    if metadata.is_symlink() {
        return Err(io::Error::other("source contains a symlink"));
    }
    if metadata.is_dir() {
        let dir = parent.open_dir(name)?;
        if dir.symlink_metadata(".caskade").is_ok() {
            return Err(io::Error::other("source contains managed metadata"));
        }
        stamps.insert(
            relative.into(),
            format!("dir:{:?}:{:?}", metadata.created(), metadata.modified()),
        );
        for entry in dir.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            walk(
                &dir,
                Path::new(&name),
                &relative.join(&name),
                stamps,
                bytes,
                cancel,
            )?;
        }
    } else if metadata.is_file() {
        let mut file = parent.open(name)?;
        let mut digest = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            if cancel.is_cancelled() {
                return Err(io::Error::other("cancelled"));
            }
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            *bytes = bytes
                .checked_add(n as u64)
                .ok_or_else(|| io::Error::other("source too large"))?;
            if *bytes > 1024 * 1024 * 1024 * 1024 {
                return Err(io::Error::other("source exceeds cleanup byte budget"));
            }
            digest.update(&buffer[..n]);
        }
        stamps.insert(
            relative.into(),
            format!(
                "{:x}:{:?}:{:?}",
                digest.finalize(),
                metadata.created(),
                metadata.modified()
            ),
        );
    } else {
        return Err(io::Error::other("source contains a special file"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn changed_nested_sources_and_selected_links_are_retained() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("source");
        fs::create_dir_all(folder.join("nested")).unwrap();
        fs::write(folder.join("nested/game"), b"old").unwrap();
        let cancel = CancellationToken::new();
        let cleanup = SourceCleanup {
            path: folder.clone(),
            snapshot: snapshot(&folder, &cancel),
            handling: SourceHandling::Delete,
        };
        fs::write(folder.join("nested/new-save"), b"new").unwrap();
        let warning = cleanup.finish(cancel).await.unwrap();
        assert_eq!(warning.code, "source_retained");
        assert!(folder.join("nested/new-save").is_file());
        #[cfg(unix)]
        {
            let outside = temp.path().join("outside");
            fs::write(&outside, b"preserve").unwrap();
            std::os::unix::fs::symlink(&outside, folder.join("link")).unwrap();
            assert!(snapshot(&folder, &CancellationToken::new()).is_err());
            assert_eq!(fs::read(outside).unwrap(), b"preserve");
        }
    }

    #[test]
    fn missing_alias_directories_resolve_through_existing_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().canonicalize().unwrap();
        assert_eq!(
            canonical_protected(&parent.join("future/aliases")).unwrap(),
            parent.join("future/aliases")
        );
        #[cfg(unix)]
        {
            let link = temp.path().join("link");
            std::os::unix::fs::symlink(&parent, &link).unwrap();
            assert_eq!(
                canonical_protected(&link.join("future/aliases")).unwrap(),
                parent.join("future/aliases")
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires CASKADE_TEST_NATIVE=1; uses a subprocess with a private HOME/XDG trash"]
    fn native_trash_uses_an_isolated_os_trash() {
        assert_eq!(std::env::var("CASKADE_TEST_NATIVE").as_deref(), Ok("1"));
        if let Some(private) = std::env::var_os("CASKADE_PRIVATE_TRASH_TEST") {
            let source = PathBuf::from(private).join("source.zip");
            fs::write(&source, b"private fixture source").unwrap();
            let cancel = CancellationToken::new();
            let cleanup = SourceCleanup {
                snapshot: snapshot(&source, &cancel),
                path: source.clone(),
                handling: SourceHandling::Trash,
            };
            let runtime = tokio::runtime::Runtime::new().unwrap();
            assert!(runtime.block_on(cleanup.finish(cancel)).is_none());
            assert!(!source.exists());
            let files =
                PathBuf::from(std::env::var_os("XDG_DATA_HOME").unwrap()).join("Trash/files");
            assert!(
                fs::read_dir(files)
                    .unwrap()
                    .any(|entry| fs::read(entry.unwrap().path()).unwrap()
                        == b"private fixture source")
            );
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let data = temp.path().join("data");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&data).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "install::source::tests::native_trash_uses_an_isolated_os_trash",
                "--ignored",
                "--nocapture",
            ])
            .env("CASKADE_PRIVATE_TRASH_TEST", &home)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data)
            .status()
            .unwrap();
        assert!(status.success());
    }
}
