//! The app's commands as plain async functions (#86): every shell calls
//! these, the Tauri adapter through one-line `#[tauri::command]`
//! forwarders. Each takes the services it uses, so a shell passes what it
//! holds and a test what it builds. Who may call which is
//! [`crate::permissions`]'; the shell checks before calling.

pub mod accounts;
