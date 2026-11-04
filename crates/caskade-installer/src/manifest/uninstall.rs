//! Reverse a managed install while retaining the executor's root exclusion.
use super::native::{
    self, NativeRegistry, NativeUninstallPolicy, NativeUninstallReport, PlatformRegistry,
    PreparedNative,
};
use super::{
    ExternalEffect, ExternalLocation, ExternalRoots, InstallManifest, ManifestError, ManifestPath,
    RootManifest,
};
use crate::exec::{CancellationToken, ExecError, Executor};
use crate::plan::presets::{InstallerFamily, build_uninstall_args};
use crate::runtime::{ProcessCommand, Runtime};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct UninstallRequest {
    pub job_id: String,
    pub root: PathBuf,
    pub relative_dir: PathBuf,
    pub manifest: InstallManifest,
}

/// Future alias providers (#31) remove their own link/junction, never its target.
/// No alias paths are guessed or stored as absolute paths in the manifest.
pub trait AliasRemover: Send + Sync {
    fn remove_alias(&self, game_dir: &Path, manifest: &InstallManifest) -> io::Result<bool>;
}
struct NoAlias;
impl AliasRemover for NoAlias {
    fn remove_alias(&self, _: &Path, _: &InstallManifest) -> io::Result<bool> {
        Ok(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum UninstallOperation {
    Steps,
    External(ExternalLocation),
    Alias,
    Game,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum UninstallFailureReason {
    Unsupported { step: String },
    Execution { message: String },
    Filesystem { message: String },
    MissingExternalScope { scope: String },
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallFailure {
    pub operation: UninstallOperation,
    pub reason: UninstallFailureReason,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallReport {
    pub relative_dir: ManifestPath,
    pub steps_completed: bool,
    pub external_effects_reverted: u64,
    pub alias_removed: bool,
    /// True also when the game was already absent. Failures still make is_clean false.
    pub game_removed: bool,
    pub failures: Vec<UninstallFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeUninstallReport>,
}
impl UninstallReport {
    pub fn is_clean(&self) -> bool {
        self.game_removed && self.failures.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UninstallError {
    #[error("invalid uninstall request: {0}")]
    Invalid(String),
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),
    #[error("uninstall filesystem operation: {0}")]
    Io(#[from] io::Error),
    #[error("uninstall executor: {0}")]
    Execution(#[from] ExecError),
    #[error("unsupported uninstall step {step}; game retained for retry")]
    Unsupported {
        step: String,
        report: Box<UninstallReport>,
    },
    #[error("uninstall cancelled")]
    Cancelled { report: Box<UninstallReport> },
    #[error("uninstall incomplete: {report:?}")]
    Incomplete { report: Box<UninstallReport> },
}
impl UninstallError {
    pub fn report(&self) -> Option<&UninstallReport> {
        match self {
            Self::Unsupported { report, .. }
            | Self::Cancelled { report }
            | Self::Incomplete { report } => Some(report),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct Uninstaller {
    executor: Executor,
    external_roots: ExternalRoots,
    aliases: Arc<dyn AliasRemover>,
    registry: Arc<dyn NativeRegistry>,
    native_policy: NativeUninstallPolicy,
}
impl Uninstaller {
    pub fn new(executor: Executor) -> Self {
        Self {
            executor,
            external_roots: ExternalRoots::new(),
            aliases: Arc::new(NoAlias),
            registry: Arc::new(PlatformRegistry),
            native_policy: NativeUninstallPolicy::PreferNative,
        }
    }
    pub fn with_native_registry(mut self, registry: Arc<dyn NativeRegistry>) -> Self {
        self.registry = registry;
        self
    }
    pub fn with_native_policy(mut self, policy: NativeUninstallPolicy) -> Self {
        self.native_policy = policy;
        self
    }
    pub fn with_external_roots(mut self, roots: ExternalRoots) -> Self {
        self.external_roots = roots;
        self
    }
    pub fn with_alias_remover(mut self, aliases: Arc<dyn AliasRemover>) -> Self {
        self.aliases = aliases;
        self
    }
    /// Remove the manifest's recorded alias (#31) from this store; only the
    /// link is removed, before the game tree, so no dangling alias remains.
    pub fn with_aliases(self, store: crate::runtime::alias::AliasStore) -> Self {
        self.with_alias_remover(Arc::new(store))
    }

    /// Run recorded steps first, then reverse scoped external effects and remove
    /// the alias and game tree. All deletion remains under the same executor root
    /// lease. On a failed step/external effect/alias, retain the game and manifest
    /// for retry and return an error containing every collected cleanup failure.
    pub async fn uninstall(
        &self,
        request: UninstallRequest,
        cancellation: CancellationToken,
    ) -> Result<UninstallReport, UninstallError> {
        let relative =
            ManifestPath::new(request.relative_dir.to_str().ok_or_else(|| {
                UninstallError::Invalid("game directory must be Unicode".into())
            })?)?;
        if relative.as_str().contains('/') || relative.as_str().eq_ignore_ascii_case(".caskade") {
            return Err(UninstallError::Invalid(
                "expected one managed game folder outside .caskade".into(),
            ));
        }
        let root = dunce::canonicalize(&request.root)?;
        let game = root.join(relative.as_path());
        // Optional root identity exists independently of individual game records.
        match RootManifest::read(&root) {
            Ok(identity) if identity.root_id() != request.manifest.record().plan.root_id => {
                return Err(UninstallError::Invalid(
                    "storage root identity differs from manifest".into(),
                ));
            }
            Ok(_) => {}
            Err(ManifestError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        match std::fs::symlink_metadata(&game) {
            Ok(metadata) if !metadata.is_dir() || metadata.is_symlink() => {
                return Err(UninstallError::Invalid(
                    "managed game path must be a real directory".into(),
                ));
            }
            Ok(_) => {
                if InstallManifest::read(&game)? != request.manifest {
                    return Err(UninstallError::Invalid(
                        "provided manifest differs from installed record".into(),
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let this = self.clone();
        let mut native = native::prepare(
            &game,
            &request.manifest,
            &*self.registry,
            self.native_policy,
        )?;
        let steps = std::mem::take(&mut native.steps);
        let no_steps = steps.is_empty();
        let manifest = request.manifest;
        let cancel = cancellation.clone();
        let callback_relative = relative.clone();
        let report = self
            .executor
            .execute_steps_in_place_then(
                &root,
                relative.as_path(),
                &request.job_id,
                steps,
                cancellation,
                move |directory, result| {
                    Ok(this.finish(
                        directory,
                        callback_relative,
                        &game,
                        &manifest,
                        result,
                        no_steps,
                        native,
                        &cancel,
                    ))
                },
            )
            .await?;
        if let Some(step) = report
            .failures
            .iter()
            .find_map(|failure| match &failure.reason {
                UninstallFailureReason::Unsupported { step } => Some(step.clone()),
                _ => None,
            })
        {
            return Err(UninstallError::Unsupported {
                step,
                report: Box::new(report),
            });
        }
        if report
            .failures
            .iter()
            .any(|failure| failure.reason == UninstallFailureReason::Cancelled)
        {
            return Err(UninstallError::Cancelled {
                report: Box::new(report),
            });
        }
        if !report.is_clean() {
            return Err(UninstallError::Incomplete {
                report: Box::new(report),
            });
        }
        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        &self,
        root: &Dir,
        relative_dir: ManifestPath,
        game: &Path,
        manifest: &InstallManifest,
        steps: crate::exec::Result<()>,
        no_steps: bool,
        mut native: PreparedNative,
        cancellation: &CancellationToken,
    ) -> UninstallReport {
        let mut report = UninstallReport {
            relative_dir,
            steps_completed: false,
            external_effects_reverted: 0,
            alias_removed: false,
            game_removed: false,
            failures: Vec::new(),
            native: None,
        };
        let steps = match steps {
            Err(error) if native.recognized && !matches!(error, ExecError::Cancelled) => {
                native.report.manifest_fallback = true;
                native.report.diagnostics.push(format!("Native uninstall failed; reverting recorded effects and remaining files: {error}"));
                Ok(())
            }
            Ok(()) if native.report.attempted => {
                native.report.succeeded = true;
                Ok(())
            }
            other => other,
        };
        let failed_native =
            native.report.attempted && native.report.manifest_fallback && !native.report.succeeded;
        if native.recognized || native.report.manifest_fallback {
            report.native = Some(native.report);
        }
        match steps {
            Ok(()) => report.steps_completed = !failed_native,
            Err(ExecError::Io(error)) if no_steps && error.kind() == io::ErrorKind::NotFound => {
                report.steps_completed = true
            }
            Err(ExecError::UnsupportedStep { step }) => report.failures.push(UninstallFailure {
                operation: UninstallOperation::Steps,
                reason: UninstallFailureReason::Unsupported { step },
            }),
            Err(ExecError::MissingRuntime { runtime }) => report.failures.push(UninstallFailure {
                operation: UninstallOperation::Steps,
                reason: UninstallFailureReason::Unsupported {
                    step: format!("runtime {runtime:?}"),
                },
            }),
            Err(ExecError::Runtime(crate::runtime::RuntimeError::NotImplemented { operation })) => {
                report.failures.push(UninstallFailure {
                    operation: UninstallOperation::Steps,
                    reason: UninstallFailureReason::Unsupported { step: operation },
                })
            }
            Err(ExecError::Cancelled) => report.failures.push(UninstallFailure {
                operation: UninstallOperation::Steps,
                reason: UninstallFailureReason::Cancelled,
            }),
            Err(error) => report.failures.push(UninstallFailure {
                operation: UninstallOperation::Steps,
                reason: UninstallFailureReason::Execution {
                    message: error.to_string(),
                },
            }),
        }
        for effect in manifest.record().external_effects.iter().rev() {
            if cancellation.is_cancelled() {
                break;
            }
            let location = effect.location();
            if let ExternalEffect::Registry { effect } = effect {
                match self.registry.undo_effect(effect) {
                    Ok(()) => report.external_effects_reverted += 1,
                    Err(error) => report.failures.push(UninstallFailure {
                        operation: UninstallOperation::External(location.clone()),
                        reason: UninstallFailureReason::Filesystem {
                            message: error.to_string(),
                        },
                    }),
                }
                continue;
            }
            let Some(base) = self.external_roots.get(&location.scope) else {
                report.failures.push(UninstallFailure {
                    operation: UninstallOperation::External(location.clone()),
                    reason: UninstallFailureReason::MissingExternalScope {
                        scope: location.scope.clone(),
                    },
                });
                continue;
            };
            match undo_effect(base, effect) {
                Ok(()) => report.external_effects_reverted += 1,
                Err(error) => report.failures.push(UninstallFailure {
                    operation: UninstallOperation::External(location.clone()),
                    reason: UninstallFailureReason::Filesystem {
                        message: error.to_string(),
                    },
                }),
            }
        }
        if !cancellation.is_cancelled() {
            for entry in &native.registrations {
                if let Err(error) = self.registry.remove_uninstall_entry(entry) {
                    report.failures.push(UninstallFailure {
                        operation: UninstallOperation::External(entry.location.clone()),
                        reason: UninstallFailureReason::Filesystem {
                            message: error.to_string(),
                        },
                    });
                }
            }
        }
        if cancellation.is_cancelled()
            && !report
                .failures
                .iter()
                .any(|failure| failure.reason == UninstallFailureReason::Cancelled)
        {
            report.failures.push(UninstallFailure {
                operation: UninstallOperation::Game,
                reason: UninstallFailureReason::Cancelled,
            });
        }
        if report.failures.is_empty() {
            match self.aliases.remove_alias(game, manifest) {
                Ok(removed) => report.alias_removed = removed,
                Err(error) => report.failures.push(UninstallFailure {
                    operation: UninstallOperation::Alias,
                    reason: UninstallFailureReason::Filesystem {
                        message: error.to_string(),
                    },
                }),
            }
        }
        if report.failures.is_empty() {
            let result = match root.symlink_metadata(&report.relative_dir) {
                Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {
                    root.remove_dir_all(&report.relative_dir)
                }
                Ok(_) => Err(io::Error::other(
                    "managed game directory changed type during uninstall",
                )),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            };
            match result {
                Ok(()) => report.game_removed = true,
                Err(error) => report.failures.push(UninstallFailure {
                    operation: UninstallOperation::Game,
                    reason: UninstallFailureReason::Filesystem {
                        message: error.to_string(),
                    },
                }),
            }
        }
        if report.game_removed
            && report.failures.is_empty()
            && !cancellation.is_cancelled()
            && let Some(code) = native.forget_product
            && let Some(native_report) = report.native.as_mut()
        {
            match self.executor.runtime(&crate::RuntimeSpec::Native) {
                Some(runtime) => forget_msi_product(&*runtime, &code, native_report),
                None => native_report
                    .diagnostics
                    .push("Windows Installer still lists the product: no native runtime".into()),
            }
        }
        report
    }
}

/// msiexec's exit codes that leave Windows Installer without the product:
/// done, unknown product (already gone) and done but a restart is pending.
const MSI_FORGOTTEN: [i32; 3] = [0, 1605, 3010];

/// Drop Windows Installer's own records of an MSI product (its `Products`,
/// `UserData` and cached package) after the manifest route removed the game
/// (#40). The game's files are gone by now, so a failure is only a note.
///
/// msiexec cannot touch the moved files: it removes only what its component
/// records name, and those still hold the paths the MSI installed to. A move
/// rewrites the game's own files and registry values, never Windows
/// Installer's component records, so they name the old location, which the
/// move emptied. And Caskade has already deleted the moved folder itself.
fn forget_msi_product(runtime: &dyn Runtime, code: &str, report: &mut NativeUninstallReport) {
    let args = match build_uninstall_args(InstallerFamily::Msi, Some(code)) {
        Ok(args) => args,
        Err(error) => {
            report.diagnostics.push(format!(
                "Windows Installer still lists the product: {error}"
            ));
            return;
        }
    };
    let command = ProcessCommand {
        program: "msiexec".into(),
        args,
        env: Default::default(),
        cwd: std::env::temp_dir(),
    };
    let exit = runtime.spawn(&command).and_then(|process| {
        let exit = runtime.wait(process, Duration::from_secs(120));
        let _ = runtime.kill(process);
        exit
    });
    match exit {
        Ok(exit) if MSI_FORGOTTEN.contains(&exit.code) => report.windows_installer_removed = true,
        Ok(exit) => report.diagnostics.push(format!(
            "Windows Installer still lists the product: msiexec /x {code} exited with {}",
            exit.code
        )),
        Err(error) => report.diagnostics.push(format!(
            "Windows Installer still lists the product: msiexec /x {code} failed: {error}"
        )),
    }
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
fn undo_effect(base: &Path, effect: &ExternalEffect) -> io::Result<()> {
    let directory = Dir::open_ambient_dir(base, cap_std::ambient_authority())?;
    let path = &effect.location().path;
    let metadata = match directory.symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    match effect {
        ExternalEffect::Registry { .. } => Err(io::Error::other(
            "registry effects require the registry provider",
        )),
        ExternalEffect::Directory { .. } => {
            let Some(metadata) = metadata else {
                return Ok(());
            };
            if !metadata.is_dir() || metadata.is_symlink() {
                return Err(io::Error::other("external directory changed type"));
            }
            directory.remove_dir(path)
        }
        ExternalEffect::File { original, .. } => {
            if metadata
                .as_ref()
                .is_some_and(|meta| !meta.is_file() && !meta.is_symlink())
            {
                return Err(io::Error::other("external file changed type"));
            }
            if let Some(bytes) = original {
                restore_file(&directory, path.as_path(), bytes)
            } else if metadata.is_some() {
                directory.remove_file(path)
            } else {
                Ok(())
            }
        }
    }
}

fn restore_file(directory: &Dir, path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    directory.create_dir_all(parent)?;
    let name = parent.join(format!(
        ".caskade-restore-{}-{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = directory.open_with(&name, OpenOptions::new().write(true).create_new(true))?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        directory.rename(&name, directory, path)
    })();
    if result.is_err() {
        let _ = directory.remove_file(name);
    }
    result
}
