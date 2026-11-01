use crate::{
    launch::Result,
    model::{GameEntry, PlaySession, StorageRoot},
};
use async_trait::async_trait;

/// Durable boundary: reserve must atomically reject a second open session for
/// the same game. DB uniqueness, rather than only a process-local lock, makes
/// this invariant survive concurrent requests and app restarts.
#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn entry(&self, id: &str) -> Result<Option<GameEntry>>;
    async fn root(&self, id: &str) -> Result<Option<StorageRoot>>;
    async fn reserve(&self, session: &PlaySession) -> Result<()>;
    async fn activate(&self, session: &PlaySession) -> Result<()>;
    async fn heartbeat(&self, session: &PlaySession) -> Result<()>;
    async fn finish(&self, session: &PlaySession) -> Result<()>;
    async fn in_flight(&self) -> Result<Vec<PlaySession>>;
    /// The launch actions the user added to an entry (#177).
    async fn actions(&self, _entry_id: &str) -> Result<Vec<crate::model::GameAction>> {
        Ok(vec![])
    }
    async fn action_settings(&self, _entry_id: &str) -> Result<crate::model::EntryActionSettings> {
        Ok(Default::default())
    }
    /// The lifecycle scripts a launch of the game runs with (#179).
    async fn scripts(
        &self,
        _game_id: &str,
        _profile_id: Option<&str>,
    ) -> Result<crate::scripts::ScriptSet> {
        Ok(Default::default())
    }
    async fn game_title(&self, _game_id: &str) -> Result<Option<String>> {
        Ok(None)
    }
}
