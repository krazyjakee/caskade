//! Plugin state (#123): enabled flags, granted permissions
//! and each plugin's bounded key-value store. The plugin host owns what the
//! grants mean; the store keeps them as JSON.

use super::LibraryStore;
use crate::{Error, Result, now_ms};

/// The longest key a plugin may store.
const PLUGIN_KEY_MAX: usize = 256;
/// The largest value a plugin may store under one key.
const PLUGIN_VALUE_MAX: usize = 64 * 1024;
/// The most a plugin may store in total.
const PLUGIN_KV_TOTAL_MAX: usize = 4 * 1024 * 1024;

/// One installed plugin's persisted state.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginRow {
    pub id: String,
    pub enabled: bool,
    /// The permissions the user granted, as the host serialises them.
    pub grants: serde_json::Value,
}

fn plugin_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 128 {
        return Err(Error::Invalid(crate::tr!("invalid plugin id")));
    }
    Ok(())
}

impl LibraryStore {
    pub async fn plugin_state(&self, id: &str) -> Result<Option<PluginRow>> {
        let row = sqlx::query_as::<_, (String, bool, String)>(
            "SELECT id,enabled,grants_json FROM plugins WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(id, enabled, grants)| {
            Ok(PluginRow {
                id,
                enabled,
                grants: serde_json::from_str(&grants)?,
            })
        })
        .transpose()
    }

    pub async fn plugin_states(&self) -> Result<Vec<PluginRow>> {
        sqlx::query_as::<_, (String, bool, String)>(
            "SELECT id,enabled,grants_json FROM plugins ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(id, enabled, grants)| {
            Ok(PluginRow {
                id,
                enabled,
                grants: serde_json::from_str(&grants)?,
            })
        })
        .collect()
    }

    pub async fn upsert_plugin_state(&self, row: &PluginRow) -> Result<()> {
        plugin_id(&row.id)?;
        sqlx::query("INSERT INTO plugins(id,enabled,grants_json,updated_at_ms) VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET enabled=excluded.enabled,grants_json=excluded.grants_json,updated_at_ms=excluded.updated_at_ms")
            .bind(&row.id)
            .bind(row.enabled)
            .bind(serde_json::to_string(&row.grants)?)
            .bind(now_ms())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Forgets a plugin: its state and everything it stored.
    pub async fn delete_plugin_state(&self, id: &str) -> Result<bool> {
        let mut tx = self.begin_write().await?;
        sqlx::query("DELETE FROM plugin_kv WHERE plugin_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let deleted = sqlx::query("DELETE FROM plugins WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
        tx.commit().await?;
        Ok(deleted)
    }

    pub async fn plugin_kv_get(&self, plugin: &str, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(
            sqlx::query_scalar("SELECT value FROM plugin_kv WHERE plugin_id=? AND key=?")
                .bind(plugin)
                .bind(key)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Stores `value` under `key`, within the per-key and per-plugin bounds.
    pub async fn plugin_kv_set(&self, plugin: &str, key: &str, value: &[u8]) -> Result<()> {
        plugin_id(plugin)?;
        if key.is_empty() || key.len() > PLUGIN_KEY_MAX {
            return Err(Error::Invalid(crate::tr!(
                "plugin keys are 1–{max} bytes",
                max = PLUGIN_KEY_MAX
            )));
        }
        if value.len() > PLUGIN_VALUE_MAX {
            return Err(Error::Invalid(crate::tr!(
                "plugin values are at most {max} bytes",
                max = PLUGIN_VALUE_MAX
            )));
        }
        let mut tx = self.begin_write().await?;
        let others: i64 = sqlx::query_scalar(
            "SELECT coalesce(sum(length(key)+length(value)),0) FROM plugin_kv WHERE plugin_id=? AND key<>?",
        )
        .bind(plugin)
        .bind(key)
        .fetch_one(&mut *tx)
        .await?;
        let total = usize::try_from(others).unwrap_or(usize::MAX) + key.len() + value.len();
        if total > PLUGIN_KV_TOTAL_MAX {
            return Err(Error::Invalid(crate::tr!(
                "a plugin may store at most {max} bytes",
                max = PLUGIN_KV_TOTAL_MAX
            )));
        }
        sqlx::query("INSERT INTO plugin_kv(plugin_id,key,value) VALUES(?,?,?) ON CONFLICT(plugin_id,key) DO UPDATE SET value=excluded.value")
            .bind(plugin)
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn plugin_kv_delete(&self, plugin: &str, key: &str) -> Result<bool> {
        Ok(
            sqlx::query("DELETE FROM plugin_kv WHERE plugin_id=? AND key=?")
                .bind(plugin)
                .bind(key)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plugin_state_and_kv_persist_across_a_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("library.db");
        {
            let store = LibraryStore::open(&path).await.unwrap();
            store
                .upsert_plugin_state(&PluginRow {
                    id: "dev.example.hello".into(),
                    enabled: true,
                    grants: serde_json::json!({"network": ["example.com"]}),
                })
                .await
                .unwrap();
            store
                .plugin_kv_set("dev.example.hello", "greeting", b"hi")
                .await
                .unwrap();
            store.close().await;
        }
        let store = LibraryStore::open(&path).await.unwrap();
        let state = store
            .plugin_state("dev.example.hello")
            .await
            .unwrap()
            .unwrap();
        assert!(state.enabled);
        assert_eq!(state.grants["network"][0], "example.com");
        assert_eq!(
            store
                .plugin_kv_get("dev.example.hello", "greeting")
                .await
                .unwrap(),
            Some(b"hi".to_vec())
        );
        store
            .upsert_plugin_state(&PluginRow {
                enabled: false,
                ..state
            })
            .await
            .unwrap();
        assert_eq!(store.plugin_states().await.unwrap().len(), 1);
        assert!(
            !store
                .plugin_state("dev.example.hello")
                .await
                .unwrap()
                .unwrap()
                .enabled
        );
        assert!(
            store
                .plugin_kv_delete("dev.example.hello", "greeting")
                .await
                .unwrap()
        );
        assert!(
            store
                .delete_plugin_state("dev.example.hello")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn plugin_storage_is_bounded() {
        let store = LibraryStore::in_memory().await.unwrap();
        let id = "dev.example.big";
        assert!(store.plugin_kv_set(id, "", b"x").await.is_err());
        assert!(
            store
                .plugin_kv_set(id, &"k".repeat(257), b"x")
                .await
                .is_err()
        );
        assert!(
            store
                .plugin_kv_set(id, "k", &vec![0; PLUGIN_VALUE_MAX + 1])
                .await
                .is_err()
        );
        let value = vec![0; PLUGIN_VALUE_MAX];
        for index in 0..(PLUGIN_KV_TOTAL_MAX / PLUGIN_VALUE_MAX - 1) {
            store
                .plugin_kv_set(id, &format!("k{index}"), &value)
                .await
                .unwrap();
        }
        assert!(store.plugin_kv_set(id, "one-more", &value).await.is_err());
        // Replacing a value counts its new size, not both.
        store.plugin_kv_set(id, "k0", b"small").await.unwrap();
        store.plugin_kv_set(id, "one-more", &value).await.unwrap();
    }
}
