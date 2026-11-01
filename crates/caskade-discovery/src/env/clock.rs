use std::time::{Duration, Instant, SystemTime};

/// Wall time describes launcher data; monotonic time measures scan budgets and
/// must never move backwards. Both are injected so fixtures are reproducible.
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
    fn monotonic(&self) -> Duration;
}

#[derive(Debug)]
pub struct LiveClock {
    origin: Instant,
}
impl Default for LiveClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}
impl Clock for LiveClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
    fn monotonic(&self) -> Duration {
        self.origin.elapsed()
    }
}
