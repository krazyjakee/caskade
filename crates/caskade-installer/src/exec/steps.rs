use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};

use cap_std::fs::{Dir, OpenOptions};

use crate::{
    Program, RuntimeSpec, SourcePath, Step,
    runtime::{ProcessCommand, Runtime},
    safety::{self, SafeDir},
};

use super::{
    ExecConfig, ExecError, ExecRequest, Extractor, JournalEntry, JournalHandle, ProcessObserver,
    Result,
    api::{NoObserver, Runtimes, source_path},
    context::{StepContext, relative, subdir},
    journal,
};

/// Argument placeholder replaced after collision resolution, so a native
/// installer's directory argument always names the actual final game folder.
pub const INSTALL_DIR: &str = "{caskade.install_dir}";

/// A complete argv entry resolved from the explicit source map at execution.
/// It is never interpreted by a shell or expanded inside a larger argument.
pub fn source_argument(artifact: &crate::ArtifactId) -> String {
    format!("{{caskade.source:{}}}", artifact.0)
}

pub(super) struct Services<'a> {
    pub request: &'a ExecRequest,
    pub game_path: &'a Path,
    /// Expansion of `INSTALL_DIR`: the stable alias when one exists (#31),
    /// otherwise the game path. Sources and working directories stay real.
    pub install_dir: &'a Path,
    pub runtimes: &'a Runtimes,
    pub config: &'a ExecConfig,
    pub extractor: &'a dyn Extractor,
    pub observer: &'a dyn ProcessObserver,
    pub journal: &'a JournalHandle,
    pub compensate_delegates: bool,
}

impl Services<'_> {
    pub fn execute(&self, step: &Step, context: &StepContext) -> Result<()> {
        context.checkpoint(0)?;
        match step {
            Step::WriteFile { path, contents } => {
                context.checkpoint(contents.len() as u64)?;
                let mut destination =
                    context.open_destination(Path::new("."), contents.len() as u64)?;
                let mut reader = context.progress_reader(contents.as_slice());
                destination.write_file(
                    text(path)?,
                    &mut reader,
                    Some(contents.len() as u64),
                    0,
                    0o644,
                )?;
                reader.finish()
            }
            Step::Copy { from, to } => self.copy(from, to, context),
            Step::CopyRange {
                from,
                offset,
                length,
                path,
            } => self.copy_range(from, *offset, *length, path, context),
            Step::Move {
                from: SourcePath::Game(from),
                to,
            } => {
                let source = relative(from)?;
                let destination = relative(to)?;
                subdir(
                    &context.dir,
                    source.parent().unwrap_or(Path::new(".")),
                    false,
                )?;
                subdir(
                    &context.dir,
                    destination.parent().unwrap_or(Path::new(".")),
                    true,
                )?;
                if context.dir.try_exists(&destination)? {
                    return Err(safety::SafetyError::Collision(destination).into());
                }
                context.dir.rename(source, &context.dir, destination)?;
                context.progress(0, 1)
            }
            Step::Move { .. } => Err(ExecError::UnsupportedStep {
                step: "move from original input (source retention is managed after commit)".into(),
            }),
            Step::Extract {
                from,
                select,
                strip,
                to,
            } => self
                .extractor
                .extract(from, select, strip.as_deref(), to, context),
            Step::SetExecutable { path } => {
                let path = relative(path)?;
                subdir(&context.dir, path.parent().unwrap_or(Path::new(".")), false)?;
                if context.dir.symlink_metadata(&path)?.is_symlink() {
                    return Err(safety::SafetyError::WriteThroughLink(path).into());
                }
                let file = context.dir.open_with(path, OpenOptions::new().read(true))?;
                #[cfg(unix)]
                {
                    use cap_std::fs::PermissionsExt;
                    let mut permissions = file.metadata()?.permissions();
                    permissions.set_mode(safety::safe_mode(permissions.mode()) | 0o111);
                    file.set_permissions(permissions)?;
                }
                #[cfg(not(unix))]
                let _ = file;
                context.check_cancelled()
            }
            Step::Symlink { path, target } => {
                let mut destination = context.open_destination(Path::new("."), 0)?;
                destination.symlink(text(path)?, text(target)?, 0)?;
                context.progress(0, 1)
            }
            Step::Run {
                program,
                args,
                env,
                runtime,
                timeout_secs,
                expect,
            } => {
                if *timeout_secs == 0 {
                    return Err(ExecError::InvalidRequest {
                        message: "zero process timeout".into(),
                    });
                }
                let command = ProcessCommand {
                    program: self.program(program)?,
                    args: self.runtime_args(args, runtime)?,
                    env: env.clone(),
                    cwd: self.game_path.into(),
                };
                self.run(
                    command,
                    runtime,
                    Duration::from_secs(*timeout_secs),
                    self.observer,
                    context,
                )?;
                for path in expect {
                    let path = relative(path)?;
                    subdir(&context.dir, path.parent().unwrap_or(Path::new(".")), false)
                        .map_err(|_| ExecError::MissingExpected { path: path.clone() })?;
                    if !context.dir.try_exists(&path)? {
                        return Err(ExecError::MissingExpected { path });
                    }
                }
                context.check_cancelled()
            }
            Step::RunInteractive {
                program,
                args,
                env,
                runtime,
                target_dir,
            } => {
                subdir(&context.dir, target_dir, true)?;
                let command = ProcessCommand {
                    program: self.program(program)?,
                    args: self.runtime_args(args, runtime)?,
                    env: env.clone(),
                    cwd: self.game_path.into(),
                };
                let watch = super::interactive::Observation::start(
                    self.request,
                    &self.config.interactive,
                    context,
                    &command,
                    program,
                    runtime,
                )?;
                self.run(
                    command,
                    runtime,
                    self.config.interactive_timeout,
                    self.observer,
                    context,
                )?;
                watch.finish(self.request, context, self.journal)
            }
            Step::Delegate {
                tool,
                args,
                uninstall,
                only_if,
            } => {
                if let Some(check) = only_if
                    && !journal::needed(
                        &**journal::runtime(self.runtimes, &RuntimeSpec::Native)?,
                        &self.check(check),
                    )
                {
                    return context.check_cancelled();
                }
                if self.compensate_delegates && uninstall.is_empty() {
                    return Err(ExecError::InvalidRequest {
                        message: "delegate needs recorded uninstall commands".into(),
                    });
                }
                let uninstall = uninstall
                    .iter()
                    .filter(|_| self.compensate_delegates)
                    .map(|command| {
                        Ok((
                            ProcessCommand {
                                program: command.tool.clone().into(),
                                args: self.args(&command.args)?,
                                env: Default::default(),
                                cwd: self.request.root.clone(),
                            },
                            command.only_if.as_ref().map(|check| self.check(check)),
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                // The package manager records its own files, so nothing
                // outside the root is watched.
                self.run(
                    ProcessCommand {
                        program: tool.into(),
                        args: self.args(args)?,
                        env: Default::default(),
                        cwd: self.request.root.clone(),
                    },
                    &RuntimeSpec::Native,
                    self.config.delegate_timeout,
                    &NoObserver,
                    context,
                )?;
                // Only an install that happened is undone: a package manager
                // that failed changed nothing of ours, and its uninstall
                // would remove a version that was installed before.
                for (command, only_if) in uninstall {
                    self.journal.record(JournalEntry::Compensation {
                        runtime: RuntimeSpec::Native,
                        command,
                        only_if,
                    })?;
                }
                Ok(())
            }
            Step::RegistryImport { runtime, entries } => {
                self.registry_import(runtime, entries, context)
            }
            Step::Mount { .. } | Step::Unmount { .. } => Err(ExecError::UnsupportedStep {
                step: "mount/registry operations require their OS format adapter".into(),
            }),
        }
    }

    /// Inside a Proton/Wine prefix the game directory is `G:\Game` (#45), so
    /// `INSTALL_DIR` expands to it; program and working paths stay host paths.
    fn runtime_args(&self, args: &[String], runtime: &RuntimeSpec) -> Result<Vec<String>> {
        match runtime {
            RuntimeSpec::ProtonPrefix { .. } | RuntimeSpec::WinePrefix { .. } => {
                self.expand(args, crate::runtime::proton::GAME_DIR)
            }
            RuntimeSpec::Native => self.args(args),
        }
    }

    /// A delegate's check, found on PATH like its tool. Its arguments are
    /// literal: it asks about the host, never about this install's files.
    fn check(&self, check: &crate::DelegateCheck) -> ProcessCommand {
        ProcessCommand {
            program: check.tool.clone().into(),
            args: check.args.clone(),
            env: Default::default(),
            cwd: self.request.root.clone(),
        }
    }

    fn args(&self, args: &[String]) -> Result<Vec<String>> {
        self.expand(args, &self.install_dir.to_string_lossy())
    }

    fn expand(&self, args: &[String], install_dir: &str) -> Result<Vec<String>> {
        args.iter()
            .map(|arg| {
                if let Some(id) = arg
                    .strip_prefix("{caskade.source:")
                    .and_then(|s| s.strip_suffix('}'))
                {
                    let source = std::path::absolute(source_path(
                        &self.request.sources,
                        &crate::ArtifactId(id.into()),
                    )?)?;
                    source
                        .to_str()
                        .map(str::to_owned)
                        .ok_or_else(|| ExecError::InvalidRequest {
                            message: "non-UTF8 source argument".into(),
                        })
                } else {
                    Ok(arg.replace(INSTALL_DIR, install_dir))
                }
            })
            .collect()
    }

    fn source(&self, source: &SourcePath) -> Result<PathBuf> {
        match source {
            SourcePath::Game(path) => Ok(self.game_path.join(relative(path)?)),
            SourcePath::Artifact { artifact, path } => {
                let source = source_path(&self.request.sources, artifact)?;
                if path == Path::new(".") || path.as_os_str().is_empty() {
                    Ok(source.clone())
                } else {
                    Ok(source.join(relative(path)?))
                }
            }
        }
    }
    fn program(&self, program: &Program) -> Result<PathBuf> {
        match program {
            Program::Tool(tool) => Ok(tool.into()),
            Program::Source(source) => self.source(source),
        }
    }

    fn copy(&self, source: &SourcePath, target: &Path, context: &StepContext) -> Result<()> {
        match self.copy_input(source, context)? {
            CopyInput::Directory(source) => {
                let mut destination =
                    context.open_destination(target, self.config.limits.max_total_bytes)?;
                let mut links = vec![];
                copy_tree(
                    &source,
                    Path::new(""),
                    &mut destination,
                    context,
                    &mut links,
                )?;
                for (path, target) in links {
                    destination.symlink(text(&path)?, text(&target)?, 0)?;
                    context.progress(0, 1)?;
                }
            }
            CopyInput::File(source) => {
                let metadata = source.metadata()?;
                let mut destination = context.open_destination(Path::new("."), metadata.len())?;
                context.checkpoint(metadata.len())?;
                #[cfg(unix)]
                let mode = {
                    use cap_std::fs::PermissionsExt;
                    metadata.permissions().mode()
                };
                #[cfg(not(unix))]
                let mode = 0o644;
                let mut reader = context.progress_reader(source);
                destination.write_file(
                    text(target)?,
                    &mut reader,
                    Some(metadata.len()),
                    0,
                    mode,
                )?;
                reader.finish()?;
            }
        }
        Ok(())
    }

    /// `length` bytes of a file artifact from `offset`; a range past the end
    /// of the file is an error, not a short file.
    fn copy_range(
        &self,
        from: &crate::ArtifactId,
        offset: u64,
        length: u64,
        path: &Path,
        context: &StepContext,
    ) -> Result<()> {
        let CopyInput::File(source) = self.copy_input(
            &SourcePath::Artifact {
                artifact: from.clone(),
                path: PathBuf::from("."),
            },
            context,
        )?
        else {
            return Err(safety::SafetyError::SpecialFile.into());
        };
        let size = source.metadata()?.len();
        if offset.checked_add(length).is_none_or(|end| end > size) {
            return Err(ExecError::InvalidRequest {
                message: "copy range is outside its source".into(),
            });
        }
        let mut source = source.into_std();
        io::Seek::seek(&mut source, io::SeekFrom::Start(offset))?;
        context.checkpoint(length)?;
        let mut destination = context.open_destination(Path::new("."), length)?;
        let mut reader = context.progress_reader(source.take(length));
        destination.write_file(text(path)?, &mut reader, Some(length), 0, 0o644)?;
        reader.finish()
    }

    fn copy_input(&self, source: &SourcePath, context: &StepContext) -> Result<CopyInput> {
        match source {
            SourcePath::Game(path) => open_input(&context.dir, path),
            SourcePath::Artifact { artifact, path } => {
                let source = source_path(&self.request.sources, artifact)?;
                let metadata = fs::symlink_metadata(source)?;
                if metadata.is_symlink() {
                    return Err(safety::SafetyError::WriteThroughLink(source.clone()).into());
                }
                if metadata.is_dir() {
                    open_input(&safety::open_staging(source)?, path)
                } else if metadata.is_file()
                    && (path == Path::new(".") || path.as_os_str().is_empty())
                {
                    Ok(CopyInput::File(cap_std::fs::File::from_std(
                        fs::File::open(source)?,
                    )))
                } else {
                    Err(safety::SafetyError::SpecialFile.into())
                }
            }
        }
    }

    /// Replay registry entries into the step's Proton/Wine prefix (#46): a
    /// job-unique `.reg` in the game directory (drive `G:` there), imported
    /// with `regedit /S`, and removed afterwards whatever the outcome.
    fn registry_import(
        &self,
        runtime: &RuntimeSpec,
        entries: &[crate::RegistryEntry],
        context: &StepContext,
    ) -> Result<()> {
        if *runtime == RuntimeSpec::Native {
            return Err(ExecError::UnsupportedStep {
                step: "registry import never targets the host registry".into(),
            });
        }
        let text = crate::runtime::registry::reg_file(entries);
        let name = format!(".caskade-registry-{}.reg", context.job_id());
        if context.dir.try_exists(&name)? {
            return Err(safety::SafetyError::Collision(name.into()).into());
        }
        let mut destination = context.open_destination(Path::new("."), text.len() as u64)?;
        destination.write_file(&name, text.as_bytes(), Some(text.len() as u64), 0, 0o644)?;
        let result = self.run(
            ProcessCommand {
                program: "regedit".into(),
                args: vec![
                    "/S".into(),
                    format!("{}\\{name}", crate::runtime::proton::GAME_DIR),
                ],
                env: Default::default(),
                cwd: self.game_path.into(),
            },
            runtime,
            Duration::from_secs(300),
            self.observer,
            context,
        );
        let removed = context.dir.remove_file(&name);
        result?;
        removed?;
        context.progress(0, 1)
    }

    fn run(
        &self,
        command: ProcessCommand,
        spec: &RuntimeSpec,
        timeout: Duration,
        observer: &dyn ProcessObserver,
        context: &StepContext,
    ) -> Result<()> {
        let runtime = Arc::clone(journal::runtime(self.runtimes, spec)?);
        context.checkpoint(0)?;
        let observation = observer.start(&command, self.journal, context)?;
        let result = (|| {
            let process = runtime.spawn(&command)?;
            let guard = ProcessGuard {
                runtime: runtime.clone(),
                process,
            };
            context.emit("process_started")?;
            let (send, receive) = mpsc::sync_channel(1);
            let waiter_runtime = runtime.clone();
            let waiter = std::thread::spawn(move || {
                let _ = send.send(waiter_runtime.wait(process, timeout));
            });
            let result = loop {
                if let Err(error) = context.checkpoint(0) {
                    let _ = runtime.kill(process);
                    break Err(error);
                }
                match receive.recv_timeout(Duration::from_millis(10)) {
                    Ok(result) => break result.map_err(Into::into),
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        break Err(ExecError::Worker {
                            message: "runtime waiter disconnected".into(),
                        });
                    }
                }
            };
            // kill terminates descendants even when the parent has exited.
            drop(guard);
            waiter.join().map_err(|_| ExecError::Worker {
                message: "runtime waiter panicked".into(),
            })?;
            let exit = result?;
            if exit.code != 0 {
                return Err(ExecError::ProcessFailed { code: exit.code });
            }
            context.emit("process_completed")
        })();
        let observed = observation.finish(self.journal);
        observed?;
        result
    }
}

struct ProcessGuard {
    runtime: Arc<dyn Runtime>,
    process: crate::runtime::ProcessId,
}
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        let _ = self.runtime.kill(self.process);
    }
}

fn copy_tree(
    source: &Dir,
    base: &Path,
    destination: &mut SafeDir,
    context: &StepContext,
    links: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<()> {
    let mut entries = source
        .entries()?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        context.checkpoint(0)?;
        let name = entry.file_name();
        let path = base.join(&name);
        let metadata = source.symlink_metadata(&name)?;
        if metadata.is_symlink() {
            links.push((path, source.read_link(&name)?));
        } else if metadata.is_dir() {
            destination.create_dir(text(&path)?, 0)?;
            copy_tree(&source.open_dir(name)?, &path, destination, context, links)?;
        } else if metadata.is_file() {
            context.checkpoint(metadata.len())?;
            let mut reader = context.progress_reader(source.open(name)?);
            #[cfg(unix)]
            let permissions = {
                use cap_std::fs::PermissionsExt;
                metadata.permissions().mode()
            };
            #[cfg(not(unix))]
            let permissions = 0o644;
            destination.write_file(
                text(&path)?,
                &mut reader,
                Some(metadata.len()),
                0,
                permissions,
            )?;
            reader.finish()?;
        } else {
            return Err(safety::SafetyError::SpecialFile.into());
        }
    }
    Ok(())
}

fn text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| ExecError::InvalidRequest {
        message: "non-UTF8 plan path".into(),
    })
}
enum CopyInput {
    Directory(Dir),
    File(cap_std::fs::File),
}

fn open_input(root: &Dir, path: &Path) -> Result<CopyInput> {
    if path == Path::new(".") || path.as_os_str().is_empty() {
        return Ok(CopyInput::Directory(root.try_clone()?));
    }
    let path = relative(path)?;
    let parent = subdir(root, path.parent().unwrap_or(Path::new(".")), false)?;
    let name = Path::new(path.file_name().ok_or(safety::SafetyError::EmptyName)?);
    let metadata = parent.symlink_metadata(name)?;
    if metadata.is_symlink() {
        return Err(safety::SafetyError::WriteThroughLink(path).into());
    }
    if metadata.is_dir() {
        Ok(CopyInput::Directory(parent.open_dir(name)?))
    } else if metadata.is_file() {
        Ok(CopyInput::File(parent.open(name)?))
    } else {
        Err(safety::SafetyError::SpecialFile.into())
    }
}
