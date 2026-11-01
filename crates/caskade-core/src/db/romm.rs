//! RomM ROMs in the library (#1): the remote ROM behind each `romm:<server>`
//! entry and its download cache. The service lives in `crate::romm`.

use super::{LibraryStore, decode};
use crate::{Result, model::validation as v, romm::RommRom};

impl LibraryStore {
    /// Every ROM of `server`, by RomM id.
    pub async fn romm_roms(&self, server: &str) -> Result<Vec<RommRom>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM romm_roms WHERE server_id=? ORDER BY rom_id",
        )
        .bind(server)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }

    pub async fn romm_rom(&self, server: &str, rom_id: u32) -> Result<Option<RommRom>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM romm_roms WHERE server_id=? AND rom_id=?",
        )
        .bind(server)
        .bind(i64::from(rom_id))
        .fetch_optional(&self.pool)
        .await?
        .map(decode)
        .transpose()
    }

    /// The RomM ROM a library entry stands for.
    pub async fn romm_rom_by_entry(&self, entry_id: &str) -> Result<Option<RommRom>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM romm_roms WHERE entry_id=? ORDER BY server_id,rom_id LIMIT 1",
        )
        .bind(entry_id)
        .fetch_optional(&self.pool)
        .await?
        .map(decode)
        .transpose()
    }

    /// Every downloaded ROM, least recently used first.
    pub async fn romm_cached(&self) -> Result<Vec<RommRom>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM romm_roms WHERE cached=1 \
             ORDER BY coalesce(last_used_ms,0),server_id,rom_id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }

    pub async fn save_romm_rom(&self, rom: &RommRom) -> Result<()> {
        v::id(&rom.server_id)?;
        v::id(&rom.entry_id)?;
        let last_used = rom.cache.as_ref().map(|cache| cache.last_used_ms);
        sqlx::query("INSERT INTO romm_roms(server_id,rom_id,entry_id,cached,last_used_ms,record_json) VALUES(?,?,?,?,?,?) ON CONFLICT(server_id,rom_id) DO UPDATE SET entry_id=excluded.entry_id,cached=excluded.cached,last_used_ms=excluded.last_used_ms,record_json=excluded.record_json")
            .bind(&rom.server_id)
            .bind(i64::from(rom.rom_id))
            .bind(&rom.entry_id)
            .bind(rom.cache.is_some())
            .bind(last_used)
            .bind(v::json(rom)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_romm_rom(&self, server: &str, rom_id: u32) -> Result<bool> {
        Ok(
            sqlx::query("DELETE FROM romm_roms WHERE server_id=? AND rom_id=?")
                .bind(server)
                .bind(i64::from(rom_id))
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }
}
