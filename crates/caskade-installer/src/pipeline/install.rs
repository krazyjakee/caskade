use super::*;

pub async fn install(
    value: String,
    root: PathBuf,
    keep_downloads: bool,
    extract: bool,
    job: Option<String>,
    ctx: &Context,
) -> PipelineResult<InstallOutcome> {
    let job_id = input::job_id(&value, job)?;
    let host = actual_host(ctx)?;
    fs::create_dir_all(&root)?;
    let root = dunce::canonicalize(root)?;
    let prepared = input::prepare(value, &root, &job_id, &host, ctx).await?;
    install_prepared(prepared, root, job_id, host, keep_downloads, extract, ctx).await
}

/// Installs an input that [`input::prepare`] already resolved (and, for URLs,
/// downloaded), so callers can queue downloads and installs separately.
pub async fn install_prepared(
    prepared: input::Prepared,
    root: PathBuf,
    job_id: String,
    mut host: HostCaps,
    keep_downloads: bool,
    extract: bool,
    ctx: &Context,
) -> PipelineResult<InstallOutcome> {
    ctx.config.compatibility_runner.configure_host(&mut host);
    if host.os == crate::OperatingSystem::Linux
        && prepared.artifact.platform == crate::PayloadPlatform::Windows
        && ctx.config.compatibility_runner != super::CompatibilityRunner::SystemWine
        && !host.has_runner(crate::host::RunnerKind::Proton)
        && let Some(bootstrap) = super::config::umu_bootstrap(ctx)
    {
        // Plan with the bootstrapped umu: it is downloaded only if a
        // candidate runs a program under it.
        host.runners.push(crate::host::RunnerInfo {
            kind: crate::host::RunnerKind::Proton,
            program: bootstrap.umu_run(),
        });
    }
    // Keep quarantined disc bytes alive until the nested job and its finalizer
    // finish. Provenance remains the original disc/URL in prepared.source.
    #[cfg(feature = "disc")]
    let (prepared, disc) = {
        let mut prepared = prepared;
        let session = super::disc::open(&prepared, &job_id, ctx).await?;
        if let Some(session) = &session {
            if let Some(outcome) = session.handoff() {
                return Ok(outcome);
            }
            if let Some(question @ crate::Question::ChooseInstaller { artifacts }) =
                &session.question()
            {
                let Some(index) = ctx.control.answer(question) else {
                    return Ok(InstallOutcome::NeedsUser(question.clone()));
                };
                let id = artifacts
                    .get(index)
                    .ok_or("disc installer choice is out of range")?;
                prepared.artifact = session
                    .artifacts
                    .iter()
                    .find(|node| &node.id == id)
                    .ok_or("disc installer is no longer available")?
                    .clone();
                prepared.path = session
                    .sources()
                    .remove(id)
                    .ok_or("disc installer source is missing")?;
            } else {
                prepared.artifact = session.installer()?.clone();
                prepared.path = session.installer_path()?;
            }
        }
        (prepared, session)
    };
    if let Some(handoff) = crate::formats::rom::RomHandler.handoff(&prepared.artifact) {
        return Ok(handoff);
    }
    if prepared.path.is_dir()
        && (root.starts_with(&prepared.path)
            || prepared.path.starts_with(&root)
            || prepared.path.join(".caskade").exists())
    {
        return Err("portable source must be disjoint from the storage root and contain no managed metadata".into());
    }
    let policy = if extract {
        Policy {
            execution: crate::plan::ExecutionPolicy::Never,
            windows_method: crate::plan::WindowsMethod::Extract,
            ..ctx.config.policy.clone()
        }
    } else {
        ctx.config.policy.clone()
    };
    let mut planner = input::planner()?;
    for handler in &ctx.handlers {
        planner.register(Box::new(super::plugins::Shared(handler.clone())))?;
    }
    let mut plans = planner.plan(std::slice::from_ref(&prepared.artifact), &host, &policy)?;
    ctx.control.apply(&mut plans)?;
    if let Some(question) = plans.question {
        return Ok(InstallOutcome::NeedsUser(question));
    }
    if plans.candidates.is_empty() {
        return Ok(InstallOutcome::Unsupported(
            crate::Reason::UnsupportedFormat {
                format: prepared.artifact.format,
            },
        ));
    }
    if ctx.review.is_some() {
        plans = super::plugins::review(ctx, &prepared.artifact, plans).await?;
        ctx.control.record_plans(&plans);
    }
    let identity = match RootManifest::read(&root) {
        Ok(identity) => identity,
        Err(ManifestError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            let identity = RootManifest::new(format!(
                "root-{:x}",
                Sha256::digest(root.to_string_lossy().as_bytes())
            ))?;
            identity.write(&root)?;
            identity
        }
        Err(error) => return Err(error.into()),
    };
    let sources = BTreeMap::from([(prepared.artifact.id.clone(), prepared.path.clone())]);
    #[cfg(feature = "disc")]
    let sources = disc.as_ref().map_or(sources, |disc| disc.sources());
    #[cfg(feature = "disc")]
    let input_warnings = disc
        .as_ref()
        .map(|disc| disc.warnings().to_vec())
        .unwrap_or_default();
    #[cfg(not(feature = "disc"))]
    let input_warnings = vec![];
    let mut hints = ctx.hints.clone();
    hints.source_name = hints.source_name.or_else(|| {
        prepared
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    });
    if let Input::Urls(urls) = &prepared.source {
        hints.source_url = hints.source_url.or_else(|| urls.first().cloned());
    }
    let title = discover::clean_title(hints.source_name.as_deref().unwrap_or("Game"));
    let fallback_id = format!("{:x}", Sha256::digest(job_id.as_bytes()));
    let relative_dir = crate::exec::folder_name(
        ctx.folder_template.as_deref().unwrap_or("{title}"),
        &title,
        &job_id,
    )
    .or_else(|_| crate::exec::folder_name("Game-{id}", "Game", &fallback_id[..16]))?;
    let source = ManifestSource::from_input(
        &prepared.source,
        prepared.final_url,
        prepared.checksum.map(Sha256Digest::new).transpose()?,
    )?;
    let mut base_worker = executor(ctx, sources.clone());
    super::config::register_runtimes(&mut base_worker, &plans, &host, ctx).await?;
    // An earlier install whose undo can't run yet keeps its journal for the
    // next try; it has nothing to do with this one.
    match base_worker.cleanup(&root).await {
        Err(crate::exec::ExecError::Rollback { message }) => {
            tracing::warn!(%message, "interrupted install left for the next run");
        }
        result => {
            result?;
        }
    }
    #[cfg(feature = "linux-formats")]
    if prepared.artifact.format == crate::Format::Flatpak {
        let report = crate::formats::linux::flatpak::FlatpakInstaller::new(base_worker)
            .install(
                ExecRequest {
                    root: root.clone(),
                    job_id: job_id.clone(),
                    relative_dir,
                    plans,
                    sources,
                },
                prepared.artifact,
                source,
                identity.root_id().into(),
                ctx.cancel.clone(),
            )
            .await?;
        if let Some(warning) =
            download_cleanup(prepared.downloaded, keep_downloads, &root, &job_id, ctx).await
        {
            tracing::warn!(
                code = warning.code,
                message = warning.message,
                "Flatpak installed; downloaded source retained"
            );
        }
        return Ok(report.outcome);
    }
    let extractor = extractor(ctx, sources.clone());
    let watcher = Arc::new(crate::runtime::OutOfRootWatcher::for_host());
    let finalizer = Arc::new(RecordInstall {
        extractor: extractor.clone(),
        host,
        hints,
        artifacts: vec![prepared.artifact],
        source,
        root_id: identity.root_id().into(),
        external_roots: watcher.external_roots().clone(),
        discovery: Mutex::new(None),
        control: ctx.control.clone(),
        compatibility_runner: ctx.config.compatibility_runner,
        install_id: ctx.install_id.clone(),
        package: Mutex::new(None),
    });
    let config = crate::exec::ExecConfig {
        interactive: crate::exec::interactive::InteractiveConfig {
            outside_roots: watcher.external_roots().values().cloned().collect(),
            ..Default::default()
        },
        execution: policy.execution,
        ..Default::default()
    };
    let worker = base_worker
        .with_config(config)
        .with_observer(watcher)
        .with_extractor(extractor)
        .with_finalizer(finalizer.clone());
    let worker = match &ctx.aliases {
        Some(aliases) => worker.with_aliases(aliases.clone(), identity.root_id()),
        None => worker,
    };
    let result = worker
        .execute(
            ExecRequest {
                root: root.clone(),
                job_id: job_id.clone(),
                relative_dir,
                plans,
                sources,
            },
            ctx.cancel.clone(),
        )
        .await;
    // A static strategy failed and the next one would run installer code.
    if let Err(ExecError::NeedsUser {
        question: question @ crate::Question::ConfirmExecution { .. },
    }) = &result
    {
        return Ok(InstallOutcome::NeedsUser(question.clone()));
    }
    let found = finalizer
        .discovery
        .lock()
        .expect("CLI discovery poisoned")
        .clone();
    if let Some(question) = found
        .as_ref()
        .and_then(|discovery| discovery.question.clone())
    {
        match result {
            Err(ExecError::AllCandidatesFailed { .. }) => (),
            Err(error) => return Err(error.into()),
            Ok(_) => return Err("ambiguous discovery unexpectedly committed".into()),
        }
        return Ok(InstallOutcome::NeedsUser(question));
    }
    let report = result?;
    let package = finalizer
        .package
        .lock()
        .expect("pipeline package poisoned")
        .take();
    if let Some(package) = package {
        if let Some(warning) =
            download_cleanup(prepared.downloaded, keep_downloads, &root, &job_id, ctx).await
        {
            tracing::warn!(
                code = warning.code,
                message = warning.message,
                "package installed; downloaded source retained"
            );
        }
        return Ok(InstallOutcome::HandOff(package));
    }
    let discovery = found.ok_or("installer completed without discovery")?;
    // A launch target that vanished after discovery found it (security
    // software, another program) leaves an install that can't start: undo
    // it and keep the download for a retry, never report success (#158).
    if let Some(target) = discovery.launch_targets.first()
        && fs::symlink_metadata(report.dir.join(&target.path)).is_err()
    {
        let missing = report.relative_dir.join(&target.path);
        tracing::warn!(path = %missing.display(), "launch target missing after install; undoing it");
        if let Err(error) =
            super::uninstall_with(report.dir.clone(), vec![], ctx, ctx.install_id.is_none()).await
        {
            tracing::warn!(%error, "incomplete install not removed");
        }
        return Err(ExecError::MissingExpected { path: missing }.into());
    }
    // On macOS a quarantined source quarantines what it installed (CSK-15).
    crate::quarantine::carry(&prepared.path, &report.dir);
    let cleanup_warning =
        download_cleanup(prepared.downloaded, keep_downloads, &root, &job_id, ctx).await;
    Ok(InstallOutcome::Installed(Installed {
        root_id: identity.root_id().into(),
        dir: report.relative_dir,
        title: discovery.title,
        ids: discovery.ids,
        runtime: discovery
            .launch_targets
            .first()
            .map(|target| target.runtime.clone())
            .unwrap_or(RuntimeSpec::Native),
        launch_targets: discovery.launch_targets,
        dependencies: vec![],
        warnings: discovery
            .warnings
            .into_iter()
            .chain(cleanup_warning)
            .chain(input_warnings)
            .collect(),
        manifest: ".caskade/install.json".into(),
    }))
}

async fn download_cleanup(
    downloaded: bool,
    keep_downloads: bool,
    root: &Path,
    job_id: &str,
    ctx: &Context,
) -> Option<crate::Warning> {
    if !downloaded {
        return None;
    }
    #[cfg(feature = "fetch")]
    {
        let result = match input::fetch_engine(keep_downloads, ctx) {
            Ok(engine) => engine.after_install(root, job_id).await,
            Err(error) => {
                return Some(crate::Warning {
                    code: "download_retained".into(),
                    message: format!(
                        "Installed successfully, but download cleanup failed: {error}"
                    ),
                    path: None,
                });
            }
        };
        result.err().map(|error| crate::Warning {
            code: "download_retained".into(),
            message: format!("Installed successfully, but download cleanup failed: {error}"),
            path: None,
        })
    }
    #[cfg(not(feature = "fetch"))]
    {
        let _ = (keep_downloads, root, job_id, ctx);
        None
    }
}
