//! The game window's size (WEB-GAMES-PLAN §3): a game whose size is known
//! opens at it, centred over Caskade's window, rather than covering it. A
//! window the user resized opens at their size next time.

use std::path::Path;

use caskade_core::{
    launch::WindowPlacement,
    web::{GameSize, bundle::Bundle},
};

/// The share of the screen a game may take before it's shrunk to fit.
const MOST_OF_THE_SCREEN: f64 = 0.9;

/// The file in the game's data folder (`--data-dir`, deleted with the game)
/// that keeps the size the user last gave its window.
const REMEMBERED: &str = "caskade-window.json";

/// How far, in logical pixels, a window may end up from the size it was
/// asked to open at before it counts as resized: GTK rounds sizes to whole
/// logical pixels.
const RESIZE_SLACK: f64 = 2.0;

/// The size, in logical pixels, the user last gave the window of the game
/// whose data folder is `data_dir`.
pub fn remembered(data_dir: Option<&Path>) -> Option<GameSize> {
    let text = std::fs::read(data_dir?.join(REMEMBERED)).ok()?;
    let size: GameSize = serde_json::from_slice(&text).ok()?;
    GameSize::new(size.width, size.height)
}

/// Keeps `size` for the next time the game opens.
pub fn remember(data_dir: &Path, size: GameSize) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let partial = data_dir.join(format!("{REMEMBERED}.partial"));
    std::fs::write(
        &partial,
        serde_json::to_vec(&size).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(partial, data_dir.join(REMEMBERED))
}

/// The size to remember, in logical pixels, for a window asked to open at
/// `asked` that is `now` (both physical, at `scale`): `None` unless the user
/// resized it.
pub fn resized(asked: (u32, u32), now: (u32, u32), scale: f64) -> Option<GameSize> {
    let logical = |physical: u32| f64::from(physical) / scale;
    let moved = |a: u32, b: u32| (logical(a) - logical(b)).abs() > RESIZE_SLACK;
    if !moved(asked.0, now.0) && !moved(asked.1, now.1) {
        return None;
    }
    GameSize::new(logical(now.0).round() as u32, logical(now.1).round() as u32)
}

/// How big the game is: as the launch says, else as its offline copy
/// keeps it, else a Flash movie's stage.
pub fn game_size(
    asked: Option<GameSize>,
    bundle: Option<&Bundle>,
    start: &str,
) -> Option<GameSize> {
    asked.or_else(|| bundle?.size()).or_else(|| {
        let start = url::Url::parse(start).ok()?;
        if !caskade_core::web::is_swf(&start) {
            return None;
        }
        let file = std::fs::File::open(bundle?.get(&start)?.path).ok()?;
        caskade_core::web::swf::stage_size(std::io::BufReader::new(file))
    })
}

/// Where the window goes and how big its inside is, in physical pixels:
/// `game` at the screen's `scale`, with `extra` logical pixels above it
/// (the capture bar), shrunk to fit `screen` and centred over `over`
/// (Caskade's window) or the screen.
pub fn place(
    game: GameSize,
    extra: f64,
    scale: f64,
    screen: WindowPlacement,
    over: Option<WindowPlacement>,
) -> WindowPlacement {
    let (width, height) = (
        f64::from(game.width) * scale,
        (f64::from(game.height) + extra) * scale,
    );
    let fit = (f64::from(screen.width) * MOST_OF_THE_SCREEN / width)
        .min(f64::from(screen.height) * MOST_OF_THE_SCREEN / height)
        .min(1.0);
    let (width, height) = ((width * fit).round() as u32, (height * fit).round() as u32);
    let around = over.unwrap_or(screen);
    let centre = |start: i32, span: u32, size: u32| {
        start.saturating_add(((i64::from(span) - i64::from(size)) / 2) as i32)
    };
    // On the screen, even when Caskade's window is partly off it.
    let clamp = |at: i32, start: i32, span: u32, size: u32| {
        at.min(start.saturating_add(span.saturating_sub(size) as i32))
            .max(start)
    };
    WindowPlacement {
        x: clamp(
            centre(around.x, around.width, width),
            screen.x,
            screen.width,
            width,
        ),
        y: clamp(
            centre(around.y, around.height, height),
            screen.y,
            screen.height,
            height,
        ),
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i32, y: i32, width: u32, height: u32) -> WindowPlacement {
        WindowPlacement {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_game_opens_at_its_size_over_caskade() {
        let screen = at(0, 0, 2560, 1440);
        let game = GameSize::new(640, 480).unwrap();
        assert_eq!(
            place(game, 0.0, 1.0, screen, Some(at(100, 100, 1600, 1000))),
            at(580, 360, 640, 480)
        );
        assert_eq!(
            place(game, 46.0, 2.0, screen, None),
            at(640, 194, 1280, 1052),
            "scaled, with the bar"
        );
    }

    #[test]
    fn only_a_resized_window_is_remembered() {
        assert_eq!(resized((1280, 960), (1280, 960), 1.0), None);
        assert_eq!(
            resized((2560, 1920), (2562, 1918), 2.0),
            None,
            "GTK's rounding"
        );
        assert_eq!(
            resized((2560, 1920), (1600, 1200), 2.0),
            GameSize::new(800, 600),
            "logical pixels"
        );
        assert_eq!(
            resized((1280, 960), (100, 100), 1.0),
            None,
            "too small for a game"
        );
    }

    #[test]
    fn the_remembered_size_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(remembered(Some(dir.path())), None);
        assert_eq!(remembered(None), None);
        let data = dir.path().join("entry");
        let size = GameSize::new(800, 600).unwrap();
        remember(&data, size).unwrap();
        assert_eq!(remembered(Some(&data)), Some(size));
        std::fs::write(data.join(REMEMBERED), "{").unwrap();
        assert_eq!(remembered(Some(&data)), None, "unreadable");
    }

    #[test]
    fn a_big_game_shrinks_to_fit_and_stays_on_screen() {
        let screen = at(1920, 0, 1920, 1080);
        let game = GameSize::new(1920, 1440).unwrap();
        assert_eq!(
            place(game, 0.0, 1.0, screen, Some(at(3000, 500, 800, 600))),
            at(2544, 108, 1296, 972)
        );
    }
}
