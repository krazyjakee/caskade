//! Caskade's own windows for web games (`web_player`) and videos
//! (`video_player`): tao and wry with no IPC, each in a process of its own,
//! which an app shell starts by running its executable again with
//! `--web-game` or `--video`. The Tauri app and the native app both
//! dispatch those arguments here before they start, so neither shell's
//! event loop has to host a webview. `recovery` is what every Caskade
//! webview does when its processes fail.

pub mod recovery;
pub mod video_player;
pub mod web_player;
