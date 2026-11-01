use super::*;

pub(super) struct RecordInstall {
    pub(super) extractor: Arc<extract::PipelineExtractor>,
    pub(super) host: HostCaps,
    pub(super) hints: DiscoveryHints,
    pub(super) artifacts: Vec<ArtifactNode>,
    pub(super) source: ManifestSource,
    pub(super) root_id: String,
    pub(super) external_roots: crate::manifest::ExternalRoots,
    pub(super) discovery: Mutex<Option<Discovery>>,
    pub(super) control: RunControl,
    pub(super) compatibility_runner: CompatibilityRunner,
    pub(super) install_id: Option<String>,
    /// A deb or rpm the system package manager installed.
    pub(super) package: Mutex<Option<crate::HandOff>>,
}
impl Finalizer for RecordInstall {
    fn finalize(&self, context: &FinalizationContext<'_>) -> crate::exec::Result<()> {
        #[cfg(feature = "linux-formats")]
        if let Some(info) = package_delegate(context.candidate, &self.artifacts) {
            return self
                .record_package(context, info)
                .map_err(|error| ExecError::InvalidRequest {
                    message: error.to_string(),
                });
        }
        let record = || -> PipelineResult<()> {
            let mut hints = self.hints.clone();
            for artifact in &self.artifacts {
                if let Some(extra) = self.extractor.discovery_hints(artifact) {
                    hints.installer_title = extra.installer_title.or(hints.installer_title);
                    hints.ids.extend(extra.ids);
                    hints.launch_targets.extend(extra.launch_targets);
                }
                if hints.installer_title.is_none() {
                    hints.installer_title = artifact
                        .facts
                        .get("title")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned);
                }
            }
            #[cfg(feature = "linux-formats")]
            let discovery = if self.artifacts.iter().any(|artifact| {
                matches!(
                    artifact.format,
                    crate::Format::Makeself | crate::Format::MojoSetup
                )
            }) {
                crate::formats::linux::mojosetup::discover(context.game_dir, &self.host)?
            } else {
                discover::discover(context.game_dir, &self.host, &hints)?
            };
            #[cfg(not(feature = "linux-formats"))]
            let discovery = discover::discover(context.game_dir, &self.host, &hints)?;
            let mut discovery = discovery;
            // An explicit Wine preference must survive discovery and DB
            // reconstruction; Linux discovery otherwise defaults to Proton.
            if self.compatibility_runner == CompatibilityRunner::SystemWine
                && self.host.os != crate::OperatingSystem::Windows
            {
                prefer_wine(&mut discovery.launch_targets);
                if let Some(crate::Question::ChooseLaunchTarget { targets }) =
                    &mut discovery.question
                {
                    prefer_wine(targets);
                }
            }
            if let Some(question @ crate::Question::ChooseLaunchTarget { targets }) =
                &discovery.question
                && let Some(index) = self.control.answer(question)
            {
                let target = targets
                    .get(index)
                    .ok_or("launch choice is out of range")?
                    .clone();
                let position = discovery
                    .launch_targets
                    .iter()
                    .position(|t| t == &target)
                    .ok_or("chosen launch target is no longer available")?;
                discovery.launch_targets.swap(0, position);
                discovery.question = None;
            }
            // A stable identity for this install that survives moves and
            // renames: the app keys its library entry on it.
            match &self.install_id {
                Some(id) => {
                    discovery.ids.insert(INSTALL_ID.into(), id.clone());
                }
                None => {
                    discovery
                        .ids
                        .entry(INSTALL_ID.into())
                        .or_insert_with(|| uuid::Uuid::now_v7().to_string());
                }
            }
            *self.discovery.lock().expect("pipeline discovery poisoned") = Some(discovery.clone());
            if discovery.question.is_some() {
                return Err(ExecError::DecisionRequired.into());
            }
            if discovery.launch_targets.is_empty() {
                return Err("archive contains no discovered launch target".into());
            }
            if matches!(
                discovery.launch_targets[0].runtime,
                RuntimeSpec::WinePrefix { .. }
            ) && !self.host.has_runner(crate::host::RunnerKind::Wine)
            {
                return Err(
                    "system Wine is unavailable for the selected compatibility runner".into(),
                );
            }
            if discovery
                .launch_targets
                .first()
                .is_some_and(|target| target.runtime != RuntimeSpec::Native)
                && (!self.host.filesystem.exec_bits || !self.host.filesystem.symlinks)
            {
                return Err(super::config::PREFIX_FILESYSTEM_UNSUPPORTED.into());
            }
            let runtime = discovery.launch_targets[0].runtime.clone();
            let plan = InstallPlan {
                schema_version: 1,
                input: self.source.input.clone(),
                artifacts: self.artifacts.clone(),
                root_id: self.root_id.clone(),
                relative_dir: context.relative_dir.into(),
                candidate: context.candidate.clone(),
            };
            let mut files = vec![];
            inventory(
                &crate::safety::open_staging(context.game_dir)?,
                Path::new(""),
                &mut files,
                context,
            )?;
            #[cfg(feature = "windows-formats")]
            let uninstall_steps =
                crate::formats::windows::uninstall_steps(&self.artifacts, context.candidate);
            #[cfg(not(feature = "windows-formats"))]
            let uninstall_steps = vec![];
            let external_effects =
                crate::manifest::effects_from_journal(context.journal, &self.external_roots)?;
            #[cfg(all(windows, feature = "windows-formats"))]
            let external_effects = {
                let mut effects = external_effects;
                if matches!(
                    context.candidate.handler.0.as_str(),
                    "inno" | "nsis" | "msi"
                ) && context.candidate.steps.iter().any(|step| {
                    matches!(
                        step,
                        crate::Step::Run {
                            runtime: RuntimeSpec::Native,
                            ..
                        } | crate::Step::RunInteractive {
                            runtime: RuntimeSpec::Native,
                            ..
                        }
                    )
                }) {
                    effects.extend(crate::manifest::native::uninstall_registration_effects(
                        context.game_dir,
                        &crate::manifest::native::PlatformRegistry,
                    )?);
                }
                effects
            };
            InstallManifest::new(InstallRecord {
                title: discovery.title,
                ids: discovery.ids,
                source: self.source.clone(),
                plan,
                handlers: vec![HandlerVersion {
                    id: context.candidate.handler.clone(),
                    version: env!("CARGO_PKG_VERSION").into(),
                }],
                files,
                launch_targets: discovery.launch_targets,
                runtime,
                uninstall_steps,
                external_effects,
                relocation: RelocationRecord {
                    last_path: Some(context.final_dir.to_string_lossy().into_owned()),
                    ..Default::default()
                },
                alias: context.alias.map(Into::into),
            })?
            .write(context.game_dir)?;
            Ok(())
        };
        record().map_err(|error| ExecError::InvalidRequest {
            message: error.to_string(),
        })
    }
}

/// The package of a deb or rpm candidate that the package manager installs.
#[cfg(feature = "linux-formats")]
fn package_delegate(
    candidate: &crate::Candidate,
    artifacts: &[ArtifactNode],
) -> Option<crate::formats::linux::deb::PackageInfo> {
    if candidate.strategy != crate::Strategy::Delegate
        || !matches!(candidate.handler.0.as_str(), "deb" | "rpm")
    {
        return None;
    }
    artifacts
        .iter()
        .find_map(|artifact| artifact.facts.get("linux.package"))
        .and_then(|info| serde_json::from_value(info.clone()).ok())
}

#[cfg(feature = "linux-formats")]
impl RecordInstall {
    /// The package's files belong to the package manager, so the game folder
    /// keeps only the record of how to uninstall it; the game launches the
    /// command the package installed.
    fn record_package(
        &self,
        context: &FinalizationContext<'_>,
        info: crate::formats::linux::deb::PackageInfo,
    ) -> PipelineResult<()> {
        use crate::formats::linux::deb::installed;
        context.steps.checkpoint(0)?;
        let uninstall_steps: Vec<_> = context
            .candidate
            .steps
            .iter()
            .flat_map(|step| match step {
                crate::Step::Delegate { uninstall, .. } => uninstall.clone(),
                _ => vec![],
            })
            .map(|command| crate::Step::Delegate {
                tool: command.tool,
                args: command.args,
                uninstall: vec![],
                only_if: command.only_if,
            })
            .collect();
        // Without a command the game is still installed; it just can't play.
        let launch = installed::installed_files(&self.host, info.format, &info.name)
            .map(|files| installed::launch(Path::new("/"), &files, &info.name))
            .unwrap_or_else(|error| {
                tracing::warn!(%error, package = info.name, "installed package files unknown");
                None
            });
        let title = launch
            .as_ref()
            .and_then(|launch| launch.title.clone())
            .unwrap_or_else(|| discover::clean_title(&info.name));
        let kind = if info.format == crate::Format::Deb {
            "deb"
        } else {
            "rpm"
        };
        InstallManifest::new(InstallRecord {
            title: title.clone(),
            ids: BTreeMap::from([(kind.into(), info.name.clone())]),
            source: self.source.clone(),
            plan: InstallPlan {
                schema_version: 1,
                input: self.source.input.clone(),
                artifacts: self.artifacts.clone(),
                root_id: self.root_id.clone(),
                relative_dir: context.relative_dir.into(),
                candidate: context.candidate.clone(),
            },
            handlers: vec![HandlerVersion {
                id: context.candidate.handler.clone(),
                version: env!("CARGO_PKG_VERSION").into(),
            }],
            files: vec![],
            launch_targets: vec![],
            runtime: RuntimeSpec::Native,
            uninstall_steps: uninstall_steps.clone(),
            external_effects: vec![],
            relocation: RelocationRecord {
                verdict: crate::manifest::RelocationVerdict::Pinned,
                ..Default::default()
            },
            alias: None,
        })?
        .write(context.game_dir)?;
        let (program, args) = launch
            .map(|launch| (Some(launch.program), launch.args))
            .unwrap_or_default();
        *self.package.lock().expect("pipeline package poisoned") = Some(crate::HandOff::Package {
            package: info.name,
            title,
            program,
            args,
            uninstall_steps,
        });
        Ok(())
    }
}

fn prefer_wine(targets: &mut [crate::LaunchTarget]) {
    for target in targets {
        if let RuntimeSpec::ProtonPrefix { id } = &target.runtime {
            target.runtime = RuntimeSpec::WinePrefix { id: id.clone() };
        }
    }
}

fn inventory(
    root: &cap_std::fs::Dir,
    base: &Path,
    files: &mut Vec<InstalledFile>,
    context: &FinalizationContext<'_>,
) -> PipelineResult<()> {
    for entry in root.entries()? {
        context.steps.checkpoint(0)?;
        let entry = entry?;
        let path = base.join(entry.file_name());
        let metadata = root.symlink_metadata(entry.file_name())?;
        if metadata.is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            inventory(&root.open_dir(entry.file_name())?, &path, files, context)?;
        } else if metadata.is_file() {
            let mut hash = Sha256::new();
            let mut reader = root.open(entry.file_name())?;
            let mut buffer = [0; 64 * 1024];
            loop {
                context.steps.checkpoint(0)?;
                let count = reader.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            files.push(InstalledFile {
                path: ManifestPath::new(path.to_str().ok_or("non-UTF8 installed file")?)?,
                size: metadata.len(),
                sha256: Some(Sha256Digest::new(format!("{:x}", hash.finalize()))?),
            });
        }
    }
    files.sort_by(|first, second| first.path.cmp(&second.path));
    Ok(())
}
