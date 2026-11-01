//! Linux desktop entries (#77). Original implementation of the freedesktop
//! format: https://specifications.freedesktop.org/desktop-entry/latest-single/
//! The shared parser turns Exec into argv; discovery never invokes a shell.
mod paths;
pub(crate) mod record;

pub(crate) use paths::{application_dirs, executable, icon, records};
pub use record::DesktopApplication;
pub(crate) use record::{application, game, proxy};

use crate::{
    DetectedGame, HostEnv, LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId, WatchPath,
};

pub const ID: SourceId = SourceId::new("desktop");

/// One `.desktop` file the user handed over, e.g. by dropping it on the app,
/// read like the application menu's entries. `None` for an entry that isn't a
/// visible application. A launcher's entry (`steam steam://rungameid/…`) is
/// returned as it is; `uri::target_identities` tells it apart.
pub fn desktop_file(
    env: &HostEnv,
    ctx: &ScanCtx,
    path: &std::path::Path,
) -> Result<Option<DesktopApplication>, SourceError> {
    let bytes =
        crate::sources::read(env, path, 1 << 20).map_err(|error| SourceError::io(path, error))?;
    let entry = crate::parsers::desktop::parse(&bytes, &crate::parsers::Limits::default())
        .map_err(|error| SourceError::parse(path, error))?;
    let id = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("dropped.desktop")
        .to_owned();
    let file = record::DesktopFile {
        id,
        path: path.into(),
        entry,
    };
    Ok(application(env, &file, ctx))
}
#[derive(Debug, Default)]
pub struct Desktop;
impl Source for Desktop {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::LINUX
    }
    fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        application_dirs(env)
            .into_iter()
            .map(|path| WatchPath {
                path,
                recursive: true,
            })
            .collect()
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let mut result = vec![];
        for file in records(env, ctx)? {
            ctx.check()?;
            let Some(app) = application(env, &file, ctx) else {
                continue;
            };
            // Proxies may be wrapped in flatpak/snap. Route them once, here,
            // before container sources exclude their launcher applications.
            if (proxy(&app)
                || (app.flatpak_id.is_none()
                    && app.snap_instance.is_none()
                    && !app.emulator_only()))
                && let Some(game) = game(env, ctx, app, ID)?
            {
                result.push(game);
            }
        }
        Ok(result)
    }
}

/// Reusable OS probe for emulator discovery (#102). Includes visible emulator
/// desktop entries from XDG, Flatpak exports and Snap exports, with parsed
/// launch targets; it does not put emulators into the game's ScanReport.
pub fn emulator_candidates(
    env: &HostEnv,
    ctx: &ScanCtx,
) -> Result<Vec<DesktopApplication>, SourceError> {
    let mut result = vec![];
    for file in records(env, ctx)? {
        ctx.check()?;
        if let Some(app) = application(env, &file, ctx)
            && app.emulator_only()
        {
            result.push(app)
        }
    }
    Ok(result)
}
