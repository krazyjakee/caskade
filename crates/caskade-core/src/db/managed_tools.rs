//! Helper tools Caskade installed (`managed_tools`). The service lives in
//! `crate::managed`.

use super::LibraryStore;
use crate::{Result, managed::ManagedTool};

type Row = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    Option<String>,
    Option<String>,
);

const SELECT: &str = "SELECT tool_id,source,dir,install_json,installed,file,latest,latest_url,\
     checked_at_ms,skipped,error FROM managed_tools";

fn read(row: Row) -> ManagedTool {
    let (
        tool_id,
        source,
        dir,
        install,
        installed,
        file,
        latest,
        latest_url,
        checked_at_ms,
        skipped,
        error,
    ) = row;
    ManagedTool {
        install: install.and_then(|json| {
            serde_json::from_str(&json)
                .inspect_err(
                    |error| tracing::warn!(tool = tool_id, %error, "tool install unreadable"),
                )
                .ok()
        }),
        tool_id,
        source,
        dir,
        installed,
        file,
        latest,
        latest_url,
        checked_at_ms,
        skipped,
        error,
    }
}

impl LibraryStore {
    pub async fn managed_tools(&self) -> Result<Vec<ManagedTool>> {
        let rows =
            sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!("{SELECT} ORDER BY tool_id")))
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(read).collect())
    }

    pub async fn managed_tool(&self, tool_id: &str) -> Result<Option<ManagedTool>> {
        let row =
            sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!("{SELECT} WHERE tool_id=?")))
                .bind(tool_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(read))
    }

    pub async fn upsert_managed_tool(&self, managed: &ManagedTool) -> Result<()> {
        let install = managed
            .install
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        sqlx::query(
            "INSERT INTO managed_tools(tool_id,source,dir,install_json,installed,file,latest,\
             latest_url,checked_at_ms,skipped,error) VALUES(?,?,?,?,?,?,?,?,?,?,?) \
             ON CONFLICT(tool_id) DO UPDATE SET source=excluded.source,dir=excluded.dir,\
             install_json=excluded.install_json,installed=excluded.installed,\
             file=excluded.file,latest=excluded.latest,latest_url=excluded.latest_url,\
             checked_at_ms=excluded.checked_at_ms,skipped=excluded.skipped,error=excluded.error",
        )
        .bind(&managed.tool_id)
        .bind(&managed.source)
        .bind(&managed.dir)
        .bind(install)
        .bind(&managed.installed)
        .bind(&managed.file)
        .bind(&managed.latest)
        .bind(&managed.latest_url)
        .bind(managed.checked_at_ms)
        .bind(&managed.skipped)
        .bind(&managed.error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_managed_tool(&self, tool_id: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM managed_tools WHERE tool_id=?")
            .bind(tool_id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }
}
