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
        Some(file) => crate::state_file::load::<Saved>(&file).and_then(|saved| saved.last_account),
        None => caskade_core::lock(&LAST).clone(),
    }
}

/// Remembers `id` as the account that signed in last here.
pub fn remember(id: &str) {
    match file() {
        Some(file) => crate::state_file::save(
            &file,
            &Saved {
                last_account: Some(id.to_owned()),
            },
        ),
        None => *caskade_core::lock(&LAST) = Some(id.to_owned()),
    }
}
