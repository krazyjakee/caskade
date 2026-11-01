//! A ROM's artwork: its cover during syncs, its screenshots when the
//! game's page first opens.
#![cfg_attr(not(feature = "net"), allow(dead_code, unused_imports))]

use super::{Result, RommRom, RommService, keyed, source_id};
use crate::{bus::Event, model::ArtworkKind};

impl RommService {
    /// Whether the game lacks some of the ROM's `kind` artwork (its cover
    /// or screenshots): not imported yet, or an earlier import failed.
    #[cfg(feature = "net")]
    pub(super) async fn missing(&self, record: &RommRom, kind: ArtworkKind) -> Result<bool> {
        if self.inner.artwork.is_none() {
            return Ok(false);
        }
        let source = source_id(&record.server_id);
        let ours = self
            .inner
            .store
            .artwork_for_game(&record.game_id)
            .await?
            .iter()
            .filter(|art| art.source == source.as_str() && art.kind == kind)
            .count();
        let rom = &record.rom;
        Ok(match kind {
            ArtworkKind::Cover => rom.cover().is_some() && ours == 0,
            _ => ours < rom.screenshots().count(),
        })
    }

    /// Downloads the screenshots of the game's ROMs that it lacks, the
    /// first time its page opens (and again while some fail). Whether any
    /// came.
    #[cfg(feature = "net")]
    pub async fn page_images(&self, game_id: &str) -> Result<bool> {
        let store = &self.inner.store;
        let mut changed = false;
        for entry in store.entries_for_game(game_id).await? {
            if super::server_of(&entry).is_none() {
                continue;
            }
            let Some(record) = store.romm_rom_by_entry(&entry.id).await? else {
                continue;
            };
            // One download per ROM, however often the page opens.
            let guard = keyed(&self.inner.page_images, &record.entry_id);
            let _guard = guard.lock().await;
            if !self.missing(&record, ArtworkKind::Screenshot).await? {
                continue;
            }
            // An offline server would only time out; the next open tries.
            if self.offline(&record.server_id) {
                continue;
            }
            let Ok(config) = self.server_config(&record.server_id).await else {
                continue;
            };
            let client = self.client(&config, None).await?;
            changed |= self.import_screenshots(&client, &record).await;
        }
        if changed {
            self.inner.bus.emit(Event::LibraryGameUpdated {
                game_id: game_id.to_owned(),
            });
        }
        Ok(changed)
    }

    #[cfg(not(feature = "net"))]
    pub async fn page_images(&self, _game_id: &str) -> Result<bool> {
        Ok(false)
    }

    /// The ROM's cover (on the server, or its IGDB URL) as the game's
    /// cover; a failure only logs.
    #[cfg(feature = "net")]
    pub(super) async fn import_cover(&self, client: &super::client::Client, record: &RommRom) {
        let (Some(artwork), Some(location)) = (&self.inner.artwork, record.rom.cover()) else {
            return;
        };
        let rom = record.rom.id;
        let cached = match fetch(client, artwork, location).await {
            Ok(cached) => cached,
            Err(error) => {
                tracing::warn!(rom, %error, "RomM cover not imported");
                return;
            }
        };
        if let Err(error) = self
            .replace_artwork(record, ArtworkKind::Cover, &[cached])
            .await
        {
            tracing::warn!(rom, %error, "RomM cover not saved");
        }
    }

    /// The ROM's screenshots as the game's, downloaded side by side (there
    /// are at most four). One that fails is left out, to be tried again
    /// when the page next opens; when all fail, the old ones stay. Whether
    /// any were saved.
    #[cfg(feature = "net")]
    async fn import_screenshots(&self, client: &super::client::Client, record: &RommRom) -> bool {
        let Some(artwork) = &self.inner.artwork else {
            return false;
        };
        let rom = record.rom.id;
        let mut downloads = tokio::task::JoinSet::new();
        for (index, location) in record.rom.screenshots().enumerate() {
            let (client, artwork, location) =
                (client.clone(), artwork.clone(), location.to_owned());
            downloads.spawn(async move { (index, fetch(&client, &artwork, &location).await) });
        }
        let mut done = Vec::new();
        let mut failed = false;
        while let Some(joined) = downloads.join_next().await {
            match joined {
                Ok((index, Ok(image))) => done.push((index, image)),
                Ok((_, Err(error))) => {
                    tracing::warn!(rom, %error, "RomM screenshot not imported");
                    failed = true;
                }
                Err(error) => {
                    tracing::warn!(rom, %error, "RomM screenshot download stopped");
                    failed = true;
                }
            }
        }
        // In the server's order.
        done.sort_by_key(|(index, _)| *index);
        let cached: Vec<_> = done.into_iter().map(|(_, image)| image).collect();
        if failed && cached.is_empty() {
            return false;
        }
        match self
            .replace_artwork(record, ArtworkKind::Screenshot, &cached)
            .await
        {
            Ok(()) => !cached.is_empty(),
            Err(error) => {
                tracing::warn!(rom, %error, "RomM screenshots not saved");
                false
            }
        }
    }

    /// Replaces the game's `kind` artwork from this server with `images`,
    /// in their order. IGDB's of that kind give way to them.
    #[cfg(feature = "net")]
    pub(super) async fn replace_artwork(
        &self,
        record: &RommRom,
        kind: ArtworkKind,
        images: &[crate::artwork::CachedArtwork],
    ) -> Result<()> {
        let source = source_id(&record.server_id);
        let store = &self.inner.store;
        for old in store.artwork_for_game(&record.game_id).await? {
            let replaced = old.source == source.as_str()
                || (!images.is_empty() && old.source.starts_with(crate::metadata::ARTWORK_SOURCE));
            if old.kind == kind && replaced {
                store.delete_artwork(&old.id).await?;
            }
        }
        for image in images {
            store
                .upsert_artwork(&image.record(&record.game_id, kind, source.as_str()))
                .await?;
        }
        Ok(())
    }
}

/// An image from the server (a path) or the web (a URL) into the artwork
/// cache. A URL elsewhere than the server must lead to a public address.
#[cfg(feature = "net")]
async fn fetch(
    client: &super::client::Client,
    artwork: &crate::artwork::ArtworkCache,
    location: &str,
) -> std::result::Result<crate::artwork::CachedArtwork, String> {
    if location.starts_with("https://") || location.starts_with("http://") {
        let on_server = url::Url::parse(location).is_ok_and(|url| client.same_origin(&url));
        let downloaded = if on_server {
            artwork.download(location).await
        } else {
            artwork.download_metadata(location).await
        };
        return downloaded.map_err(|e| e.to_string());
    }
    let bytes = client.resource(location).await.map_err(|e| e.to_string())?;
    artwork.import_bytes(bytes).await.map_err(|e| e.to_string())
}
