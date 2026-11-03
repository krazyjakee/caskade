//! Paired devices (#226): migration 0031.

use super::LibraryStore;
use crate::Result;

/// A row of `paired_devices`. Its grants are credentials: never logged,
/// never sent to the webview.
#[derive(Clone, PartialEq, Eq)]
pub struct PairedDeviceRecord {
    pub endpoint_id: String,
    pub name: String,
    pub kind: String,
    pub os: String,
    pub addresses: Vec<String>,
    pub wol_macs: Vec<String>,
    pub last_sync_ms: Option<i64>,
    pub grant_in: String,
    pub grant_out: String,
    pub paired_at_ms: i64,
    pub forgotten_at_ms: Option<i64>,
}

impl std::fmt::Debug for PairedDeviceRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairedDeviceRecord")
            .field("endpoint_id", &self.endpoint_id)
            .field("name", &self.name)
            .field("paired_at_ms", &self.paired_at_ms)
            .field("forgotten_at_ms", &self.forgotten_at_ms)
            .finish_non_exhaustive()
    }
}

type Row = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<i64>,
    String,
    String,
    i64,
    Option<i64>,
);

fn record(row: Row) -> PairedDeviceRecord {
    let list = |text: String| serde_json::from_str::<Vec<String>>(&text).unwrap_or_default();
    PairedDeviceRecord {
        endpoint_id: row.0,
        name: row.1,
        kind: row.2,
        os: row.3,
        addresses: list(row.4),
        wol_macs: list(row.5),
        last_sync_ms: row.6,
        grant_in: row.7,
        grant_out: row.8,
        paired_at_ms: row.9,
        forgotten_at_ms: row.10,
    }
}

impl LibraryStore {
    /// Every paired device, forgotten ones still to be told included.
    pub async fn paired_devices(&self) -> Result<Vec<PairedDeviceRecord>> {
        let rows = sqlx::query_as::<_, Row>(
            "SELECT endpoint_id, name, kind, os, addresses_json, wol_macs_json, last_sync_ms,
                    grant_in, grant_out, paired_at_ms, forgotten_at_ms
             FROM paired_devices ORDER BY paired_at_ms, endpoint_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(record).collect())
    }

    /// Saves a pairing, replacing an earlier one with the same device (a
    /// forgotten one included).
    pub async fn save_paired_device(&self, device: &PairedDeviceRecord) -> Result<()> {
        sqlx::query(
            "INSERT INTO paired_devices(endpoint_id, name, kind, os, addresses_json, wol_macs_json,
                 last_sync_ms, grant_in, grant_out, paired_at_ms, forgotten_at_ms)
             VALUES(?,?,?,?,?,?,?,?,?,?,?)
             ON CONFLICT(endpoint_id) DO UPDATE SET name=excluded.name, kind=excluded.kind,
                 os=excluded.os, addresses_json=excluded.addresses_json,
                 wol_macs_json=excluded.wol_macs_json, last_sync_ms=excluded.last_sync_ms,
                 grant_in=excluded.grant_in, grant_out=excluded.grant_out,
                 paired_at_ms=excluded.paired_at_ms, forgotten_at_ms=excluded.forgotten_at_ms",
        )
        .bind(&device.endpoint_id)
        .bind(&device.name)
        .bind(&device.kind)
        .bind(&device.os)
        .bind(serde_json::to_string(&device.addresses)?)
        .bind(serde_json::to_string(&device.wol_macs)?)
        .bind(device.last_sync_ms)
        .bind(&device.grant_in)
        .bind(&device.grant_out)
        .bind(device.paired_at_ms)
        .bind(device.forgotten_at_ms)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Marks a device forgotten; it stays only until it's been told.
    pub async fn forget_paired_device(&self, endpoint_id: &str, at_ms: i64) -> Result<bool> {
        Ok(sqlx::query(
            "UPDATE paired_devices SET forgotten_at_ms=? WHERE endpoint_id=? AND forgotten_at_ms IS NULL",
        )
        .bind(at_ms)
        .bind(endpoint_id)
        .execute(&self.pool)
        .await?
        .rows_affected()
            > 0)
    }

    pub async fn delete_paired_device(&self, endpoint_id: &str) -> Result<bool> {
        Ok(
            sqlx::query("DELETE FROM paired_devices WHERE endpoint_id=?")
                .bind(endpoint_id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }

    /// Records when library sync with a device last finished.
    pub async fn set_paired_device_synced(&self, endpoint_id: &str, at_ms: i64) -> Result<()> {
        sqlx::query("UPDATE paired_devices SET last_sync_ms=? WHERE endpoint_id=?")
            .bind(at_ms)
            .bind(endpoint_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// A paired device's Wake-on-LAN addresses, as it last said (#234).
    pub async fn paired_device_macs(&self, endpoint_id: &str) -> Result<Vec<String>> {
        let text: Option<String> =
            sqlx::query_scalar("SELECT wol_macs_json FROM paired_devices WHERE endpoint_id=?")
                .bind(endpoint_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(text
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default())
    }

    /// Records a paired device's Wake-on-LAN addresses (#234).
    pub async fn set_paired_device_macs(&self, endpoint_id: &str, macs: &[String]) -> Result<()> {
        sqlx::query("UPDATE paired_devices SET wol_macs_json=? WHERE endpoint_id=?")
            .bind(serde_json::to_string(macs)?)
            .bind(endpoint_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Records where a paired device was reached.
    pub async fn set_paired_device_addresses(
        &self,
        endpoint_id: &str,
        addresses: &[String],
    ) -> Result<()> {
        sqlx::query("UPDATE paired_devices SET addresses_json=? WHERE endpoint_id=?")
            .bind(serde_json::to_string(addresses)?)
            .bind(endpoint_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
