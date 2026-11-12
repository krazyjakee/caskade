//! The account that signed in last on this device, so the picker starts on
//! it (the webview keeps it in local storage). Kept in
//! `native-accounts.json` in the config folder; for the run only without
//! one (fixtures mode, tests). An account id is no secret.

use std::path::PathBuf;
use std::sync::Mutex;

/// The run's memory, where there's no file.
static LAST: Mutex<Option<String>> = Mutex::new(None);

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    last_account: Option<String>,
}

fn file() -> Option<PathBuf> {
    crate::shell::services().map(|services| services.paths.config.join("native-accounts.json"))
}

/// The account that signed in last here.
pub fn last_account() -> Option<String> {
    match file() {
        Some(file) => std::fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str::<Saved>(&text).ok())
            .and_then(|saved| saved.last_account),
        None => LAST.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    }
}

/// Remembers `id` as the account that signed in last here.
pub fn remember(id: &str) {
    match file() {
        Some(file) => {
            let saved = Saved {
                last_account: Some(id.to_owned()),
            };
            let Ok(text) = serde_json::to_string(&saved) else {
                return;
            };
            // Written beside, then moved over: never half a file.
            let partial = file.with_extension("json.partial");
            if std::fs::write(&partial, text).is_ok() && std::fs::rename(&partial, &file).is_err() {
                let _ = std::fs::remove_file(&partial);
            }
        }
        None => *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.to_owned()),
    }
}
