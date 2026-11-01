//! GameMode (#133): a `launch.before` interceptor that runs games through
//! `gamemoderun`. It needs the `process` permission to change what runs, and
//! read-only access to the usual bin folders to see whether GameMode is
//! installed; without it, launches are left alone.
use caskade_plugin_sdk::{Error, Intercept, Level, Plugin, host, json};
use serde::Deserialize;

/// Where distributions install `gamemoderun` (the manifest's `fs` grants).
const SEARCH: [&str; 3] = ["/usr/bin", "/usr/games", "/usr/local/bin"];

#[derive(Deserialize)]
#[serde(default)]
struct Settings {
    enabled: bool,
    /// A specific `gamemoderun`; empty searches [`SEARCH`].
    command: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            command: String::new(),
        }
    }
}

/// The `gamemoderun` to use, if GameMode is installed.
fn gamemoderun(settings: &Settings) -> Option<String> {
    if !settings.command.is_empty() {
        return Some(settings.command.clone());
    }
    SEARCH
        .iter()
        .map(|dir| format!("{dir}/gamemoderun"))
        .find(|path| std::fs::metadata(path).is_ok_and(|meta| meta.is_file()))
}

struct GameMode;

impl Plugin for GameMode {
    fn intercept(point: &str, payload: &str) -> Result<Intercept, Error> {
        if point != "launch.before" {
            return Ok(Intercept::Continue(payload.to_owned()));
        }
        let settings: Settings = host::settings()?.unwrap_or_default();
        let mut command: serde_json::Value = json::from_str(payload)?;
        let program = command["program"].as_str().unwrap_or_default().to_owned();
        let wrapper = match gamemoderun(&settings) {
            Some(wrapper) if settings.enabled && program != wrapper => wrapper,
            _ => return Ok(Intercept::Continue(payload.to_owned())),
        };
        // gamemoderun <program> <args…>: it execs the game, so the launched
        // process stays the one Caskade tracks.
        let args = command["args"].as_array().cloned().unwrap_or_default();
        command["args"] = std::iter::once(serde_json::Value::String(program))
            .chain(args)
            .collect();
        command["program"] = serde_json::Value::String(wrapper.clone());
        host::log(Level::Info, &format!("running the game through {wrapper}"));
        Ok(Intercept::Continue(json::to_string(&command)?))
    }
}

caskade_plugin_sdk::export!(GameMode);
