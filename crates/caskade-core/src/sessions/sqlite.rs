use super::SessionRepository;
use crate::{
    db::LibraryStore,
    launch::{LaunchError, Result},
    model::{GameEntry, PlaySession, StorageRoot},
};
use async_trait::async_trait;
fn error(e: crate::Error) -> LaunchError {
    if let crate::Error::Db(sqlx::Error::Database(db)) = &e
        && db.is_unique_violation()
    {
        return LaunchError::AlreadyRunning;
    }
    LaunchError::Store(e.to_string())
}
#[async_trait]
impl SessionRepository for LibraryStore {
    async fn entry(&self, id: &str) -> Result<Option<GameEntry>> {
        self.get_entry(id).await.map_err(error)
    }
    async fn root(&self, id: &str) -> Result<Option<StorageRoot>> {
        self.get_root(id).await.map_err(error)
    }
    async fn reserve(&self, s: &PlaySession) -> Result<()> {
        self.start_session(s).await.map(|_| ()).map_err(error)
    }
    async fn activate(&self, s: &PlaySession) -> Result<()> {
        self.activate_session(
            &s.id,
            s.started_at_ms,
            s.tracking.clone().unwrap_or(serde_json::Value::Null),
        )
        .await
        .map(|_| ())
        .map_err(error)
    }
    async fn heartbeat(&self, s: &PlaySession) -> Result<()> {
        self.heartbeat_session(&s.id, s.heartbeat_at_ms, s.duration_ms, s.tracking.clone())
            .await
            .map(|_| ())
            .map_err(error)
    }
    async fn finish(&self, s: &PlaySession) -> Result<()> {
        self.finish_session(
            &s.id,
            s.ended_at_ms.unwrap_or(s.heartbeat_at_ms),
            s.duration_ms,
        )
        .await
        .map(|_| ())
        .map_err(error)
    }
    async fn in_flight(&self) -> Result<Vec<PlaySession>> {
        self.in_flight_sessions().await.map_err(error)
    }
    async fn actions(&self, entry_id: &str) -> Result<Vec<crate::model::GameAction>> {
        self.game_actions(entry_id).await.map_err(error)
    }
    async fn action_settings(&self, entry_id: &str) -> Result<crate::model::EntryActionSettings> {
        self.entry_action_settings(entry_id).await.map_err(error)
    }
    async fn scripts(
        &self,
        game_id: &str,
        profile_id: Option<&str>,
    ) -> Result<crate::scripts::ScriptSet> {
        self.scripts_for_launch(Some(game_id), profile_id)
            .await
            .map_err(error)
    }
    async fn game_title(&self, game_id: &str) -> Result<Option<String>> {
        Ok(self
            .get_game(game_id)
            .await
            .map_err(error)?
            .map(|details| details.game.title))
    }
}
