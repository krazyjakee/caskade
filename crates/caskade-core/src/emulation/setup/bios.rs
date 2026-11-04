//! BIOS files from somewhere the user keeps them (EMULATOR-SETUP-PLAN §5):
//! after an install, and from the Platforms section, the platform's
//! missing BIOS files are fetched into the folder its emulator reads.

use super::EmulatorSetup;
use crate::{
    Error, Result,
    emulation::{Bios, BiosState, bios_dirs},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{future::Future, path::Path, pin::Pin};

pub type BiosFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>>;

/// Somewhere BIOS files can come from: a RomM server's firmware.
pub trait BiosProvider: Send + Sync {
    /// Writes the files of `files` it has for `platform` into `dir` (a
    /// file's name may be a relative path) and returns their names. A file
    /// whose MD5 the definition gives and the source's doesn't match is
    /// left out.
    fn fetch<'a>(&'a self, platform: &'a str, files: &'a [Bios], dir: &'a Path) -> BiosFuture<'a>;
}

/// What a BIOS fetch did for a platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct BiosFetch {
    pub platform_id: String,
    /// Files fetched.
    pub fetched: Vec<String>,
    /// Required files still missing.
    pub missing: Vec<String>,
    /// Where they go.
    pub dir: Option<String>,
}

impl EmulatorSetup {
    /// Fetches the platform's missing BIOS files for its default emulator.
    pub async fn fetch_bios(&self, platform: &str) -> Result<BiosFetch> {
        let emulation = &self.inner.emulation;
        let view = emulation
            .platforms()
            .await?
            .into_iter()
            .find(|view| view.platform.id == platform)
            .ok_or_else(|| Error::Invalid(crate::tr!("Caskade doesn't know this platform.")))?;
        let default = view.settings.default.clone().ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "{platform} has no emulator yet",
                platform = view.platform.name
            ))
        })?;
        let emulator = self
            .inner
            .store
            .get_emulator(&default.emulator_id)
            .await?
            .ok_or_else(|| Error::Invalid(crate::tr!("the default emulator is gone")))?;
        let catalog = emulation.catalog();
        let vars = emulator
            .definition
            .as_deref()
            .and_then(|id| catalog.emulator(id))
            .map(|definition| definition.vars.clone())
            .unwrap_or_default();
        let dir = bios_dirs(
            view.settings.bios_dir.as_deref(),
            &vars,
            emulation.os(),
            emulator.install.kind(),
            &self.context(&emulator.install),
        )
        .into_iter()
        .next();
        let lacking = |view: &crate::emulation::PlatformView| -> Vec<Bios> {
            view.platform
                .bios
                .iter()
                .filter(|entry| entry.state != Some(BiosState::Found))
                .filter_map(|entry| {
                    let bios = catalog
                        .platform(platform)?
                        .bios
                        .iter()
                        .find(|bios| bios.file == entry.file)?;
                    Some(bios.clone())
                })
                .collect()
        };
        let wanted = lacking(&view);
        let provider = crate::lock(&self.inner.bios).clone();
        let fetched = match (&dir, provider) {
            (Some(dir), Some(provider)) if !wanted.is_empty() => {
                tokio::fs::create_dir_all(dir).await?;
                provider.fetch(platform, &wanted, dir).await?
            }
            _ => Vec::new(),
        };
        #[cfg(feature = "net")]
        let fetched = match &dir {
            Some(dir) => self.bios_nearby(&wanted, dir, fetched).await,
            None => fetched,
        };
        if !fetched.is_empty() {
            self.changed();
        }
        // Checked again: the files fetched may not be all it needs.
        let after = emulation
            .platforms()
            .await?
            .into_iter()
            .find(|view| view.platform.id == platform);
        let missing = after
            .as_ref()
            .map(|view| {
                view.platform
                    .bios
                    .iter()
                    .filter(|entry| entry.required && entry.state != Some(BiosState::Found))
                    .map(|entry| entry.file.clone())
                    .collect()
            })
            .unwrap_or_default();
        Ok(BiosFetch {
            platform_id: platform.to_owned(),
            fetched,
            missing,
            dir: dir.map(|dir| dir.to_string_lossy().into_owned()),
        })
    }
}

#[cfg(feature = "net")]
impl EmulatorSetup {
    /// BIOS files through devices nearby (#277), by the MD5 the definition
    /// gives: what the provider fetched is kept for them, and what it
    /// couldn't (or with no provider, offline) is asked of them, checked
    /// against that MD5. Returns `fetched` with what arrived.
    async fn bios_nearby(
        &self,
        wanted: &[Bios],
        dir: &Path,
        mut fetched: Vec<String>,
    ) -> Vec<String> {
        let Some(near) = crate::lock(&self.inner.near).clone() else {
            return fetched;
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        for bios in wanted {
            let Some(md5) = bios.md5.as_deref().map(str::to_ascii_lowercase) else {
                continue;
            };
            let Ok(url) = url::Url::parse(&format!("caskade-bios:md5/{md5}")) else {
                continue;
            };
            let path = dir.join(&bios.file);
            if fetched.contains(&bios.file) {
                near.keep_path(&url, &path, None).await;
                continue;
            }
            if !near.fetch_to(&url, &path, &cancel, &|_, _| {}).await {
                continue;
            }
            let file = path.clone();
            let matches = tokio::task::spawn_blocking(move || {
                use md5::Digest;
                let bytes = std::fs::read(&file).ok()?;
                let digest = md5::Md5::digest(&bytes);
                Some(
                    digest
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>(),
                )
            })
            .await
            .ok()
            .flatten()
            .is_some_and(|digest| digest == md5);
            if matches {
                fetched.push(bios.file.clone());
            } else {
                let _ = tokio::fs::remove_file(&path).await;
            }
        }
        fetched
    }
}
