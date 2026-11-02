use super::*;
use crate::{
    model::{GameEntry, PlaySession, StorageRoot},
    sessions::SessionRepository,
};
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct MemoryStore {
    pub entries: Mutex<BTreeMap<String, GameEntry>>,
    pub sessions: Mutex<BTreeMap<String, PlaySession>>,
}
#[async_trait]
impl SessionRepository for MemoryStore {
    async fn entry(&self, id: &str) -> Result<Option<GameEntry>> {
        Ok(self.entries.lock().unwrap().get(id).cloned())
    }
    async fn root(&self, _id: &str) -> Result<Option<StorageRoot>> {
        Ok(None)
    }
    async fn reserve(&self, session: &PlaySession) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions
            .values()
            .any(|s| s.game_id == session.game_id && s.ended_at_ms.is_none())
        {
            return Err(LaunchError::AlreadyRunning);
        }
        sessions.insert(session.id.clone(), session.clone());
        Ok(())
    }
    async fn activate(&self, session: &PlaySession) -> Result<()> {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.id.clone(), session.clone());
        Ok(())
    }
    async fn heartbeat(&self, session: &PlaySession) -> Result<()> {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.id.clone(), session.clone());
        Ok(())
    }
    async fn finish(&self, session: &PlaySession) -> Result<()> {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.id.clone(), session.clone());
        Ok(())
    }
    async fn in_flight(&self) -> Result<Vec<PlaySession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.ended_at_ms.is_none())
            .cloned()
            .collect())
    }
}
#[derive(Default)]
pub struct Events(pub Mutex<Vec<(String, LaunchEvent)>>);
#[async_trait]
impl LaunchHooks for Events {
    async fn stream_starting(&self, launch: &LaunchTicket) {
        self.0.lock().unwrap().push((
            "stream.starting".into(),
            LaunchEvent {
                command: None,
                game_id: launch.game_id.clone(),
                entry_id: launch.entry_id.clone(),
                session_id: launch.session_id.clone(),
                started_at_ms: 0,
                ended_at_ms: None,
                duration_ms: 0,
                status: SessionStatus::Running,
            },
        ));
    }
    async fn started(&self, event: &LaunchEvent) {
        self.0
            .lock()
            .unwrap()
            .push(("game.started".into(), event.clone()));
    }
    async fn exited(&self, event: &LaunchEvent) {
        self.0
            .lock()
            .unwrap()
            .push(("game.exited".into(), event.clone()));
    }
    async fn after(&self, event: &LaunchEvent) {
        self.0
            .lock()
            .unwrap()
            .push(("launch.after".into(), event.clone()));
    }
}
pub fn fast_config() -> LaunchConfig {
    LaunchConfig {
        poll_interval: std::time::Duration::from_millis(20),
        launch_timeout: std::time::Duration::from_millis(500),
        exit_grace: std::time::Duration::from_millis(60),
        heartbeat_interval: std::time::Duration::from_millis(20),
    }
}
pub fn entry(
    program: std::path::PathBuf,
    args: Vec<String>,
    location: Option<std::path::PathBuf>,
) -> GameEntry {
    let mut entry = GameEntry::new("game", caskade_discovery::SourceId::new("fixture"), "entry");
    entry.state = caskade_discovery::InstallState::Installed;
    entry.location = location.map(|path| crate::model::InstallLocation::External { path });
    entry.launch = Some(crate::model::LaunchSpec::Executable {
        path: program,
        args,
        cwd: None,
        env: BTreeMap::new(),
    });
    entry
}
pub fn service(store: Arc<MemoryStore>, events: Arc<Events>) -> LaunchService {
    LaunchService::from_repository(store)
        .with_hooks(events)
        .with_config(fast_config())
        .with_aliases(None)
}
