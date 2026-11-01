//! macOS quarantine (CSK-15): what Caskade downloads gets the
//! `com.apple.quarantine` attribute a browser would give it, and a game
//! installed from a quarantined source carries the source's attribute onto
//! its files, so Gatekeeper checks the game's apps when they first run.
//!
//! Only macOS builds with `macos-formats` (a default feature, for `xattr`)
//! set or read the attribute. A filesystem without extended
//! attributes logs a warning and never fails the install. The value format
//! and the file walk are tested on every host; setting the attribute and
//! Gatekeeper's check of it have not been run on macOS.
use std::{
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const ATTRIBUTE: &str = "com.apple.quarantine";

/// The attribute's value for a file Caskade downloaded at `time`: flags
/// (downloaded, not yet opened), the time in hex seconds, the agent, and
/// no event id.
pub fn value(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("0081;{seconds:08x};Caskade;")
}

/// `path` and, for a folder, everything under it; symbolic links are
/// listed but never followed.
pub fn entries(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = vec![path.to_path_buf()];
    let mut pending = vec![path.to_path_buf()];
    while let Some(dir) = pending.pop() {
        if !std::fs::symlink_metadata(&dir)?.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            found.push(entry.path());
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(found)
}

/// Quarantines a finished download (a file, or a torrent's folder).
pub fn mark_download(path: &Path) {
    #[cfg(all(target_os = "macos", feature = "macos-formats"))]
    {
        let value = value(SystemTime::now());
        if let Err(error) = set_missing(path, value.as_bytes()) {
            tracing::warn!(%error, "download not quarantined");
        }
    }
    #[cfg(not(all(target_os = "macos", feature = "macos-formats")))]
    let _ = path;
}

/// Gives every file under `game` that has none the quarantine of `source`,
/// when the source has one.
pub fn carry(source: &Path, game: &Path) {
    #[cfg(all(target_os = "macos", feature = "macos-formats"))]
    {
        let Ok(Some(value)) = xattr::get(source, ATTRIBUTE) else {
            return;
        };
        if let Err(error) = set_missing(game, &value) {
            tracing::warn!(%error, "installed files not quarantined");
        }
    }
    #[cfg(not(all(target_os = "macos", feature = "macos-formats")))]
    let _ = (source, game);
}

#[cfg(all(target_os = "macos", feature = "macos-formats"))]
fn set_missing(path: &Path, value: &[u8]) -> io::Result<()> {
    for entry in entries(path)? {
        // A link's own attributes don't matter to Gatekeeper, and setting
        // one through it would mark its target.
        if std::fs::symlink_metadata(&entry)?.is_symlink() {
            continue;
        }
        if xattr::get(&entry, ATTRIBUTE)?.is_none() {
            xattr::set(&entry, ATTRIBUTE, value)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_download_is_quarantined_like_a_browsers() {
        let time = UNIX_EPOCH + Duration::from_secs(0x6540_0000);
        assert_eq!(value(time), "0081;65400000;Caskade;");
    }

    #[test]
    fn carrying_reaches_every_file_and_folder_but_never_through_a_link() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("Game");
        std::fs::create_dir_all(game.join("Game.app/Contents/MacOS")).unwrap();
        std::fs::write(game.join("Game.app/Contents/MacOS/Game"), b"bin").unwrap();
        std::fs::write(game.join("readme.txt"), b"hi").unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret"), b"x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, game.join("link")).unwrap();
        let mut found = entries(&game).unwrap();
        found.sort();
        let mut expected = vec![
            game.clone(),
            game.join("Game.app"),
            game.join("Game.app/Contents"),
            game.join("Game.app/Contents/MacOS"),
            game.join("Game.app/Contents/MacOS/Game"),
            game.join("readme.txt"),
        ];
        #[cfg(unix)]
        expected.push(game.join("link"));
        expected.sort();
        assert_eq!(found, expected);
        assert_eq!(
            entries(&game.join("readme.txt")).unwrap(),
            vec![game.join("readme.txt")]
        );
    }
}
