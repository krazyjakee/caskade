//! Shared deterministic host seams for source and orchestrator tests (#65).
use crate::env::{Clock, DriveProvider, Volume};
use std::{
    io,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone)]
pub struct FakeClock {
    state: Arc<Mutex<(SystemTime, Duration)>>,
}
impl Default for FakeClock {
    fn default() -> Self {
        Self::new(UNIX_EPOCH)
    }
}
impl FakeClock {
    pub fn new(now: SystemTime) -> Self {
        Self {
            state: Arc::new(Mutex::new((now, Duration::ZERO))),
        }
    }
    pub fn advance(&self, duration: Duration) {
        let mut state = self.state.lock().expect("fake clock poisoned");
        state.0 = state
            .0
            .checked_add(duration)
            .expect("fake wall clock overflow");
        state.1 = state.1.saturating_add(duration);
    }
    /// Wall clock corrections do not change monotonic scan time.
    pub fn set_wall_time(&self, now: SystemTime) {
        self.state.lock().expect("fake clock poisoned").0 = now;
    }
}
impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        self.state.lock().expect("fake clock poisoned").0
    }
    fn monotonic(&self) -> Duration {
        self.state.lock().expect("fake clock poisoned").1
    }
}

#[derive(Debug, Default, Clone)]
pub struct FakeDrives {
    volumes: Vec<Volume>,
    error: Option<(io::ErrorKind, String)>,
}
impl FakeDrives {
    pub fn new(volumes: Vec<Volume>) -> Self {
        Self {
            volumes,
            error: None,
        }
    }
    pub fn failing(kind: io::ErrorKind, message: impl Into<String>) -> Self {
        Self {
            volumes: vec![],
            error: Some((kind, message.into())),
        }
    }
}
impl DriveProvider for FakeDrives {
    fn volumes(&self) -> io::Result<Vec<Volume>> {
        match &self.error {
            Some((kind, message)) => Err(io::Error::new(*kind, message.clone())),
            None => Ok(self.volumes.clone()),
        }
    }
}
