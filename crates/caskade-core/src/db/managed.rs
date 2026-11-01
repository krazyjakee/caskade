//! Emulators Caskade installed (`managed_emulators`). The service lives in
//! `crate::emulation::setup`.

use super::LibraryStore;
use crate::{Result, emulation::ManagedEmulator};

type Row = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    Option<String>,
    Option<String>,
);

const SELECT: &str = "SELECT emulator_id,definition_id,source,dir,installed,file,latest,\
     latest_url,checked_at_ms,skipped,error FROM managed_emulators";

fn read(row: Row) -> ManagedEmulator {
    let (
        emulator_id,
        definition_id,
        source,
        dir,
        installed,
        file,
        latest,
        latest_url,
        checked_at_ms,
        skipped,
        error,
    ) = row;
    ManagedEmulator {
        emulator_id,
        definition_id,
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
    pub async fn managed_emulators(&self) -> Result<Vec<ManagedEmulator>> {
        let rows = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!(
            "{SELECT} ORDER BY definition_id"
        )))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(read).collect())
    }

    pub async fn managed_emulator(&self, emulator_id: &str) -> Result<Option<ManagedEmulator>> {
        let row =
            sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!("{SELECT} WHERE emulator_id=?")))
                .bind(emulator_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(read))
    }

    pub async fn upsert_managed_emulator(&self, managed: &ManagedEmulator) -> Result<()> {
        sqlx::query(
            "INSERT INTO managed_emulators(emulator_id,definition_id,source,dir,installed,file,\
             latest,latest_url,checked_at_ms,skipped,error) VALUES(?,?,?,?,?,?,?,?,?,?,?) \
             ON CONFLICT(emulator_id) DO UPDATE SET definition_id=excluded.definition_id,\
             source=excluded.source,dir=excluded.dir,installed=excluded.installed,\
             file=excluded.file,latest=excluded.latest,latest_url=excluded.latest_url,\
             checked_at_ms=excluded.checked_at_ms,skipped=excluded.skipped,error=excluded.error",
        )
        .bind(&managed.emulator_id)
        .bind(&managed.definition_id)
        .bind(&managed.source)
        .bind(&managed.dir)
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
}
