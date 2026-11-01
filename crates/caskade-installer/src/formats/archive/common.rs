use crate::{
    events::{EventSink, JobEvent, Phase},
    exec::{CancellationToken, StepContext},
    safety::{self, SafeDir},
};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("archive I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("unsafe archive entry: {0}")]
    Safety(#[from] safety::SafetyError),
    #[error("invalid archive: {0}")]
    Invalid(String),
    #[error("unsupported archive feature: {0}")]
    Unsupported(String),
    #[error("archive metadata or decoder memory limit exceeded")]
    MemoryLimit,
    #[error("archive extraction cancelled")]
    Cancelled,
    #[error("archive operation failed: {0}")]
    Execution(#[from] crate::exec::ExecError),
}
pub type Result<T> = std::result::Result<T, ArchiveError>;

impl ArchiveError {
    /// The execution error for this archive error: I/O, safety, cancellation
    /// and wrapped execution errors keep their kind; anything else becomes
    /// [`ExecError::Extract`](crate::exec::ExecError::Extract) with the text
    /// from `message`, so a format can keep its own error prefix.
    pub(crate) fn into_exec_error(
        self,
        message: impl FnOnce(Self) -> String,
    ) -> crate::exec::ExecError {
        use crate::exec::ExecError;
        match self {
            Self::Io(e) => ExecError::Io(e),
            Self::Safety(e) => ExecError::Safety(e),
            Self::Cancelled => ExecError::Cancelled,
            Self::Execution(e) => e,
            other => ExecError::Extract {
                message: message(other),
            },
        }
    }
}

impl From<ArchiveError> for crate::exec::ExecError {
    fn from(error: ArchiveError) -> Self {
        error.into_exec_error(|other| other.to_string())
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Hardlink,
}
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    specta::Type,
)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub mode: u32,
    pub target: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArchiveOptions {
    pub max_metadata_bytes: usize,
    pub max_entries: usize,
    pub max_decoder_bytes: u64,
    pub max_unpacked_bytes: u64,
    pub max_expansion_ratio: u64,
    pub max_link_bytes: usize,
    pub depth: usize,
    pub select: Vec<String>,
    pub strip: Option<PathBuf>,
    pub legacy_zip_encoding: safety::NameEncoding,
}
impl Default for ArchiveOptions {
    fn default() -> Self {
        Self {
            max_metadata_bytes: 16 * 1024 * 1024,
            max_entries: 100_000,
            max_decoder_bytes: 64 * 1024 * 1024,
            max_unpacked_bytes: 100 * 1024 * 1024 * 1024,
            max_expansion_ratio: 1000,
            max_link_bytes: 4096,
            depth: 0,
            select: vec![],
            strip: None,
            legacy_zip_encoding: safety::NameEncoding::Cp437,
        }
    }
}
impl ArchiveOptions {
    /// Options for an `Extract` step: its selection, clamped to the job's limits.
    pub fn for_step(select: &[String], strip: Option<&Path>, limits: &safety::Limits) -> Self {
        let defaults = Self::default();
        Self {
            select: select.to_vec(),
            strip: strip.map(Path::to_path_buf),
            max_entries: defaults.max_entries.min(limits.max_entries as usize),
            max_unpacked_bytes: defaults.max_unpacked_bytes.min(limits.max_total_bytes),
            max_expansion_ratio: defaults.max_expansion_ratio.min(limits.max_expansion_ratio),
            ..defaults
        }
    }
}

/// Called between bounded reads, including decompression of skipped entries.
/// Progress reports deltas after bytes have been written to the staging writer.
pub trait ExtractionControl {
    fn checkpoint(&mut self, next_bytes: u64) -> Result<()>;
    fn progress(&mut self, bytes: u64, files: u64) -> Result<()>;
}
#[derive(Default)]
pub struct NoopControl;
impl ExtractionControl for NoopControl {
    fn checkpoint(&mut self, _: u64) -> Result<()> {
        Ok(())
    }
    fn progress(&mut self, _: u64, _: u64) -> Result<()> {
        Ok(())
    }
}
impl ExtractionControl for &StepContext {
    fn checkpoint(&mut self, bytes: u64) -> Result<()> {
        StepContext::checkpoint(self, bytes).map_err(Into::into)
    }
    fn progress(&mut self, bytes: u64, files: u64) -> Result<()> {
        StepContext::progress(self, bytes, files).map_err(Into::into)
    }
}
pub struct EventProgress<'a> {
    pub job_id: &'a str,
    pub cancellation: &'a CancellationToken,
    pub events: &'a dyn EventSink,
    bytes: u64,
    files: u64,
}
impl<'a> EventProgress<'a> {
    pub fn new(
        job_id: &'a str,
        cancellation: &'a CancellationToken,
        events: &'a dyn EventSink,
    ) -> Self {
        Self {
            job_id,
            cancellation,
            events,
            bytes: 0,
            files: 0,
        }
    }
}
impl ExtractionControl for EventProgress<'_> {
    fn checkpoint(&mut self, _: u64) -> Result<()> {
        if self.cancellation.is_cancelled() {
            Err(ArchiveError::Cancelled)
        } else {
            Ok(())
        }
    }
    fn progress(&mut self, bytes: u64, files: u64) -> Result<()> {
        self.checkpoint(0)?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(safety::SafetyError::ByteLimit)?;
        self.files = self
            .files
            .checked_add(files)
            .ok_or(safety::SafetyError::EntryLimit)?;
        self.events.emit(JobEvent {
            job_id: self.job_id.into(),
            phase: Phase::Install,
            step: None,
            bytes: self.bytes,
            total_bytes: None,
            files: self.files,
            eta_secs: None,
            message: "extracting".into(),
        });
        self.checkpoint(0)
    }
}

pub(super) struct Checked<'a, R> {
    pub reader: R,
    pub control: &'a mut dyn ExtractionControl,
}
impl<R: Read> Read for Checked<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = buffer.len().min(64 * 1024);
        self.control
            .checkpoint(length as u64)
            .map_err(io::Error::other)?;
        self.reader.read(&mut buffer[..length])
    }
}

pub(super) struct SharedControl<'a, 'b>(pub &'a std::cell::RefCell<&'b mut dyn ExtractionControl>);
impl ExtractionControl for SharedControl<'_, '_> {
    fn checkpoint(&mut self, bytes: u64) -> Result<()> {
        self.0.borrow_mut().checkpoint(bytes)
    }
    fn progress(&mut self, bytes: u64, files: u64) -> Result<()> {
        self.0.borrow_mut().progress(bytes, files)
    }
}
struct WritingReader<'a, R> {
    reader: R,
    control: &'a mut dyn ExtractionControl,
    pending: u64,
}
impl<R: Read> Read for WritingReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.pending != 0 {
            self.control
                .progress(self.pending, 0)
                .map_err(io::Error::other)?;
            self.pending = 0;
        }
        let length = buffer.len().min(64 * 1024);
        self.control
            .checkpoint(length as u64)
            .map_err(io::Error::other)?;
        let count = self.reader.read(&mut buffer[..length])?;
        self.pending = count as u64;
        Ok(count)
    }
}

pub(super) struct Selection {
    globs: globset::GlobSet,
    empty: bool,
    strip: Option<PathBuf>,
}
impl Selection {
    pub fn new(options: &ArchiveOptions) -> Result<Self> {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in &options.select {
            builder.add(
                globset::Glob::new(pattern).map_err(|e| ArchiveError::Invalid(e.to_string()))?,
            );
        }
        let strip = options
            .strip
            .as_ref()
            .map(|p| {
                if p == Path::new(".") {
                    Ok(PathBuf::new())
                } else {
                    safety::sanitize_path(p.to_str().ok_or(safety::SafetyError::InvalidEncoding)?)
                }
            })
            .transpose()?;
        Ok(Self {
            globs: builder
                .build()
                .map_err(|e| ArchiveError::Invalid(e.to_string()))?,
            empty: options.select.is_empty(),
            strip,
        })
    }
    pub fn path(&self, name: &str) -> Result<Option<String>> {
        let safe = safety::sanitize_path(name)?;
        if !self.empty && !self.globs.is_match(&safe) {
            return Ok(None);
        }
        let path = if let Some(strip) = &self.strip {
            match safe.strip_prefix(strip) {
                Ok(path) => path,
                Err(_) => return Ok(None),
            }
        } else {
            &safe
        };
        if path.as_os_str().is_empty() {
            return Ok(None);
        }
        Ok(Some(
            path.to_str()
                .ok_or(safety::SafetyError::InvalidEncoding)?
                .replace('\\', "/"),
        ))
    }
    pub fn hardlink_target(&self, target: &str) -> Result<String> {
        self.path(target)?
            .ok_or_else(|| ArchiveError::Invalid("hardlink target is excluded by selection".into()))
    }
}

pub(super) fn link_contents(reader: impl Read, options: &ArchiveOptions) -> Result<String> {
    let mut bytes = Vec::new();
    reader
        .take(options.max_link_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > options.max_link_bytes {
        return Err(ArchiveError::MemoryLimit);
    }
    safety::decode_name(&bytes, safety::NameEncoding::Utf8).map_err(Into::into)
}

pub(super) struct FileMode {
    pub bits: u32,
    pub restore_exec: bool,
}

pub(super) fn write_stream(
    dir: &mut SafeDir,
    name: &str,
    mut reader: impl Read,
    size: u64,
    mode: FileMode,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    control.checkpoint(size)?;
    // Only a tiny prefix is buffered to restore executable modes on ZIPs made
    // on Windows. Full contents are always streamed through the safety writer.
    let mut prefix = [0_u8; 4];
    let mut length = 0;
    while length < prefix.len() {
        control.checkpoint(4)?;
        let count = reader.read(&mut prefix[length..])?;
        if count == 0 {
            break;
        }
        length += count;
    }
    let mode = if mode.restore_exec
        && (prefix[..length].starts_with(b"\x7fELF") || prefix[..length].starts_with(b"#!"))
    {
        mode.bits | 0o111
    } else {
        mode.bits
    };
    let stream = std::io::Cursor::new(&prefix[..length]).chain(reader);
    let mut checked = WritingReader {
        reader: stream,
        control,
        pending: 0,
    };
    // The next read reports the previous completed write. EOF accounts for
    // the final chunk, and the completion event increments the file counter.
    let result = dir.write_file(name, &mut checked, Some(size), options.depth, mode);
    if let Err(error) = result {
        return Err(error.into());
    }
    control.progress(0, 1)?;
    Ok(())
}

pub(super) fn finish_links(
    dir: &mut SafeDir,
    mut links: Vec<(String, String)>,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    // Framework links can precede their targets and may themselves form chains.
    // Retry only unresolved targets; no successful link is ever overwritten.
    while !links.is_empty() {
        let count = links.len();
        let mut pending = Vec::new();
        for (name, target) in links {
            control.checkpoint(0)?;
            match dir.symlink(&name, &target, options.depth) {
                Ok(_) => control.progress(0, 1)?,
                Err(safety::SafetyError::InvalidSymlinkTarget) => pending.push((name, target)),
                Err(error) => return Err(error.into()),
            }
        }
        if pending.len() == count {
            return Err(safety::SafetyError::InvalidSymlinkTarget.into());
        }
        links = pending;
    }
    Ok(())
}
