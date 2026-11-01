//! Install jobs (§3.2, §3.4): one per install or update, in memory, each
//! change published as `emulation.setup.changed`.

use super::{EmulatorSetup, ManagedEmulator, SetupJob, SetupStatus, first_run, source, unpack};
use crate::{
    Error, Result,
    bus::Event,
    emulation::{
        Availability, Emulator, EmulatorDefinition, EmulatorInstall, EmulatorProfile,
        InstallSource, PlatformSettings, ProfileRef, VarContext, bios_dirs, emulator_dir,
        vars::Expander,
    },
    lock,
};
use std::path::{Path, PathBuf};
#[cfg(feature = "net")]
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub(super) struct JobState {
    job: SetupJob,
    cancel: CancellationToken,
    plan: Plan,
}

/// What a job does besides installing.
#[derive(Debug, Clone, Default)]
struct Plan {
    /// Keep the installed emulator; only add profiles and their files.
    reuse: bool,
    /// The definition profile wanted (a platform's RetroArch core); `None`
    /// fetches the files of every profile whose platforms have games.
    profile: Option<String>,
}

/// Progress events at most this often.
#[cfg(feature = "net")]
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

impl EmulatorSetup {
    fn publish(&self, job: &SetupJob) {
        self.inner
            .bus
            .emit(Event::EmulationSetupChanged(Box::new(job.clone())));
    }

    fn update_job(&self, id: &str, change: impl FnOnce(&mut SetupJob)) -> Option<SetupJob> {
        let job = {
            let mut jobs = lock(&self.inner.jobs);
            let state = jobs.get_mut(id)?;
            change(&mut state.job);
            state.job.revision += 1;
            state.job.clone()
        };
        self.publish(&job);
        Some(job)
    }

    /// Every job since startup, oldest first.
    pub fn jobs(&self) -> Vec<SetupJob> {
        lock(&self.inner.jobs)
            .values()
            .map(|state| state.job.clone())
            .collect()
    }

    pub fn job(&self, id: &str) -> Option<SetupJob> {
        lock(&self.inner.jobs)
            .get(id)
            .map(|state| state.job.clone())
    }

    /// Cancels a job that hasn't finished.
    pub fn cancel(&self, id: &str) -> Result<SetupJob> {
        let state = lock(&self.inner.jobs)
            .get(id)
            .map(|state| (state.cancel.clone(), state.job.clone()))
            .ok_or_else(|| Error::Invalid(crate::tr!("unknown emulator install {id}", id = id)))?;
        state.0.cancel();
        Ok(state.1)
    }

    /// Installs the platform's recommended emulator (§2.4).
    /// An emulator Caskade installed that is still there keeps its
    /// install: only the profile (and its core) is added.
    pub async fn install_for(&self, platform: &str) -> Result<SetupJob> {
        let catalog = self.inner.emulation.catalog();
        let (definition, profile) = self.pick(catalog.recommended(platform)).ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "nothing to install for {platform} on this system",
                platform = platform
            ))
        })?;
        let reuse = self.installed_emulator(&definition.id).await?.is_some();
        let plan = Plan {
            reuse,
            profile: Some(profile.name.clone()),
        };
        self.start(&definition.id, Some(platform.to_owned()), plan)
    }

    /// Installs (or reinstalls) emulator `definition_id`.
    pub async fn install(&self, definition_id: &str) -> Result<SetupJob> {
        self.start(definition_id, None, Plan::default())
    }

    /// The managed emulator of a definition, while its install is there.
    async fn installed_emulator(&self, definition_id: &str) -> Result<Option<Emulator>> {
        let Some(managed) = self.managed(definition_id).await? else {
            return Ok(None);
        };
        let emulation = &self.inner.emulation;
        Ok(self
            .inner
            .store
            .get_emulator(&managed.emulator_id)
            .await?
            .filter(|emulator| {
                crate::emulation::installed(
                    &emulator.install,
                    emulation.folders(),
                    emulation.search_path(),
                )
            }))
    }

    fn start(&self, definition_id: &str, platform: Option<String>, plan: Plan) -> Result<SetupJob> {
        let catalog = self.inner.emulation.catalog();
        let definition = catalog.emulator(definition_id).ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "unknown emulator {emulator}",
                emulator = definition_id
            ))
        })?;
        if self.source(definition).is_none() {
            return Err(Error::Invalid(crate::tr!(
                "{emulator} can't be installed on this system",
                emulator = definition.name
            )));
        }
        let busy = lock(&self.inner.jobs)
            .values()
            .any(|state| state.job.definition_id == definition_id && !state.job.status.finished());
        if busy {
            return Err(Error::Invalid(crate::tr!(
                "{emulator} is already being installed",
                emulator = definition.name
            )));
        }
        let job = SetupJob {
            id: crate::model::new_id(),
            revision: 0,
            definition_id: definition_id.to_owned(),
            name: definition.name.clone(),
            platform_id: platform,
            status: SetupStatus::Queued,
            version: None,
            downloaded: 0,
            total: None,
            emulator_id: None,
            defaults_set: Vec::new(),
            fetched_bios: Vec::new(),
            warnings: Vec::new(),
            error: None,
        };
        let cancel = CancellationToken::new();
        lock(&self.inner.jobs).insert(
            job.id.clone(),
            JobState {
                job: job.clone(),
                cancel: cancel.clone(),
                plan,
            },
        );
        self.publish(&job);
        let service = self.clone();
        let id = job.id.clone();
        tokio::spawn(async move {
            let _activity = caskade_jobs::global().activity(crate::tr!("Installing an emulator"));
            let result = service.run(&id, &cancel).await;
            let status = match &result {
                Ok(()) => SetupStatus::Done,
                Err(_) if cancel.is_cancelled() => SetupStatus::Cancelled,
                Err(_) => SetupStatus::Failed,
            };
            if let Err(error) = &result {
                tracing::warn!(job = id, %error, "emulator install failed");
            }
            service.update_job(&id, |job| {
                job.status = status;
                if status == SetupStatus::Failed {
                    job.error = result.err().map(|error| error.to_string());
                }
            });
            let _ = tokio::fs::remove_dir_all(service.inner.cache.join(&id)).await;
        });
        Ok(job)
    }

    async fn run(&self, id: &str, cancel: &CancellationToken) -> Result<()> {
        let (job, plan) = lock(&self.inner.jobs)
            .get(id)
            .map(|state| (state.job.clone(), state.plan.clone()))
            .ok_or_else(|| Error::Invalid(crate::tr!("the job is gone")))?;
        let catalog = self.inner.emulation.catalog();
        let definition = catalog.emulator(&job.definition_id).ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "unknown emulator {emulator}",
                emulator = job.definition_id
            ))
        })?;
        let source = self
            .source(definition)
            .ok_or_else(|| Error::Invalid(crate::tr!("no install source on this system")))?
            .clone();
        let existing = self.managed(&definition.id).await?;
        let kept = match (plan.reuse, &existing) {
            (true, Some(managed)) => self
                .installed_emulator(&definition.id)
                .await?
                .map(|emulator| (emulator.install, managed.clone())),
            _ => None,
        };
        let (install, version, file, dir) = match (&source, kept) {
            (_, Some((install, managed))) => {
                (install, managed.installed, managed.file, managed.dir)
            }
            (InstallSource::Flatpak { app }, None) => {
                self.update_job(id, |job| job.status = SetupStatus::Downloading);
                self.flatpak(app, cancel).await?;
                let install = EmulatorInstall::Flatpak {
                    app_id: app.clone(),
                };
                (install, None, None, None)
            }
            (_, None) => {
                let resolved = source::resolve(&self.inner.hosts, &source).await?;
                self.update_job(id, |job| {
                    job.status = SetupStatus::Downloading;
                    job.version.clone_from(&resolved.version);
                });
                let download = self.inner.cache.join(id).join(&resolved.file);
                self.download(id, &resolved.url, &download, cancel).await?;
                self.update_job(id, |job| job.status = SetupStatus::Unpacking);
                let target = self.inner.root.join(&definition.id);
                let old_version = existing
                    .as_ref()
                    .and_then(|managed| managed.installed.clone())
                    .unwrap_or_else(|| "previous".into());
                let install = self
                    .unpack(id, definition, &download, &target, &old_version, cancel)
                    .await?;
                let file = resolved.file.clone();
                (
                    install,
                    resolved.version,
                    Some(file),
                    Some(target.to_string_lossy().into_owned()),
                )
            }
        };
        if cancel.is_cancelled() {
            return Err(Error::Invalid(crate::tr!("cancelled")));
        }
        self.update_job(id, |job| job.status = SetupStatus::Configuring);
        let context = self.context(&install);
        let mut warnings =
            first_run::apply(definition, &install, self.inner.emulation.os(), &context);
        let wanted = self
            .wanted_profiles(definition, plan.profile.as_deref())
            .await?;
        warnings.extend(
            self.fetch_requires(id, definition, &install, &wanted, cancel)
                .await,
        );
        if cancel.is_cancelled() {
            return Err(Error::Invalid(crate::tr!("cancelled")));
        }
        let previous = match &existing {
            Some(managed) => self.inner.store.get_emulator(&managed.emulator_id).await?,
            None => None,
        };
        let (emulator_id, defaults_set) = self.configure(definition, install, previous).await?;
        // BIOS files from RomM for the platforms it now runs by default.
        let mut platforms = defaults_set.clone();
        platforms.extend(job.platform_id.clone());
        platforms.dedup();
        let mut fetched_bios = Vec::new();
        for platform in &platforms {
            match self.fetch_bios(platform).await {
                Ok(fetch) => fetched_bios.extend(fetch.fetched),
                Err(error) => tracing::debug!(platform, %error, "no BIOS files fetched"),
            }
        }
        warnings.extend(self.missing_bios(&emulator_id).await);
        let mut managed = existing.unwrap_or(ManagedEmulator {
            emulator_id: emulator_id.clone(),
            definition_id: definition.id.clone(),
            source: source.kind().into(),
            dir: None,
            installed: None,
            file: None,
            latest: None,
            latest_url: None,
            checked_at_ms: 0,
            skipped: None,
            error: None,
        });
        managed.emulator_id.clone_from(&emulator_id);
        managed.source = source.kind().into();
        managed.dir = dir;
        managed.installed.clone_from(&version);
        managed.file = file;
        managed.latest = version;
        managed.latest_url = None;
        managed.checked_at_ms = crate::now_ms();
        managed.error = None;
        self.inner.store.upsert_managed_emulator(&managed).await?;
        self.update_job(id, |job| {
            job.emulator_id = Some(emulator_id);
            job.defaults_set = defaults_set;
            job.fetched_bios = fetched_bios;
            job.warnings = warnings;
        });
        Ok(())
    }

    pub(super) async fn managed(&self, definition_id: &str) -> Result<Option<ManagedEmulator>> {
        Ok(self
            .inner
            .store
            .managed_emulators()
            .await?
            .into_iter()
            .find(|row| row.definition_id == definition_id))
    }

    #[cfg(feature = "net")]
    async fn download(
        &self,
        id: &str,
        url: &url::Url,
        to: &Path,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let mut last = Instant::now() - PROGRESS_EVERY;
        super::download::fetch(url, to, cancel, |downloaded, total| {
            let done = total.is_some_and(|total| downloaded >= total);
            if done || last.elapsed() >= PROGRESS_EVERY {
                last = Instant::now();
                self.update_job(id, |job| {
                    job.downloaded = downloaded;
                    job.total = total;
                });
            }
        })
        .await
    }

    #[cfg(not(feature = "net"))]
    async fn download(
        &self,
        _id: &str,
        _url: &url::Url,
        _to: &Path,
        _cancel: &CancellationToken,
    ) -> Result<()> {
        Err(Error::Io(std::io::Error::other(
            "built without network support",
        )))
    }

    /// Unpacks `file` next to `target`, finds the emulator in it, then puts
    /// it in `target`'s place with the user's files from the old install.
    async fn unpack(
        &self,
        id: &str,
        definition: &EmulatorDefinition,
        file: &Path,
        target: &Path,
        old_version: &str,
        cancel: &CancellationToken,
    ) -> Result<EmulatorInstall> {
        let os = self.inner.emulation.os();
        let rules = definition.detect.on(os).cloned().unwrap_or_default();
        let staging = self.inner.root.join(".staging").join(id);
        let aside = self.inner.root.join(".old").join(id);
        let (file, target_dir) = (file.to_path_buf(), target.to_path_buf());
        let old_version = old_version.to_owned();
        let name = definition.name.clone();
        let cancel = cancel.clone();
        tokio::task::spawn_blocking(move || {
            let _ = std::fs::remove_dir_all(&staging);
            let unpacked = (|| {
                unpack::unpack(&file, &staging)?;
                let install = unpack::locate(&staging, &rules, os).ok_or_else(|| {
                    let listing = unpack::files(&staging)
                        .unwrap_or_default()
                        .iter()
                        .take(40)
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    tracing::warn!(emulator = name, listing, "no emulator in the release");
                    Error::Invalid(crate::tr!(
                        "{emulator}'s download doesn't contain the emulator",
                        emulator = name
                    ))
                })?;
                unpack::record_shipped(&staging)?;
                if cancel.is_cancelled() {
                    return Err(Error::Invalid(crate::tr!("cancelled")));
                }
                Ok(install)
            })();
            let install = match unpacked {
                Ok(install) => install,
                Err(error) => {
                    let _ = std::fs::remove_dir_all(&staging);
                    return Err(error);
                }
            };
            // The old install goes aside; it comes back if the swap fails.
            if target_dir.exists() {
                std::fs::create_dir_all(aside.parent().unwrap_or(&aside))?;
                std::fs::rename(&target_dir, &aside)?;
            }
            if let Err(error) = std::fs::rename(&staging, &target_dir) {
                if aside.exists() {
                    let _ = std::fs::rename(&aside, &target_dir);
                }
                let _ = std::fs::remove_dir_all(&staging);
                return Err(error.into());
            }
            if aside.exists() {
                let kept = unpack::carry(&aside, &target_dir, &old_version)?;
                tracing::info!(
                    emulator = name,
                    kept,
                    "user files carried to the new version"
                );
                std::fs::remove_dir_all(&aside)?;
            }
            Ok(unpack::rebase(install, &staging, &target_dir))
        })
        .await
        .map_err(|error| Error::Invalid(error.to_string()))?
    }

    /// `flatpak install --user` from Flathub.
    async fn flatpak(&self, app: &str, cancel: &CancellationToken) -> Result<()> {
        let mut command = tokio::process::Command::new(&self.inner.flatpak);
        command
            .args([
                "install",
                "--user",
                "--noninteractive",
                "-y",
                "flathub",
                app,
            ])
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        let child = command.spawn().map_err(|error| {
            Error::Invalid(crate::tr!("couldn't run flatpak: {error}", error = error))
        })?;
        let output = tokio::select! {
            () = cancel.cancelled() => return Err(Error::Invalid(crate::tr!("cancelled"))),
            output = child.wait_with_output() => output?,
        };
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Invalid(crate::tr!(
                "flatpak couldn't install {app}: {error}",
                app = app,
                error = message.trim()
            )));
        }
        Ok(())
    }

    pub(super) fn context(&self, install: &EmulatorInstall) -> VarContext {
        let emulation = &self.inner.emulation;
        VarContext {
            emulator_dir: emulator_dir(install, emulation.folders(), emulation.search_path()),
            caskade_dir: emulation.caskade_dir().map(Path::to_path_buf),
            folders: emulation.folders().clone(),
            ..VarContext::default()
        }
    }

    /// Adds the emulator (or points the existing managed one at the new
    /// install) with its available profiles, and makes it the default of
    /// every platform it runs that has no working default. Returns its id
    /// and the platforms it became the default of.
    async fn configure(
        &self,
        definition: &EmulatorDefinition,
        install: EmulatorInstall,
        previous: Option<Emulator>,
    ) -> Result<(String, Vec<String>)> {
        let emulation = &self.inner.emulation;
        let fresh = previous.is_none();
        let emulator = match previous {
            Some(mut emulator) => {
                emulator.install = install;
                emulator
            }
            None => {
                let names: Vec<String> = emulation
                    .emulators()
                    .await?
                    .into_iter()
                    .map(|view| view.emulator.name)
                    .collect();
                let name = if names.contains(&definition.name) {
                    format!("{} (Caskade)", definition.name)
                } else {
                    definition.name.clone()
                };
                Emulator {
                    id: String::new(),
                    name,
                    definition: Some(definition.id.clone()),
                    install,
                    profiles: definition
                        .profiles
                        .iter()
                        .map(|profile| EmulatorProfile::built_in(&profile.name))
                        .collect(),
                }
            }
        };
        let mut view = emulation.save_emulator(emulator).await?;
        // An existing one gains the definition's profiles that work now
        // (a core fetched for another platform).
        if !fresh {
            let views = emulation.emulators().await?;
            let addable: Vec<String> = views
                .iter()
                .find(|row| row.emulator.id == view.emulator.id)
                .map(|row| {
                    row.addable
                        .iter()
                        .filter(|profile| profile.available)
                        .map(|profile| profile.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            if !addable.is_empty() {
                let mut emulator = view.emulator.clone();
                emulator
                    .profiles
                    .extend(addable.into_iter().map(EmulatorProfile::built_in));
                view = emulation.save_emulator(emulator).await?;
            }
        }
        // A new emulator keeps the profiles it can run (RetroArch: those
        // with their cores, maybe none yet); the rest stay addable, and a
        // platform's install adds its own.
        if fresh {
            let available: Vec<&str> = view
                .profiles
                .iter()
                .filter(|profile| profile.availability == Availability::Available)
                .map(|profile| profile.id.as_str())
                .collect();
            if available.len() < view.profiles.len() {
                let mut emulator = view.emulator.clone();
                emulator
                    .profiles
                    .retain(|profile| available.contains(&profile.id()));
                view = emulation.save_emulator(emulator).await?;
            }
        }
        if let Some(dir) = self.bios_dir(&view.emulator) {
            let _ = tokio::fs::create_dir_all(dir).await;
        }

        // Defaults where the platform has none that works.
        let views = emulation.platforms().await?;
        let mut defaults_set = Vec::new();
        for profile in &view.profiles {
            if profile.availability != Availability::Available {
                continue;
            }
            for platform in &profile.platforms {
                let Some(current) = views.iter().find(|row| &row.platform.id == platform) else {
                    continue;
                };
                let works = current.settings.default.as_ref().is_some_and(|default| {
                    current.compatible.iter().any(|row| {
                        row.available
                            && row.emulator_id == default.emulator_id
                            && row.profile_id == default.profile_id
                    })
                });
                if works || defaults_set.contains(platform) {
                    continue;
                }
                emulation
                    .set_platform_settings(PlatformSettings {
                        default: Some(ProfileRef {
                            emulator_id: view.emulator.id.clone(),
                            profile_id: profile.id.clone(),
                        }),
                        ..current.settings.clone()
                    })
                    .await?;
                defaults_set.push(platform.clone());
            }
        }
        Ok((view.emulator.id, defaults_set))
    }

    /// The profiles whose files to fetch: the one asked for, or every one
    /// whose platforms have games.
    async fn wanted_profiles(
        &self,
        definition: &EmulatorDefinition,
        profile: Option<&str>,
    ) -> Result<Vec<String>> {
        if let Some(profile) = profile {
            return Ok(vec![profile.to_owned()]);
        }
        let platforms = self.inner.emulation.platforms().await?;
        let has_games = |id: &String| {
            platforms
                .iter()
                .any(|view| &view.platform.id == id && view.rom_count > 0)
        };
        Ok(definition
            .profiles
            .iter()
            .filter(|profile| profile.platforms.iter().any(has_games))
            .map(|profile| profile.name.clone())
            .collect())
    }

    /// Downloads the wanted profiles' missing `requires` files (RetroArch
    /// cores) from the definition's `requires_from`. A file that can't be
    /// fetched leaves its profile unavailable and becomes a warning.
    async fn fetch_requires(
        &self,
        id: &str,
        definition: &EmulatorDefinition,
        install: &EmulatorInstall,
        wanted: &[String],
        cancel: &CancellationToken,
    ) -> Vec<String> {
        let os = self.inner.emulation.os();
        let Some(template) = definition.requires_from.on(os) else {
            return Vec::new();
        };
        let context = self.context(install);
        let expander = Expander {
            vars: &definition.vars,
            os,
            kind: install.kind(),
            context: &context,
        };
        let arch = if std::env::consts::ARCH == "aarch64" {
            "arm64"
        } else {
            "x86_64"
        };
        let mut warnings = Vec::new();
        for profile in wanted.iter().filter_map(|name| definition.profile(name)) {
            for required in &profile.requires {
                let Ok(path) = expander.expand(required).map(PathBuf::from) else {
                    continue;
                };
                if path.exists() {
                    continue;
                }
                let Some(file) = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                else {
                    continue;
                };
                let fetched = async {
                    let url =
                        url::Url::parse(&template.replace("{file}", &file).replace("{arch}", arch))
                            .map_err(|error| Error::Invalid(error.to_string()))?;
                    let name = url
                        .path_segments()
                        .and_then(|mut segments| segments.next_back())
                        .unwrap_or(&file)
                        .to_owned();
                    let download = self.inner.cache.join(id).join("requires").join(&name);
                    self.update_job(id, |job| {
                        job.status = SetupStatus::Downloading;
                        job.downloaded = 0;
                        job.total = None;
                    });
                    self.download(id, &url, &download, cancel).await?;
                    let (file, path) = (file.clone(), path.clone());
                    tokio::task::spawn_blocking(move || unpack::take(&download, &file, &path))
                        .await
                        .map_err(|error| Error::Invalid(error.to_string()))?
                }
                .await;
                if let Err(error) = fetched {
                    tracing::warn!(emulator = definition.id, file, %error, "couldn't fetch a profile's file");
                    warnings.push(crate::tr!(
                        "Couldn't download {emulator}'s {profile} ({file}): {error}",
                        emulator = definition.name,
                        profile = profile.name,
                        file = file,
                        error = error
                    ));
                }
            }
        }
        self.update_job(id, |job| job.status = SetupStatus::Configuring);
        warnings
    }

    /// The folder the emulator looks for BIOS files in.
    fn bios_dir(&self, emulator: &Emulator) -> Option<PathBuf> {
        let emulation = &self.inner.emulation;
        let catalog = emulation.catalog();
        let definition = catalog.emulator(emulator.definition.as_deref()?)?;
        bios_dirs(
            None,
            &definition.vars,
            emulation.os(),
            emulator.install.kind(),
            &self.context(&emulator.install),
        )
        .into_iter()
        .next()
    }

    /// Required BIOS files the new emulator still lacks, as warnings.
    async fn missing_bios(&self, emulator_id: &str) -> Vec<String> {
        let Ok(views) = self.inner.emulation.emulators().await else {
            return Vec::new();
        };
        let Some(view) = views.iter().find(|view| view.emulator.id == emulator_id) else {
            return Vec::new();
        };
        let dir = self.bios_dir(&view.emulator);
        let mut missing: Vec<String> = view
            .profiles
            .iter()
            .filter(|profile| profile.availability == Availability::Available)
            .flat_map(|profile| &profile.bios)
            .filter(|bios| bios.required && bios.found == Some(false))
            .map(|bios| bios.file.clone())
            .collect();
        missing.sort();
        missing.dedup();
        if missing.is_empty() {
            return Vec::new();
        }
        let files = missing.join(", ");
        vec![match dir {
            Some(dir) => crate::tr!(
                "{emulator} needs BIOS files to play: {files}. Put them in {folder}.",
                emulator = view.emulator.name,
                files = files,
                folder = dir.display()
            ),
            None => crate::tr!(
                "{emulator} needs BIOS files to play: {files}. Put them.",
                emulator = view.emulator.name,
                files = files
            ),
        }]
    }
}
