use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use caskade_installer::{
    InstallOutcome,
    events::{EventSink, JobEvent, Phase},
};
use serde::Serialize;
use serde_json::Value;

use super::CliResult;

pub struct Output {
    pub value: Value,
    pub human: String,
    pub code: u8,
}

impl Output {
    pub fn new(value: impl Serialize, human: String) -> CliResult<Self> {
        Ok(Self {
            value: serde_json::to_value(value)?,
            human,
            code: 0,
        })
    }
    pub fn outcome(outcome: InstallOutcome) -> CliResult<Self> {
        let (human, code) = match &outcome {
            InstallOutcome::Installed(game) => {
                let mut text = format!("Installed {} in {}", game.title, game.dir.display());
                for warning in &game.warnings {
                    text.push_str(&format!("\nWarning: {}", warning.message));
                }
                (text, 0)
            }
            InstallOutcome::HandOff(caskade_installer::HandOff::Rom { platform, files }) => (
                format!(
                    "ROM for {platform}: {} file(s) ready for emulation import",
                    files.len()
                ),
                0,
            ),
            InstallOutcome::HandOff(caskade_installer::HandOff::Delegated {
                launch_uri, ..
            }) => (format!("Installed by system service: {launch_uri}"), 0),
            InstallOutcome::HandOff(caskade_installer::HandOff::Package {
                package,
                program,
                ..
            }) => (
                match program {
                    Some(program) => format!(
                        "Installed package {package} by the package manager; runs {}",
                        program.display()
                    ),
                    None => format!(
                        "Installed package {package} by the package manager; it has no command"
                    ),
                },
                0,
            ),
            InstallOutcome::NeedsUser(question) => (format!("Needs a decision: {question:?}"), 2),
            InstallOutcome::Unsupported(reason) => (format!("Unsupported input: {reason:?}"), 3),
        };
        Ok(Self {
            value: serde_json::to_value(outcome)?,
            human,
            code,
        })
    }
}

pub struct Progress {
    pub enabled: bool,
    state: Mutex<Option<(Instant, Phase)>>,
}
impl Progress {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            state: Mutex::new(None),
        }
    }
}
impl EventSink for Progress {
    fn emit(&self, event: JobEvent) {
        if !self.enabled {
            return;
        }
        let mut state = self.state.lock().expect("CLI progress poisoned");
        if event.message == "progress"
            && state.as_ref().is_some_and(|(last, phase)| {
                *phase == event.phase && last.elapsed() < Duration::from_millis(200)
            })
        {
            return;
        }
        *state = Some((Instant::now(), event.phase));
        let phase = match event.phase {
            Phase::Download => "download",
            Phase::Install => "install",
            _ => "job",
        };
        let step = event
            .step
            .map(|step| format!(" step {}", step + 1))
            .unwrap_or_default();
        let eta = event
            .eta_secs
            .map(|eta| format!(", ETA {eta}s"))
            .unwrap_or_default();
        eprintln!(
            "{phase}{step}: {} — {} bytes, {} files{eta}",
            event.message, event.bytes, event.files
        );
    }
}
