//! ROM scanner state and review choices (#119, EMULATION-PLAN §6.1). The
//! scanners themselves live with the emulator configuration.

use super::{LibraryStore, decode, records::escape_like};
use crate::{
    Result,
    emulation::{ROMS_SOURCE, RomOverride},
    model::{GameEntry, validation as v},
};
use std::collections::BTreeMap;

/// When a scanner last ran and how many games it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanState {
    pub last_scan_ms: i64,
    pub games: u32,
}

impl LibraryStore {
    pub async fn scan_states(&self) -> Result<BTreeMap<String, ScanState>> {
        let rows = sqlx::query_as::<_, (String, i64, i64)>(
            "SELECT scanner_id,last_scan_ms,games FROM rom_scan_state",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, last_scan_ms, games)| {
                let games = u32::try_from(games).unwrap_or(u32::MAX);
                (
                    id,
                    ScanState {
                        last_scan_ms,
                        games,
                    },
                )
            })
            .collect())
    }

    pub async fn set_scan_state(&self, scanner_id: &str, state: ScanState) -> Result<()> {
        sqlx::query("INSERT INTO rom_scan_state(scanner_id,last_scan_ms,games) VALUES(?,?,?) ON CONFLICT(scanner_id) DO UPDATE SET last_scan_ms=excluded.last_scan_ms,games=excluded.games")
            .bind(scanner_id)
            .bind(state.last_scan_ms)
            .bind(i64::from(state.games))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Review choices by ROM path relative to the scanner folder.
    pub async fn rom_overrides(&self, scanner_id: &str) -> Result<BTreeMap<String, RomOverride>> {
        sqlx::query_as::<_, (String, String)>(
            "SELECT path,record_json FROM rom_overrides WHERE scanner_id=? ORDER BY path",
        )
        .bind(scanner_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(path, record)| Ok((path, decode(record)?)))
        .collect()
    }

    /// Replaces the choices for these paths in one transaction; an empty
    /// override removes the path's row.
    pub async fn set_rom_overrides(
        &self,
        scanner_id: &str,
        overrides: &[(String, RomOverride)],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for (path, choice) in overrides {
            v::text(path, "ROM path", false, 4096)?;
            if choice.is_empty() {
                sqlx::query("DELETE FROM rom_overrides WHERE scanner_id=? AND path=?")
                    .bind(scanner_id)
                    .bind(path)
                    .execute(&mut *tx)
                    .await?;
            } else {
                sqlx::query("INSERT INTO rom_overrides(scanner_id,path,record_json) VALUES(?,?,?) ON CONFLICT(scanner_id,path) DO UPDATE SET record_json=excluded.record_json")
                    .bind(scanner_id)
                    .bind(path)
                    .bind(v::json(choice)?)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    /// The library entries a scanner imported; their keys start with its id.
    pub async fn entries_for_scanner(&self, scanner_id: &str) -> Result<Vec<GameEntry>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM game_entries WHERE source_id=? AND external_id LIKE ? ESCAPE '\\' ORDER BY external_id,id",
        )
        .bind(ROMS_SOURCE.as_str())
        .bind(format!("{}/%", escape_like(scanner_id)))
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }
}
