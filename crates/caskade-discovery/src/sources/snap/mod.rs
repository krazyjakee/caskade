//! Snap applications are read from their exported desktop entries; no snap
//! commands or snap daemon requests are made.
use super::desktop::{self, application, game, proxy, records};
use crate::{
    DetectedGame, HostEnv, LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId, WatchPath,
};
pub const ID: SourceId = SourceId::new("snap");
#[derive(Debug, Default)]
pub struct Snap;
impl Source for Snap {
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
        desktop::application_dirs(env)
            .into_iter()
            .map(|path| WatchPath {
                path,
                recursive: true,
            })
            .collect()
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let mut games = vec![];
        for file in records(env, ctx)? {
            ctx.check()?;
            let Some(app) = application(env, &file, ctx) else {
                continue;
            };
            if app.snap_instance.is_none() || app.flatpak_id.is_some() || proxy(&app) {
                continue;
            }
            if let Some(game) = game(env, ctx, app, ID)? {
                games.push(game)
            }
        }
        Ok(games)
    }
}
