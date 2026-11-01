use super::*;
use crate::input;

pub struct Prepared {
    pub path: PathBuf,
    pub source: Input,
    pub artifact: ArtifactNode,
    pub resolved: input::ResolvedInput,
    pub io: probe::ReadStats,
    pub final_url: Option<url::Url>,
    /// The file a URL resolved to, before redirects: a GitHub release's
    /// asset URL names its tag, which the input (the repository) doesn't.
    pub download_url: Option<url::Url>,
    pub checksum: Option<String>,
    pub downloaded: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("URL resolution needs a decision")]
pub struct NeedsDecision(pub crate::Question);

pub fn remote(input: &str) -> Option<url::Url> {
    url::Url::parse(input)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
}

/// Whether [`prepare`] downloads `input` before reading it: an HTTP(S)
/// URL, or with the `torrent` feature a magnet link or a `.torrent` file
/// (#159). A download can be discarded and downloaded again.
pub fn downloads(input: &str) -> bool {
    #[cfg(feature = "torrent")]
    if crate::fetch::torrent::Source::parse(input).is_some() {
        return true;
    }
    remote(input).is_some()
}

pub fn job_id(input: &str, supplied: Option<String>) -> PipelineResult<String> {
    let id = supplied.unwrap_or_else(|| format!("cli-{:x}", Sha256::digest(input.as_bytes())));
    // Same request + root uses the same state across command/process restarts.
    let safe = crate::safety::sanitize_path(&id)?;
    if safe.components().count() != 1
        || safe != Path::new(&id)
        || id.eq_ignore_ascii_case(".caskade")
        || id.eq_ignore_ascii_case(".state")
    {
        return Err("job ID must be one safe filename component".into());
    }
    Ok(id)
}

pub async fn prepare(
    input: String,
    root: &Path,
    job: &str,
    host: &HostCaps,
    ctx: &Context,
) -> PipelineResult<Prepared> {
    #[cfg(feature = "torrent")]
    if let Some(torrent) = crate::fetch::torrent::Source::parse(&input) {
        let source = match &torrent {
            crate::fetch::torrent::Source::Magnet(url) => {
                Input::Urls(vec![crate::fetch::torrent::public_magnet(url)])
            }
            crate::fetch::torrent::Source::File(path) => Input::Path(path.clone()),
        };
        let path = download_torrent(root, job, &torrent, ctx).await?;
        return finish((path, source, None, None, None, true), ctx).await;
    }
    let got = if let Some(url) = remote(&input) {
        #[cfg(feature = "fetch")]
        {
            fs::create_dir_all(root)?;
            let engine = fetch_engine(false, ctx)?;
            let resolution = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(crate::fetch::FetchError::Cancelled.into()),
                result = engine.resolve(&url, host) => result?,
            };
            let requests = match resolution {
                crate::fetch::Resolution::Downloads(requests) => requests,
                crate::fetch::Resolution::NeedsUser(question) => {
                    if let crate::Question::ChooseAsset { assets } = &question
                        && let Some(index) = ctx.control.answer(&question)
                    {
                        let asset = assets.get(index).ok_or("asset choice is out of range")?;
                        vec![crate::fetch::DownloadRequest {
                            url: asset.url.clone(),
                            headers: BTreeMap::new(),
                            file_name: Some(asset.name.clone()),
                            size: asset.size,
                            sha256: asset.sha256.clone(),
                            allowed_hosts: None,
                        }]
                    } else {
                        return Err(NeedsDecision(question).into());
                    }
                }
            };
            if requests.len() != 1 {
                return Err("this input resolves to multiple files; use fetch to download the set, then select a local member".into());
            }
            let file = engine
                .download(root, job, &requests[0], ctx.cancel.clone())
                .await?;
            // A link to a .torrent installs what the torrent holds (#159).
            #[cfg(feature = "torrent")]
            if crate::fetch::torrent::is_metadata(&file.path) {
                let torrent = crate::fetch::torrent::Source::File(file.path.clone());
                let path = download_torrent(root, job, &torrent, ctx).await?;
                return finish(
                    (
                        path,
                        Input::Urls(vec![url]),
                        Some(file.final_url),
                        Some(file.source_url),
                        None,
                        true,
                    ),
                    ctx,
                )
                .await;
            }
            (
                file.path,
                Input::Urls(vec![url]),
                Some(file.final_url),
                Some(file.source_url),
                Some(file.sha256),
                true,
            )
        }
        #[cfg(not(feature = "fetch"))]
        {
            let _ = (url, root, job, host);
            return Err("URL input requires the fetch feature".into());
        }
    } else {
        let path = fs::canonicalize(input)?;
        (path.clone(), Input::Path(path), None, None, None, false)
    };
    finish(got, ctx).await
}

/// What [`prepare`] got before reading it: the path, the input, the final
/// and resolved URLs, the checksum, and whether it was downloaded.
type Got = (
    PathBuf,
    Input,
    Option<url::Url>,
    Option<url::Url>,
    Option<String>,
    bool,
);

/// Downloads a torrent into `root`'s `.caskade/torrents`; the payload
/// question becomes a [`NeedsDecision`].
#[cfg(feature = "torrent")]
async fn download_torrent(
    root: &Path,
    job: &str,
    source: &crate::fetch::torrent::Source,
    ctx: &Context,
) -> PipelineResult<PathBuf> {
    use crate::fetch::torrent::{Fetched, TorrentEngine};
    fs::create_dir_all(root)?;
    let engine = TorrentEngine::new(
        ctx.torrent.clone(),
        Arc::new(FileSystemDiskSpace),
        ctx.sink.clone(),
    );
    let control = ctx.control.clone();
    let answer = move |question: &crate::Question| control.answer(question);
    match engine
        .download(root, job, source, &answer, ctx.cancel.clone())
        .await?
    {
        Fetched::Downloaded(torrent) => Ok(torrent.path),
        Fetched::NeedsUser(question) => Err(NeedsDecision(question).into()),
    }
}

/// Reads what [`prepare`] got.
async fn finish(got: Got, ctx: &Context) -> PipelineResult<Prepared> {
    let (path, source, final_url, download_url, checksum, downloaded) = got;
    if ctx.cancel.is_cancelled() {
        return Err(ExecError::Cancelled.into());
    }
    if downloaded {
        crate::quarantine::mark_download(&path);
    }
    let control = ctx.control.clone();
    let prepared =
        caskade_jobs::blocking("Reading installer", move || -> PipelineResult<Prepared> {
            let resolved = input::resolve_path(&path)?;
            let actual = match &resolved {
                input::ResolvedInput::MultiPart { primary, .. } => primary.clone(),
                input::ResolvedInput::Folder {
                    classification: input::FolderKind::InstallerSet,
                    installers,
                    ..
                } if installers.len() == 1 => installers[0].clone(),
                input::ResolvedInput::Folder {
                    classification: input::FolderKind::InstallerSet,
                    installers,
                    ..
                } if installers.len() > 1 => {
                    let question = crate::Question::ChooseInstaller {
                        artifacts: installers
                            .iter()
                            .map(|p| {
                                crate::ArtifactId(format!(
                                    "installer:{}",
                                    p.strip_prefix(&path).unwrap_or(p).display()
                                ))
                            })
                            .collect(),
                    };
                    let index = control
                        .answer(&question)
                        .ok_or_else(|| NeedsDecision(question.clone()))?;
                    installers
                        .get(index)
                        .ok_or("installer choice is out of range")?
                        .clone()
                }
                _ => path,
            };
            let mut result =
                probe::ProbeEngine::with_builtin_formats(probe::ProbeLimits::default())
                    .probe_path(&actual)?;
            if matches!(
                &resolved,
                input::ResolvedInput::Folder {
                    classification: input::FolderKind::PortableGame,
                    ..
                }
            ) {
                result.root.role = crate::Role::Application;
                result
                    .root
                    .facts
                    .insert("folder.portable".into(), true.into());
            }
            if let input::ResolvedInput::MultiPart { files, .. } = &resolved {
                result.root.size = files
                    .iter()
                    .try_fold(0_u64, |sum, path| -> std::io::Result<u64> {
                        sum.checked_add(path.metadata()?.len())
                            .ok_or_else(|| std::io::Error::other("input size overflow"))
                    })
                    .ok();
            }
            Ok(Prepared {
                path: actual,
                source,
                artifact: result.root,
                resolved,
                io: result.io,
                final_url,
                download_url,
                checksum,
                downloaded,
            })
        })
        .await??;
    if ctx.cancel.is_cancelled() {
        return Err(ExecError::Cancelled.into());
    }
    Ok(prepared)
}

#[cfg(feature = "fetch")]
pub fn fetch_engine(
    keep_downloads: bool,
    ctx: &Context,
) -> PipelineResult<crate::fetch::FetchEngine> {
    let engine = crate::fetch::FetchEngine::new(
        crate::fetch::FetchConfig {
            keep_downloads,
            ..Default::default()
        },
        Arc::new(FileSystemDiskSpace),
        ctx.sink.clone(),
    )?;
    let mut engine = if let Some(path) = std::env::var_os("CASKADE_RESOLVERS_CONFIG") {
        let config = toml::from_str(&fs::read_to_string(path)?)?;
        engine.with_resolver_config(config)?
    } else {
        engine
    };
    for resolver in &ctx.resolvers {
        engine.register(resolver.clone())?;
    }
    Ok(engine)
}

pub fn host(profile: Option<&str>) -> PipelineResult<HostCaps> {
    let mut host = if let Some(profile) = profile {
        let builtin = match profile.trim_end_matches(".toml") {
            "windows" => Some(include_str!("../../tests/hosts/windows.toml")),
            "linux" => Some(include_str!("../../tests/hosts/linux.toml")),
            "linux-no-umu" => Some(include_str!("../../tests/hosts/linux-no-umu.toml")),
            "linux-exfat-root" => Some(include_str!("../../tests/hosts/linux-exfat-root.toml")),
            "macos" => Some(include_str!("../../tests/hosts/macos.toml")),
            "macos-no-wine" => Some(include_str!("../../tests/hosts/macos-no-wine.toml")),
            _ => None,
        };
        match builtin {
            Some(text) => toml::from_str(text)?,
            None => toml::from_str(&fs::read_to_string(profile).map_err(|_| {
                format!("unknown host profile or unreadable TOML file: {profile}")
            })?)?,
        }
    } else {
        HostCaps::detect()
    };
    if profile.is_none() {
        host.filesystem.writable = true;
    }
    Ok(host)
}

pub fn planner() -> PipelineResult<PlanEngine> {
    let mut planner = PlanEngine::new();
    planner.register(Box::new(ArchiveHandler::new()))?;
    planner.register(Box::new(crate::formats::archive::sfx::SfxHandler::new()))?;
    planner.register(Box::new(crate::formats::archive::sevenzip::SevenZipHandler))?;
    planner.register(Box::new(crate::formats::portable::FolderHandler))?;
    planner.register(Box::new(crate::formats::rom::RomHandler))?;
    #[cfg(feature = "windows-formats")]
    {
        planner.register(Box::new(crate::formats::windows::msi::MsiHandler))?;
        planner.register(Box::new(crate::formats::windows::inno::InnoHandler))?;
        planner.register(Box::new(crate::formats::windows::nsis::NsisHandler))?;
        planner.register(Box::new(crate::formats::windows::burn::BurnHandler::new()))?;
        planner.register(Box::new(
            crate::formats::windows::generic::GenericHandler::new(),
        ))?;
        planner.register(Box::new(
            crate::formats::windows::installshield::InstallShieldHandler::new(),
        ))?;
        planner.register(Box::new(crate::formats::windows::wise::WiseHandler::new()))?;
    }
    #[cfg(feature = "macos-formats")]
    planner.register(Box::new(crate::formats::macos::app::AppHandler::new()))?;
    #[cfg(feature = "macos-formats")]
    planner.register(Box::new(crate::formats::macos::pkg::PkgHandler::new()))?;
    #[cfg(feature = "macos-formats")]
    planner.register(Box::new(crate::formats::macos::dmg::DmgHandler::new()))?;
    #[cfg(feature = "linux-formats")]
    {
        planner.register(Box::new(crate::formats::linux::deb::DebHandler::new()))?;
        planner.register(Box::new(crate::formats::linux::rpm::RpmHandler::new()))?;
        planner.register(Box::new(
            crate::formats::linux::makeself::MakeselfHandler::new(),
        ))?;
        planner.register(Box::new(
            crate::formats::linux::mojosetup::MojoSetupHandler::new(),
        ))?;
        planner.register(Box::new(crate::formats::linux::appimage::AppImageHandler))?;
        planner.register(Box::new(crate::formats::linux::flatpak::FlatpakHandler))?;
    }
    Ok(planner)
}
