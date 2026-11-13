//! The small JSON files the app keeps its own state in (the window's place,
//! the views to come back to, the last account).

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::Path;

/// What `file` holds; `None` without one, or when it isn't readable.
pub fn load<T: DeserializeOwned>(file: &Path) -> Option<T> {
    let text = std::fs::read_to_string(file).ok()?;
    serde_json::from_str(&text)
        .inspect_err(
            |error| tracing::warn!(%error, file = %file.display(), "a state file isn't readable"),
        )
        .ok()
}

/// Writes `value` to `file`, beside it and then moved over: never half a
/// file.
pub fn save(file: &Path, value: &impl Serialize) {
    let Ok(text) = serde_json::to_string(value) else {
        return;
    };
    let partial = file.with_extension("json.partial");
    if let Err(error) =
        std::fs::write(&partial, text).and_then(|()| std::fs::rename(&partial, file))
    {
        let _ = std::fs::remove_file(&partial);
        tracing::warn!(%error, file = %file.display(), "a state file wasn't saved");
    }
}
