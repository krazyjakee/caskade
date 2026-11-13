//! Save versions shared with someone else through a link (#248): the
//! sharer reads a version's files, and the recipient keeps them as a new
//! version of the same game here.
//!
//! A received version descends from nothing. When the recipient has saves
//! of their own, that makes two tips, so the next Play asks which to use
//! (as when two devices both played); with none, it is simply their save.
//! It never replaces a save without asking.

use std::path::PathBuf;

use super::{
    MAX_BYTES, MAX_FILES, Manifest, ManifestFile, SaveVersion, SaveVersions, VersionReason,
};
use crate::{Error, Result};

impl SaveVersions {
    /// `version`'s files, each with where its content is kept here; `None`
    /// when one of them hasn't arrived on this device.
    pub fn files(&self, version: &SaveVersion) -> Option<Vec<(ManifestFile, PathBuf)>> {
        self.manifest(&version.content_hash)?
            .files
            .into_iter()
            .map(|file| {
                let path = self.inner.blobs.path(&file.hash)?;
                Some((file, path))
            })
            .collect()
    }

    /// Keeps files someone shared as a new version of `game_id`'s saves, for
    /// the account playing now, and returns it. `files` pairs each file of
    /// the version with where its content arrived. Each file's content must
    /// match its hash. Files outside a named folder (an absolute path on the
    /// sharer's computer) are left out: they have no place here. The same
    /// content already kept for the account returns that version.
    pub async fn add_shared(
        &self,
        game_id: &str,
        files: Vec<(ManifestFile, PathBuf)>,
    ) -> Result<SaveVersion> {
        let files: Vec<(ManifestFile, PathBuf)> = files
            .into_iter()
            .filter(|(file, _)| !file.root.is_empty())
            .collect();
        if files.is_empty() {
            return Err(Error::Invalid(crate::tr!(
                "This save has no files that can go on this device."
            )));
        }
        let size: u64 = files.iter().map(|(file, _)| file.size).sum();
        if files.len() > MAX_FILES || size > MAX_BYTES {
            return Err(Error::Invalid(crate::tr!(
                "This save is too large to keep versions of."
            )));
        }
        let account = self.account();
        let lock = self.lock(game_id);
        let _held = lock.lock().await;

        let blobs = self.inner.blobs.clone();
        let manifest = tokio::task::spawn_blocking(move || {
            let mut kept = Vec::with_capacity(files.len());
            for (file, source) in files {
                let (hash, size) = blobs.put_file(&source)?;
                if hash != file.hash || size != file.size {
                    return Err(Error::Invalid(crate::tr!(
                        "A file of this save arrived damaged. Ask for it again."
                    )));
                }
                kept.push(file);
            }
            kept.sort();
            kept.dedup_by(|a, b| a.root == b.root && a.path == b.path);
            let manifest = Manifest { files: kept };
            blobs.put_bytes(&manifest.bytes())?;
            Ok::<_, Error>(manifest)
        })
        .await??;
        let content = manifest.hash();
        if let Some(known) = self
            .store()
            .save_versions(game_id, Some(&account))
            .await?
            .into_iter()
            .find(|version| version.content_hash == content)
        {
            return Ok(known);
        }
        let version = SaveVersion {
            id: uuid::Uuid::now_v7().to_string(),
            game_id: game_id.to_owned(),
            account_id: account,
            device_id: self.device(),
            session_id: None,
            created_at_ms: crate::now_ms(),
            content_hash: content,
            size_bytes: manifest.size(),
            file_count: u32::try_from(manifest.files.len()).unwrap_or(u32::MAX),
            parents: Vec::new(),
            reason: VersionReason::Shared,
            local: true,
        };
        self.store().insert_save_version(&version).await?;
        self.prune(game_id).await?;
        self.announce(game_id);
        Ok(version)
    }
}
