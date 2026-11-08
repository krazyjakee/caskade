//! Web games' launches: Caskade's own executable in
//! game-window mode, one process per game, so sessions, Stop and recovery
//! work as for any other game.

use super::{LaunchCommand, LaunchError, PreparedLaunch, Result};
use crate::{
    model::{GameEntry, InstallLocation, LaunchSpec, StorageRoot},
    settings::SettingsService,
    tracking::{TrackingMode, TrackingTarget},
    web::GameSize,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use url::Url;

/// The game-window argument `main` dispatches on, before the app starts.
pub const WEB_GAME_ARG: &str = "--web-game";
/// The folder the game's webview keeps its data in (saves, storage).
pub const DATA_DIR_ARG: &str = "--data-dir";
/// Start fullscreen.
pub const FULLSCREEN_ARG: &str = "--fullscreen";
/// Play the offline bundle in this folder.
pub const BUNDLE_ARG: &str = "--bundle";
/// Save the game into the `--bundle` folder: the guided capture.
pub const CAPTURE_ARG: &str = "--capture";
/// Where the window opens: `x,y,width,height`, physical pixels.
pub const GEOMETRY_ARG: &str = "--geometry";
/// How big the game is: `widthxheight`, CSS pixels. A capture keeps it.
pub const SIZE_ARG: &str = "--size";
/// WebKitGTK keeps `SharedArrayBuffer` off without it; Godot 4's threaded
/// exports need it (research/web-capture-spike.md).
const SHARED_ARRAY_BUFFER: (&str, &str) = ("JSC_useSharedArrayBuffer", "1");

/// Where a window is on screen, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl WindowPlacement {
    /// As [`GEOMETRY_ARG`]'s value.
    pub fn arg(&self) -> String {
        format!("{},{},{},{}", self.x, self.y, self.width, self.height)
    }

    /// [`WindowPlacement::arg`]'s text back; `None` for anything else or an
    /// empty size.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split(',');
        let placement = Self {
            x: parts.next()?.trim().parse().ok()?,
            y: parts.next()?.trim().parse().ok()?,
            width: parts.next()?.trim().parse().ok()?,
            height: parts.next()?.trim().parse().ok()?,
        };
        (parts.next().is_none() && placement.width > 0 && placement.height > 0).then_some(placement)
    }
}

/// Where Caskade's window is now, when it is on screen.
type Placement = Arc<dyn Fn() -> Option<WindowPlacement> + Send + Sync>;

/// Starts web games in the game window.
#[derive(Clone)]
pub struct WebPlayer {
    program: PathBuf,
    data_root: PathBuf,
    settings: Option<SettingsService>,
    placement: Option<Placement>,
}

impl WebPlayer {
    /// `program` is Caskade's executable; each game keeps its webview data
    /// in `data_root/<entry id>`.
    pub fn new(program: impl Into<PathBuf>, data_root: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            data_root: data_root.into(),
            settings: None,
            placement: None,
        }
    }

    /// Outside console mode the window opens over Caskade's, where
    /// `placement` says Caskade's is.
    pub fn with_placement(
        mut self,
        placement: impl Fn() -> Option<WindowPlacement> + Send + Sync + 'static,
    ) -> Self {
        self.placement = Some(Arc::new(placement));
        self
    }

    /// Plays fullscreen in console mode (`core.console.active`).
    pub fn with_settings(mut self, settings: SettingsService) -> Self {
        self.settings = Some(settings);
        self
    }

    /// The game window for `entry`, with its offline bundle when it has one
    /// (a managed location in `root`). Its own process is the game: the
    /// session ends the moment the window closes, not when WebKit's helper
    /// processes do (a busy web process can outlive it by up to 10 s).
    pub async fn resolve(
        &self,
        entry: &GameEntry,
        root: Option<StorageRoot>,
    ) -> Result<PreparedLaunch> {
        let Some(LaunchSpec::Web { url }) = &entry.launch else {
            return Err(LaunchError::Invalid("not a web game".into()));
        };
        let url =
            crate::web::page_url(url).map_err(|error| LaunchError::Invalid(error.to_string()))?;
        let mut command = self.command(entry, &url)?;
        if matches!(entry.location, Some(InstallLocation::Managed { .. })) {
            let entry = entry.clone();
            let (bundle, _) = tokio::task::spawn_blocking(move || {
                super::resolve::locations(&entry, root.as_ref(), None)
            })
            .await
            .map_err(|error| LaunchError::Invalid(error.to_string()))??;
            let bundle =
                bundle.ok_or_else(|| LaunchError::Invalid("web game bundle not found".into()))?;
            command
                .args
                .extend([BUNDLE_ARG.into(), utf8(&bundle)?.into()]);
        }
        if self.fullscreen().await {
            command.args.push(FULLSCREEN_ARG.into());
        } else {
            self.place(&mut command);
        }
        Ok(PreparedLaunch::new(
            command,
            TrackingTarget {
                mode: TrackingMode::OriginalProcess,
                ..Default::default()
            },
            true,
        ))
    }

    /// The guided capture of `entry`'s game, starting at `start` (its page,
    /// or the frame the game runs in), into `bundle`, with the game's `size`
    /// when its page said.
    pub fn capture(
        &self,
        entry: &GameEntry,
        start: &Url,
        size: Option<GameSize>,
        bundle: &Path,
    ) -> Result<LaunchCommand> {
        let mut command = self.command(entry, start)?;
        command
            .args
            .extend([BUNDLE_ARG.into(), utf8(bundle)?.into(), CAPTURE_ARG.into()]);
        if let Some(size) = size {
            command.args.extend([SIZE_ARG.into(), size.arg()]);
        }
        self.place(&mut command);
        Ok(command)
    }

    /// Opens the window over Caskade's, when Caskade's is on screen.
    fn place(&self, command: &mut LaunchCommand) {
        if let Some(placement) = self.placement.as_ref().and_then(|placement| placement()) {
            command.args.extend([GEOMETRY_ARG.into(), placement.arg()]);
        }
    }

    /// The folder the entry's game window keeps its webview data in:
    /// cookies, local storage and the game's saves.
    pub fn data_dir(&self, entry_id: &str) -> PathBuf {
        self.data_root.join(entry_id)
    }

    /// The game window at `url`, with `entry`'s own data folder.
    fn command(&self, entry: &GameEntry, url: &Url) -> Result<LaunchCommand> {
        let mut command = LaunchCommand::executable(&self.program);
        command.args = vec![
            WEB_GAME_ARG.into(),
            url.as_str().into(),
            DATA_DIR_ARG.into(),
            utf8(&self.data_dir(&entry.id))?.into(),
        ];
        command
            .env
            .insert(SHARED_ARRAY_BUFFER.0.into(), SHARED_ARRAY_BUFFER.1.into());
        Ok(command)
    }

    async fn fullscreen(&self) -> bool {
        let Some(settings) = &self.settings else {
            return false;
        };
        settings.flag("core.console", "active").await
    }
}

fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| LaunchError::Invalid("web game folder isn't UTF-8".into()))
}

#[cfg(test)]
mod tests {
    use super::WindowPlacement;

    #[test]
    fn placements_cross_as_text() {
        let placement = WindowPlacement {
            x: -1920,
            y: 24,
            width: 1600,
            height: 900,
        };
        assert_eq!(placement.arg(), "-1920,24,1600,900");
        assert_eq!(WindowPlacement::parse(&placement.arg()), Some(placement));
        for bad in ["", "1,2,3", "1,2,3,4,5", "1,2,0,4", "a,2,3,4", "1,2,-3,4"] {
            assert_eq!(WindowPlacement::parse(bad), None, "{bad}");
        }
    }
}
