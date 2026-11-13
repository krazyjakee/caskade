//! The main window's place and size across starts: saved as it moves and
//! resizes, restored at the next start. A maximised window keeps the size
//! it had before, so unmaximising goes back to it.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The smallest window a restore gives back, in physical pixels: a saved
/// size below it (a broken file, a window shrunk to nothing) isn't used.
pub const MIN_WIDTH: u32 = 640;
pub const MIN_HEIGHT: u32 = 400;

/// The window's last normal (not maximised) place and size, in physical
/// pixels, and whether it was maximised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl WindowState {
    /// The saved state in `file`; `None` without one, or when it can't be
    /// used.
    pub fn load(file: &Path) -> Option<Self> {
        let state: Self = crate::state_file::load(file)?;
        (state.width >= MIN_WIDTH && state.height >= MIN_HEIGHT).then_some(state)
    }

    /// Writes the state to `file`, replacing it whole.
    pub fn save(&self, file: &Path) {
        crate::state_file::save(file, self);
    }

    /// The state after the window moved or resized: a maximised window
    /// keeps the normal place and size from before.
    pub fn followed(
        previous: Option<Self>,
        position: (i32, i32),
        size: (u32, u32),
        maximized: bool,
    ) -> Self {
        match previous {
            Some(previous) if maximized => Self {
                maximized: true,
                ..previous
            },
            _ => Self {
                x: position.0,
                y: position.1,
                width: size.0,
                height: size.1,
                maximized,
            },
        }
    }
}

/// Where the main window's state is kept.
pub fn file(paths: &caskade_core::AppPaths) -> PathBuf {
    paths.config.join("native-window.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_state_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("native-window.json");
        let state = WindowState {
            x: -1200,
            y: 40,
            width: 1600,
            height: 900,
            maximized: true,
        };
        state.save(&file);
        assert_eq!(WindowState::load(&file), Some(state));
    }

    #[test]
    fn a_missing_broken_or_tiny_state_is_not_used() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("native-window.json");
        assert_eq!(WindowState::load(&file), None);
        std::fs::write(&file, "{not json").unwrap();
        assert_eq!(WindowState::load(&file), None);
        WindowState {
            x: 0,
            y: 0,
            width: 12,
            height: 9,
            maximized: false,
        }
        .save(&file);
        assert_eq!(WindowState::load(&file), None);
    }

    #[test]
    fn maximising_keeps_the_size_to_go_back_to() {
        let normal = WindowState::followed(None, (100, 50), (1280, 800), false);
        let maximised = WindowState::followed(Some(normal), (0, 0), (2560, 1400), true);
        assert_eq!(
            maximised,
            WindowState {
                maximized: true,
                ..normal
            }
        );
        let back = WindowState::followed(Some(maximised), (100, 50), (1280, 800), false);
        assert_eq!(back, normal);
    }
}
