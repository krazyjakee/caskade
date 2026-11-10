//! One process's half of a scenario run by a script in network namespaces
//! (`scripts/lan-unplugged.sh`, `scripts/two-networks.sh`): the other half
//! is another process, and they pass notes through a directory. caskade-share's
//! tests include this file too.

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// This process's device, and notes passed to the other device's process in
/// place of the two people comparing screens.
pub struct Peer {
    pub me: String,
    pub other: String,
    dir: PathBuf,
}

impl Peer {
    pub fn from_env() -> Self {
        let var = |name| std::env::var(name).expect("run by scripts/lan-unplugged.sh");
        Self {
            me: var("CASKADE_LAN_ME"),
            other: var("CASKADE_LAN_OTHER"),
            dir: var("CASKADE_LAN_NOTES").into(),
        }
    }

    pub fn say(&self, step: &str, value: &str) {
        let note = self.dir.join(format!("{}-{step}", self.me));
        let partial = note.with_extension("partial");
        std::fs::write(&partial, value).unwrap();
        std::fs::rename(partial, note).unwrap();
    }

    pub async fn heard(&self, step: &str) -> String {
        self.heard_within(step, 120).await
    }

    pub async fn heard_within(&self, step: &str, seconds: u64) -> String {
        let note = self.dir.join(format!("{}-{step}", self.other));
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            if let Ok(value) = std::fs::read_to_string(&note) {
                return value;
            }
            assert!(Instant::now() < deadline, "{} said {step}", self.other);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
