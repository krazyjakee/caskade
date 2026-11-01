//! What app-data backups (`crate::backup`, #191) read from the live library.

use super::LibraryStore;
use crate::Result;
use std::path::Path;

/// Each artwork file's content hash, and whether every image using it came
/// from IGDB (which a restore can download again).
pub(crate) const ARTWORK_HASHES: &str = "SELECT content_hash, \
     MIN(COALESCE(json_extract(record_json,'$.source'),'') LIKE 'igdb:%') \
     FROM artwork GROUP BY content_hash ORDER BY content_hash";

impl LibraryStore {
    /// Writes a consistent copy of the database to `file`, which must not
    /// exist: one read transaction, so writers carry on meanwhile and the
    /// copy has everything committed before it began, the WAL's included.
    pub(crate) async fn vacuum_into(&self, file: &Path) -> Result<()> {
        let file = file.to_str().ok_or_else(|| {
            crate::Error::Invalid(crate::tr!(
                "the backup folder's path can't be used: {path}",
                path = file.display()
            ))
        })?;
        sqlx::query("VACUUM INTO ?")
            .bind(file)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The database's size in bytes, free pages left out.
    pub(crate) async fn used_bytes(&self) -> Result<u64> {
        let (pages, free, size): (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT page_count FROM pragma_page_count), \
             (SELECT freelist_count FROM pragma_freelist_count), \
             (SELECT page_size FROM pragma_page_size)",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(u64::try_from((pages - free).max(0) * size).unwrap_or(0))
    }

    /// [`ARTWORK_HASHES`] in the live library.
    pub(crate) async fn artwork_hashes(&self) -> Result<Vec<(String, bool)>> {
        Ok(sqlx::query_as(ARTWORK_HASHES).fetch_all(&self.pool).await?)
    }
}
