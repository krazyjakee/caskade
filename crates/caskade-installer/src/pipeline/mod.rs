//! The end-to-end install and uninstall pipeline: input preparation (paths,
//! URLs, discs, ROM hand-offs), planning, execution, discovery and the
//! install record. The development CLI and the app core both drive it.

mod choices;
mod config;
pub use config::{CompatibilityRunner, InstallerConfig};
#[cfg(feature = "disc")]
mod disc;
pub mod extract;
pub mod input;
mod install;
pub mod relocate;
pub use choices::RunControl;
mod plugins;
pub use plugins::{PlanReview, ReviewFuture};
mod record;

pub use install::{install, install_prepared};

use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::{
    ArtifactNode, Input, InstallOutcome, InstallPlan, Installed, RuntimeSpec,
    discover::{self, Discovery, DiscoveryHints},
    disk::FileSystemDiskSpace,
    events::EventSink,
    exec::{CancellationToken, ExecError, ExecRequest, Executor, FinalizationContext, Finalizer},
    formats::archive::ArchiveHandler,
    host::HostCaps,
    manifest::{
        HandlerVersion, InstallManifest, InstallRecord, InstalledFile, ManifestError, ManifestPath,
        ManifestSource, RelocationRecord, RootManifest, Sha256Digest, UninstallReport,
        UninstallRequest, Uninstaller,
    },
    plan::{PlanEngine, Policy},
    probe,
    runtime::NativeRuntime,
};
use sha2::{Digest, Sha256};

pub use input::NeedsDecision;
use record::RecordInstall;

/// The `ids` key of the install record's stable UUIDv7 identity.
pub const INSTALL_ID: &str = "caskade-install";

/// Pipeline errors carry the failing layer's message.
pub type PipelineResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// What every pipeline run shares: where progress goes, how it is
/// cancelled, and the pinned 7-Zip sidecar.
pub struct Context {
    pub sink: Arc<dyn EventSink>,
    pub cancel: CancellationToken,
    pub tool: Option<crate::formats::archive::sevenzip::SevenZip>,
    /// Gives each installed game a stable alias (#31); `None` skips it.
    pub aliases: Option<crate::runtime::alias::AliasStore>,
    pub control: RunControl,
    /// Validated per-root folder naming preference; None uses {title}.
    pub folder_template: Option<String>,
    pub config: InstallerConfig,
    /// Explicit capability injection for tests and trusted app boundaries.
    pub host: Option<HostCaps>,
    /// Plugin handlers (#55), planned and ranked with the built-ins.
    pub handlers: Vec<Arc<dyn crate::plan::Handler>>,
    /// Plugin URL resolvers (#55); they run before the built-ins.
    #[cfg(feature = "fetch")]
    pub resolvers: Vec<Arc<dyn crate::fetch::UrlResolver>>,
    /// Peer discovery, upload and limits for torrent inputs (#159).
    #[cfg(feature = "torrent")]
    pub torrent: crate::fetch::torrent::TorrentConfig,
    /// `install.plan` (#55): sees the ranked plans before anything runs.
    pub review: Option<Arc<dyn PlanReview>>,
    /// The [`INSTALL_ID`] to record instead of a new one: an update
    /// reinstalls a game under its old identity.
    pub install_id: Option<String>,
    /// What the caller already knows about the game, e.g. the executable a
    /// dropped shortcut points at. Format handlers' own hints take precedence.
    pub hints: DiscoveryHints,
    /// Where umu-launcher is bootstrapped when the host has none; `None`
    /// uses the user's data directory.
    pub umu: Option<crate::runtime::umu::UmuBootstrap>,
}

impl Context {
    /// A context with the host's 7-Zip (when found).
    pub fn new(sink: Arc<dyn EventSink>, cancel: CancellationToken) -> Self {
        Self {
            sink,
            cancel,
            tool: crate::formats::archive::sevenzip::SevenZip::from_host(&HostCaps::detect()).ok(),
            aliases: None,
            control: RunControl::default(),
            folder_template: None,
            config: InstallerConfig::default(),
            host: None,
            handlers: Vec::new(),
            #[cfg(feature = "fetch")]
            resolvers: Vec::new(),
            #[cfg(feature = "torrent")]
            torrent: Default::default(),
            review: None,
            install_id: None,
            hints: DiscoveryHints::default(),
            umu: None,
        }
    }
}

pub fn extractor(
    ctx: &Context,
    sources: BTreeMap<crate::ArtifactId, PathBuf>,
) -> Arc<extract::PipelineExtractor> {
    Arc::new(extract::PipelineExtractor::new(
        ctx,
        sources,
        HostCaps::detect(),
        Arc::new(NativeRuntime::new()),
    ))
}

pub fn executor(ctx: &Context, sources: BTreeMap<crate::ArtifactId, PathBuf>) -> Executor {
    Executor::new(
        Arc::new(NativeRuntime::new()),
        Arc::new(FileSystemDiskSpace),
        ctx.sink.clone(),
    )
    .with_extractor(extractor(ctx, sources))
}

/// This host's capabilities with the context's 7-Zip.
pub fn actual_host(ctx: &Context) -> PipelineResult<HostCaps> {
    let mut host = ctx.host.clone().map_or_else(|| input::host(None), Ok)?;
    host.seven_zip = ctx.tool.as_ref().map(|tool| tool.program().to_path_buf());
    Ok(host)
}

/// Uninstalls the managed game in `game_dir` (a directory directly inside
/// its storage root). `external_roots` binds `scope=path` rewrite scopes.
pub async fn uninstall(
    game_dir: PathBuf,
    external_roots: Vec<String>,
    ctx: &Context,
) -> PipelineResult<UninstallReport> {
    uninstall_with(game_dir, external_roots, ctx, true).await
}

/// [`uninstall`], leaving the game's alias when `alias` is false: an
/// update that is undone keeps the alias of the version it replaced.
pub(crate) async fn uninstall_with(
    game_dir: PathBuf,
    external_roots: Vec<String>,
    ctx: &Context,
    alias: bool,
) -> PipelineResult<UninstallReport> {
    let metadata = fs::symlink_metadata(&game_dir)?;
    if !metadata.is_dir() || metadata.is_symlink() {
        return Err("game directory must be a real directory, not a symlink".into());
    }
    let game = fs::canonicalize(game_dir)?;
    let root = game
        .parent()
        .ok_or("game directory has no storage root")?
        .to_path_buf();
    let relative_dir = PathBuf::from(game.file_name().ok_or("game directory has no name")?);
    let manifest = InstallManifest::read(&game)?;
    let mut roots = crate::runtime::default_watch_roots();
    let mut supplied_scopes = std::collections::BTreeSet::new();
    for binding in external_roots {
        let (scope, path) = binding
            .split_once('=')
            .ok_or("external root must be scope=path")?;
        if scope.is_empty() || !supplied_scopes.insert(scope.to_owned()) {
            return Err("external scope must be nonempty and unique".into());
        }
        roots.insert(scope.to_owned(), fs::canonicalize(path)?);
    }
    let job_id = input::job_id(&format!("uninstall:{}", game.display()), None)?;
    let mut worker = executor(ctx, BTreeMap::new());
    if manifest.record().runtime != RuntimeSpec::Native {
        let host = actual_host(ctx)?;
        config::register_uninstall_runtime(
            &mut worker,
            manifest.record().runtime.clone(),
            &host,
            ctx,
        );
    }
    let uninstaller = Uninstaller::new(worker).with_external_roots(roots);
    let uninstaller = match &ctx.aliases {
        Some(aliases) if alias => uninstaller.with_aliases(aliases.clone()),
        _ => uninstaller,
    };
    Ok(uninstaller
        .uninstall(
            UninstallRequest {
                job_id,
                root,
                relative_dir,
                manifest,
            },
            ctx.cancel.clone(),
        )
        .await?)
}
