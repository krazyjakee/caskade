//! BIOS files from RomM servers' firmware: a
//! platform's missing files, found by name or MD5 on every server whose
//! platform maps to it, the same mapping RomM syncs use.

use super::RommService;
use crate::emulation::{Bios, BiosFuture, BiosProvider};
use std::path::Path;

impl BiosProvider for RommService {
    fn fetch<'a>(&'a self, platform: &'a str, files: &'a [Bios], dir: &'a Path) -> BiosFuture<'a> {
        Box::pin(async move {
            self.fetch_bios(platform, files, dir)
                .await
                .map_err(|error| crate::Error::Invalid(error.to_string()))
        })
    }
}

#[cfg(feature = "net")]
impl RommService {
    async fn fetch_bios(
        &self,
        platform: &str,
        files: &[Bios],
        dir: &Path,
    ) -> super::Result<Vec<String>> {
        let settings = self.settings().await?;
        let mut fetched: Vec<String> = Vec::new();
        for server in &settings.servers {
            let wanted: Vec<&Bios> = files
                .iter()
                .filter(|bios| !fetched.contains(&bios.file))
                .collect();
            if wanted.is_empty() {
                break;
            }
            if self.offline(&server.id) {
                continue;
            }
            match self.fetch_from(server, platform, &wanted, dir).await {
                Ok(names) => fetched.extend(names),
                Err(error) => {
                    tracing::warn!(server = server.id, platform, %error, "BIOS files from RomM failed");
                }
            }
        }
        Ok(fetched)
    }

    async fn fetch_from(
        &self,
        server: &super::settings::RommServerConfig,
        platform: &str,
        wanted: &[&Bios],
        dir: &Path,
    ) -> super::Result<Vec<String>> {
        use md5::Digest;
        let catalog = self.inner.emulation.catalog();
        let client = self.client(server, None).await?;
        let mut names = Vec::new();
        for remote in client.platforms().await? {
            let mapped = server
                .mapping(&remote.slug)
                .and_then(|mapping| mapping.platform.clone())
                .or_else(|| super::sync::suggest(&catalog, &remote));
            if mapped.as_deref() != Some(platform) {
                continue;
            }
            let firmware = client.firmware(remote.id).await?;
            for bios in wanted {
                if names.contains(&bios.file) {
                    continue;
                }
                let name = bios.file.rsplit('/').next().unwrap_or(&bios.file);
                let same_md5 = |hash: &Option<String>| {
                    bios.md5
                        .as_ref()
                        .zip(hash.as_ref())
                        .is_some_and(|(want, have)| want.eq_ignore_ascii_case(have))
                };
                let found = firmware
                    .iter()
                    .filter(|file| !file.missing_from_fs)
                    .filter(|file| {
                        // A dump the definition doesn't name.
                        bios.md5.is_none() || file.md5_hash.is_none() || same_md5(&file.md5_hash)
                    })
                    .find(|file| file.file_name.eq_ignore_ascii_case(name))
                    .or_else(|| {
                        firmware
                            .iter()
                            .filter(|file| !file.missing_from_fs)
                            .find(|file| same_md5(&file.md5_hash))
                    });
                let Some(found) = found else {
                    continue;
                };
                let bytes = client.firmware_content(found).await?;
                if let Some(want) = &bios.md5 {
                    let have = format!("{:x}", md5::Md5::digest(&bytes));
                    if !have.eq_ignore_ascii_case(want) {
                        tracing::warn!(
                            file = bios.file,
                            "RomM's BIOS file isn't the dump expected"
                        );
                        continue;
                    }
                }
                let target = dir.join(&bios.file);
                if let Some(parent) = target.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                write_atomically(&target, &bytes).await?;
                tracing::info!(file = bios.file, platform, "BIOS file fetched from RomM");
                names.push(bios.file.clone());
            }
        }
        Ok(names)
    }
}

/// Writes `bytes` beside `target` and renames them into place, so a crash
/// or a full disk never leaves a short file that later counts as present.
#[cfg(feature = "net")]
async fn write_atomically(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut name = target.file_name().unwrap_or_default().to_owned();
    name.push(".caskade-part");
    let part = target.with_file_name(name);
    let written = async {
        let mut file = tokio::fs::File::create(&part).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&part, target).await
    }
    .await;
    if written.is_err() {
        let _ = tokio::fs::remove_file(&part).await;
    }
    written
}

#[cfg(not(feature = "net"))]
impl RommService {
    async fn fetch_bios(
        &self,
        _platform: &str,
        _files: &[Bios],
        _dir: &Path,
    ) -> super::Result<Vec<String>> {
        Ok(Vec::new())
    }
}
