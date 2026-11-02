use super::{LocationKind, SavePreferences};
use crate::{Error, Result, db::LibraryStore};
use std::path::Path;

impl LibraryStore {
    pub async fn save_preferences(&self, entry_id: &str) -> Result<(u32, SavePreferences)> {
        let row: Option<(i64, String)> = sqlx::query_as(
            "SELECT revision,record_json FROM release_save_preferences WHERE entry_id=?",
        )
        .bind(entry_id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some((revision, json)) => Ok((
                u32::try_from(revision)
                    .map_err(|_| Error::Invalid("invalid save preferences revision".into()))?,
                serde_json::from_str(&json)?,
            )),
            None => Ok((0, SavePreferences::default())),
        }
    }

    /// Compare-and-set, so two editors cannot silently overwrite each other.
    pub async fn set_save_preferences(
        &self,
        entry_id: &str,
        preferences: &SavePreferences,
        revision: u32,
    ) -> Result<u32> {
        if self.get_entry(entry_id).await?.is_none() {
            return Err(crate::db::missing("entry", entry_id));
        }
        validate(preferences)?;
        let next = revision
            .checked_add(1)
            .ok_or_else(|| Error::Invalid("save preferences revision exhausted".into()))?;
        let json = serde_json::to_string(preferences)?;
        let saved: Option<i64> = if revision == 0 {
            sqlx::query_scalar(
                "INSERT INTO release_save_preferences(entry_id,revision,record_json) VALUES(?,?,?) \
                 ON CONFLICT(entry_id) DO NOTHING RETURNING revision",
            )
            .bind(entry_id)
            .bind(i64::from(next))
            .bind(&json)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_scalar(
                "UPDATE release_save_preferences SET revision=?,record_json=? WHERE entry_id=? AND revision=? RETURNING revision",
            ).bind(i64::from(next)).bind(&json).bind(entry_id).bind(i64::from(revision)).fetch_optional(&self.pool).await?
        };
        if saved.is_some() {
            return Ok(next);
        }
        Err(Error::Invalid(crate::tr!(
            "Save settings changed elsewhere. Reload them and try again."
        )))
    }
}

fn validate(preferences: &SavePreferences) -> Result<()> {
    if preferences
        .manifest_title
        .as_ref()
        .is_some_and(|title| title.trim().is_empty() || title.len() > 1024 || title.contains('\0'))
    {
        return Err(Error::Invalid(crate::tr!(
            "The manifest game name is invalid."
        )));
    }
    if let Some(locations) = &preferences.locations {
        if locations.len() > 256 {
            return Err(Error::Invalid(crate::tr!(
                "Choose at most 256 save locations."
            )));
        }
        for location in locations {
            if location.path.is_empty()
                || location.path.len() > 4096
                || location.path.contains('\0')
            {
                return Err(Error::Invalid(crate::tr!("The save location is invalid.")));
            }
            if location.kind == LocationKind::Registry {
                if super::registry::parse_key(&location.path).is_none() {
                    return Err(Error::Invalid(crate::tr!(
                        "Registry save locations must start with HKEY_CURRENT_USER or HKEY_LOCAL_MACHINE."
                    )));
                }
            } else if !Path::new(&location.path).is_absolute() {
                return Err(Error::Invalid(crate::tr!(
                    "Choose an absolute file or folder path for saves."
                )));
            }
        }
    }
    Ok(())
}
