// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Before any thread exists, so none keeps an arena of its own.
    caskade_malloc::limit_arenas();
    caskade_malloc::trim_every(std::time::Duration::from_secs(60));
    caskade_desktop::run();
}
