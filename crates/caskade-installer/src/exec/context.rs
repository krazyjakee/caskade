use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use cap_std::fs::Dir;
use tokio_util::sync::CancellationToken;

use crate::{
    HandlerId,
    disk::DiskSpace,
    events::{EventSink, JobEvent, Phase},
    safety::{self, SafeDir},
};

use super::{ExecConfig, ExecError, ExecRequest, Result};

#[derive(Default)]
pub(super) struct Progress {
    bytes: u64,
    files: u64,
}

/// Context shared with format extractors. Absolute writable paths are not
/// exposed: destination writers are opened below the job's confined capability.
#[derive(Clone)]
pub struct StepContext {
    pub limits: safety::Limits,
    pub case_sensitive: bool,
    pub(super) dir: Arc<Dir>,
    pub(super) disk_root: PathBuf,
    pub(super) working_dir: PathBuf,
    pub(super) final_dir: PathBuf,
    pub(super) cancellation: CancellationToken,
    pub(super) sink: Arc<dyn EventSink>,
    pub(super) disk: Arc<dyn DiskSpace>,
    pub(super) margin: u64,
    pub(super) job: String,
    pub(super) handler: Option<HandlerId>,
    pub(super) step: Option<u32>,
    pub(super) total: Option<u64>,
    pub(super) progress: Arc<Mutex<Progress>>,
    pub(super) started: Instant,
}

impl StepContext {
    pub fn job_id(&self) -> &str {
        &self.job
    }

    /// Handler for the active candidate. In-place recorded steps have no
    /// candidate and return None. Hybrid extractors can route fallback attempts
    /// without changing the serialized Extract step or artifact identifier.
    pub fn handler_id(&self) -> Option<&HandlerId> {
        self.handler.as_ref()
    }

    /// Read-only reference anchor for generated launchers/configuration. This
    /// names the collision-adjusted final game tree, never private staging;
    /// destination writes must still use the confined writer capability.
    pub fn final_destination_path(&self, path: &Path) -> Result<PathBuf> {
        self.check_cancelled()?;
        if path == Path::new(".") || path.as_os_str().is_empty() {
            return Ok(self.final_dir.clone());
        }
        Ok(self.final_dir.join(relative(path)?))
    }

    /// Private quarantine for a trusted external extractor such as 7zz. Drop
    /// before returning from extraction; reimport validated entries through
    /// `open_destination`, never rename this scratch tree into the payload.
    pub fn create_scratch(&self) -> Result<tempfile::TempDir> {
        self.checkpoint(0)?;
        let scratch = tempfile::Builder::new()
            .prefix(".caskade-scratch-")
            .tempdir_in(&self.working_dir)?;
        self.check_cancelled()?;
        Ok(scratch)
    }

    pub fn checkpoint(&self, next_bytes: u64) -> Result<()> {
        self.check_cancelled()?;
        let available = self.disk.available_bytes(&self.disk_root)?;
        let required = next_bytes
            .checked_add(self.margin)
            .ok_or(ExecError::DiskFull {
                available,
                required: u64::MAX,
            })?;
        if available < required {
            return Err(ExecError::DiskFull {
                available,
                required,
            });
        }
        Ok(())
    }

    pub(super) fn check_cancelled(&self) -> Result<()> {
        if self.cancellation.is_cancelled() {
            Err(ExecError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Incremental counters (not cumulative totals). Cancellation is checked
    /// after emitting so cancellation from any progress callback is observed.
    pub fn progress(&self, bytes_delta: u64, files_delta: u64) -> Result<()> {
        self.check_cancelled()?;
        {
            let mut progress = self.progress.lock().expect("executor progress poisoned");
            progress.bytes = progress
                .bytes
                .checked_add(bytes_delta)
                .ok_or(safety::SafetyError::ByteLimit)?;
            progress.files = progress
                .files
                .checked_add(files_delta)
                .ok_or(safety::SafetyError::EntryLimit)?;
            if progress.bytes > self.limits.max_total_bytes {
                return Err(safety::SafetyError::ByteLimit.into());
            }
            if progress.files > self.limits.max_entries {
                return Err(safety::SafetyError::EntryLimit.into());
            }
        }
        self.emit("progress")
    }

    pub(super) fn emit(&self, message: &str) -> Result<()> {
        self.check_cancelled()?;
        let progress = self.progress.lock().expect("executor progress poisoned");
        let elapsed = self.started.elapsed().as_secs();
        let eta = self.total.and_then(|total| {
            if progress.bytes == 0 || elapsed == 0 {
                None
            } else {
                Some(total.saturating_sub(progress.bytes).saturating_mul(elapsed) / progress.bytes)
            }
        });
        let event = JobEvent {
            job_id: self.job.clone(),
            phase: Phase::Install,
            step: self.step,
            bytes: progress.bytes,
            total_bytes: self.total,
            files: progress.files,
            eta_secs: eta,
            message: message.into(),
        };
        drop(progress);
        self.sink.emit(event);
        self.check_cancelled()
    }

    /// `.` selects the whole game directory; every other component is validated
    /// and must not traverse a symlink. Limits also apply inside the safe writer.
    pub fn open_destination(&self, destination: &Path, packed_bytes: u64) -> Result<SafeDir> {
        self.checkpoint(0)?;
        let dir = subdir(&self.dir, destination, true)?;
        Ok(SafeDir::new(
            dir,
            self.limits,
            packed_bytes,
            self.case_sensitive,
        ))
    }

    /// Wrap an archive stream to check cancellation and space before each read.
    /// Call progress when the corresponding bytes have actually been written.
    pub fn checked_reader<'a, R: Read>(&'a self, reader: R) -> CheckedReader<'a, R> {
        CheckedReader {
            reader,
            context: self,
        }
    }

    /// A `checked_reader` that also reports each chunk as progress once the
    /// writer asks for the next one, i.e. after the chunk has been written.
    /// Call `finish` after the file is complete.
    pub(crate) fn progress_reader<'a, R: Read>(&'a self, reader: R) -> ProgressReader<'a, R> {
        ProgressReader {
            reader: self.checked_reader(reader),
            pending: 0,
        }
    }
}

/// A [`CheckedReader`] that reports progress; see [`StepContext::progress_reader`].
pub(crate) struct ProgressReader<'a, R> {
    reader: CheckedReader<'a, R>,
    pending: u64,
}
impl<R> ProgressReader<'_, R> {
    /// Report the last chunk's bytes and one completed file.
    pub fn finish(self) -> Result<()> {
        self.reader.context.progress(self.pending, 1)
    }
}
impl<R: Read> Read for ProgressReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.pending > 0 {
            self.reader
                .context
                .progress(std::mem::take(&mut self.pending), 0)
                .map_err(io::Error::other)?;
        }
        let count = self.reader.read(buffer)?;
        self.pending = count as u64;
        Ok(count)
    }
}

pub struct CheckedReader<'a, R> {
    reader: R,
    context: &'a StepContext,
}
impl<R: Read> Read for CheckedReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.context.check_cancelled().map_err(io::Error::other)?;
        let count = self.reader.read(buffer)?;
        self.context
            .checkpoint(count as u64)
            .map_err(io::Error::other)?;
        Ok(count)
    }
}

pub(super) fn subdir(root: &Dir, path: &Path, create: bool) -> Result<Dir> {
    let mut dir = root.try_clone()?;
    if path == Path::new(".") || path.as_os_str().is_empty() {
        return Ok(dir);
    }
    let safe = relative(path)?;
    for component in safe.components() {
        let name = Path::new(component.as_os_str());
        match dir.symlink_metadata(name) {
            Ok(metadata) if metadata.is_symlink() || !metadata.is_dir() => {
                return Err(safety::SafetyError::WriteThroughLink(safe).into());
            }
            Ok(_) => (),
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                dir.create_dir(name)?
            }
            Err(error) => return Err(error.into()),
        }
        dir = dir.open_dir(name)?;
    }
    Ok(dir)
}

pub(super) fn relative(path: &Path) -> Result<PathBuf> {
    safety::sanitize_path(path.to_str().ok_or_else(|| ExecError::InvalidRequest {
        message: "non-UTF8 plan path".into(),
    })?)
    .map_err(Into::into)
}

pub(super) fn new_context(
    dir: Dir,
    request: &ExecRequest,
    config: &ExecConfig,
    cancellation: CancellationToken,
    sink: Arc<dyn EventSink>,
    disk: Arc<dyn DiskSpace>,
    total: Option<u64>,
) -> StepContext {
    StepContext {
        limits: config.limits,
        case_sensitive: config.case_sensitive,
        dir: Arc::new(dir),
        disk_root: request.root.clone(),
        working_dir: request.root.join(".caskade/staging").join(&request.job_id),
        final_dir: request.root.join(&request.relative_dir),
        cancellation,
        sink,
        disk,
        margin: config.disk_margin_bytes,
        job: request.job_id.clone(),
        handler: None,
        step: None,
        total,
        progress: Arc::new(Mutex::new(Progress::default())),
        started: Instant::now(),
    }
}
