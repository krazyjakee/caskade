//! Official 7-Zip sidecar. List and validate before external extraction, keep
//! output quarantined inside staging, then import every entry through SafeDir.
//! The program is an explicitly configured trusted utility, never an installer.
mod adapter;
mod import;
mod listing;
mod process;

use crate::{
    ArtifactNode,
    formats::archive::{ArchiveError, ArchiveOptions, Entry, ExtractionControl, NoopControl},
    host::HostCaps,
    safety::{self, Limits, SafeDir},
};
pub use adapter::{HybridArchiveExtractor, SevenZipExtractor, SevenZipHandler};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum SevenZipError {
    #[error("7-Zip is not configured")]
    Unavailable,
    #[error("7-Zip binary or companion DLL does not match the pinned release")]
    UnpinnedBinary,
    #[error("7-Zip I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsafe 7-Zip entry: {0}")]
    Safety(#[from] safety::SafetyError),
    #[error("7-Zip extraction control failed: {0}")]
    Control(#[from] ArchiveError),
    #[error("7-Zip listing is invalid: {0}")]
    Listing(String),
    #[error("7-Zip metadata output exceeds its limit")]
    OutputLimit,
    #[error("7-Zip process timed out")]
    TimedOut,
    #[error("7-Zip archive is encrypted or needs a password")]
    Encrypted,
    #[error("7-Zip warning: {0}")]
    Warning(String),
    #[error("7-Zip fatal error: {0}")]
    Fatal(String),
    #[error("7-Zip command line error: {0}")]
    CommandLine(String),
    #[error("7-Zip ran out of memory")]
    OutOfMemory,
    #[error("7-Zip process was stopped")]
    Stopped,
    #[error("7-Zip exited with {code:?}: {details}")]
    Exit { code: Option<i32>, details: String },
}
pub type Result<T> = std::result::Result<T, SevenZipError>;
impl SevenZipError {
    pub fn exit(code: Option<i32>, details: &str) -> Self {
        let detail = details.to_lowercase();
        if detail.contains("wrong password")
            || detail.contains("password is incorrect")
            || detail.contains("encrypted archive")
        {
            return Self::Encrypted;
        }
        match code {
            Some(1) => Self::Warning(details.into()),
            Some(2) => Self::Fatal(details.into()),
            Some(7) => Self::CommandLine(details.into()),
            Some(8) => Self::OutOfMemory,
            Some(255) => Self::Stopped,
            _ => Self::Exit {
                code,
                details: details.into(),
            },
        }
    }
}

#[derive(Clone)]
pub struct SevenZip {
    program: PathBuf,
    options: ArchiveOptions,
    limits: Limits,
    case_sensitive: bool,
    timeout: Duration,
    progress: Option<Arc<dyn Fn(u8) + Send + Sync>>,
}
impl SevenZip {
    /// Explicit utility injection (including test doubles). Production callers
    /// should use pinned/from_host to require the verified official binary.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            options: ArchiveOptions::default(),
            limits: Limits::default(),
            case_sensitive: true,
            timeout: Duration::from_secs(600),
            progress: None,
        }
    }
    /// Validate the installed binary against the verified 26.05 packages. The
    /// Windows console also requires its matching pinned 7z.dll companion.
    pub fn pinned(program: impl Into<PathBuf>) -> Result<Self> {
        use sha2::{Digest, Sha256};
        fn digest(path: &Path) -> Result<String> {
            let mut hash = Sha256::new();
            std::io::copy(&mut std::fs::File::open(path)?, &mut hash)?;
            Ok(format!("{:x}", hash.finalize()))
        }
        let program = dunce::canonicalize(program.into())?;
        let checksum = digest(&program)?;
        let dll = match checksum.as_str() {
            "d32657a7a0e4cdc0a8aa57cafe474746944d76e861e753e9c902b9c9356fa886"
            | "17925963fe96bd0b441d1a139dbd2780b8894de0446fe2650ccdc65372167415"
            | "8e495531e1b2b8cb169ca583493922d4ab386007756418c15b95f07f642c35fb" => None,
            "94205cfc054dd3bbeb978905bc619a65b3f04e9e73bb2717841110b6f46d962e" => {
                Some("78bf6b668d75801e3aa180b4ccb16e6d8395fba80dec8a065055bb6f8dfabb82")
            }
            "08c3868b294fd12984bd1537a939ae7200ad695420dc65a2eeeec9f8fdf99772" => {
                Some("311d0a45182401e10c71a12b33a5c4a7171f6a8c1d67eeec9ef5dc2a5c962fe8")
            }
            _ => return Err(SevenZipError::UnpinnedBinary),
        };
        if let Some(expected) = dll
            && digest(&program.with_file_name("7z.dll"))? != expected
        {
            return Err(SevenZipError::UnpinnedBinary);
        }
        Ok(Self::new(program))
    }
    pub fn from_host(host: &HostCaps) -> Result<Self> {
        let path = host.seven_zip.as_ref().ok_or(SevenZipError::Unavailable)?;
        Ok(Self::pinned(path)?.with_case_sensitivity(host.filesystem.case_sensitive))
    }
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }
    pub fn with_options(mut self, options: ArchiveOptions) -> Self {
        self.options = options;
        self
    }
    pub fn with_case_sensitivity(mut self, sensitive: bool) -> Self {
        self.case_sensitive = sensitive;
        self
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    /// Percentage updates from -bsp1. Actual written-byte progress is emitted
    /// separately when quarantined files pass validation and are imported.
    pub fn with_progress(mut self, progress: Arc<dyn Fn(u8) + Send + Sync>) -> Self {
        self.progress = Some(progress);
        self
    }
    pub fn program(&self) -> &Path {
        &self.program
    }
    pub fn version_output(&self) -> Result<String> {
        let output = self.run(&["i".into()], None, &mut NoopControl)?;
        String::from_utf8(output)
            .map_err(|_| SevenZipError::Listing("non-UTF-8 version output".into()))
    }
    pub fn list(&self, input: &Path, control: &mut dyn ExtractionControl) -> Result<Vec<Entry>> {
        let (source, packed) = resolved(input)?;
        let args = [
            "l",
            "-slt",
            "-ba",
            "-sccUTF-8",
            "-bso1",
            "-bse2",
            "-bsp0",
            "-p-",
            "--",
        ]
        .map(OsString::from);
        let mut args = args.to_vec();
        args.push(source.into_os_string());
        let output = self.run(&args, None, control)?;
        let mut limits = self.limits;
        limits.max_entries = limits.max_entries.min(self.options.max_entries as u64);
        listing::parse(&output, limits, packed, self.case_sensitive)
    }
    pub fn parse_listing(&self, bytes: &[u8], packed_bytes: u64) -> Result<Vec<Entry>> {
        if bytes.len() > self.options.max_metadata_bytes {
            return Err(SevenZipError::OutputLimit);
        }
        let mut limits = self.limits;
        limits.max_entries = limits.max_entries.min(self.options.max_entries as u64);
        listing::parse(bytes, limits, packed_bytes, self.case_sensitive)
    }
    /// The caller supplies a private staging child as scratch_parent. An inner
    /// temporary directory is dropped on every success/error/cancellation path.
    pub fn extract(
        &self,
        input: &Path,
        scratch_parent: &Path,
        destination: &mut SafeDir,
        control: &mut dyn ExtractionControl,
    ) -> Result<Vec<ArtifactNode>> {
        let (source, _) = resolved(input)?;
        let entries = self.list(&source, control)?;
        let total = entries.iter().try_fold(0_u64, |sum, entry| {
            sum.checked_add(entry.size)
                .ok_or(safety::SafetyError::ByteLimit)
        })?;
        // Quarantine and the confined destination coexist during import.
        control.checkpoint(total.checked_mul(2).ok_or(safety::SafetyError::ByteLimit)?)?;
        let scratch = tempfile::Builder::new()
            .prefix("sevenzip-")
            .tempdir_in(scratch_parent)?;
        let mut args: Vec<OsString> = [
            "x",
            "-y",
            "-aos",
            "-bb0",
            "-sccUTF-8",
            "-bso1",
            "-bse2",
            "-bsp1",
            "-p-",
            "-spod",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        let mut output = OsString::from("-o");
        output.push(scratch.path());
        args.push(output);
        args.push("--".into());
        args.push(source.into_os_string());
        let mut extracting = self.clone();
        extracting.limits.max_total_bytes = extracting.limits.max_total_bytes.min(total);
        extracting.limits.max_entries = extracting
            .limits
            .max_entries
            .min(self.options.max_entries as u64);
        extracting.run(&args, Some(scratch.path()), control)?;
        import::import(
            scratch.path(),
            &entries,
            destination,
            &self.options,
            self.limits,
            control,
        )?;
        // Re-probing only reads completed confined output; no utility is run.
        let mut engine = crate::probe::ProbeEngine::default();
        engine.register(Box::new(super::ArchiveProbe));
        let mut artifacts = Vec::new();
        for path in destination.extracted_files() {
            control.checkpoint(0)?;
            let name = path.to_str().ok_or(safety::SafetyError::InvalidEncoding)?;
            let mut file = destination.open_read(name)?;
            let mut node = engine
                .probe_reader(&mut file, path)
                .map_err(|e| SevenZipError::Listing(e.to_string()))?
                .root;
            node.id = crate::ArtifactId(format!("extracted:{}", artifacts.len()));
            artifacts.push(node);
        }
        Ok(artifacts)
    }
}
pub(super) fn resolved(input: &Path) -> Result<(PathBuf, u64)> {
    let resolved =
        crate::input::resolve_path(input).map_err(|e| SevenZipError::Listing(e.to_string()))?;
    let (primary, files) = match resolved {
        crate::input::ResolvedInput::File { path } => (path.clone(), vec![path]),
        crate::input::ResolvedInput::MultiPart { primary, files, .. } => (primary, files),
        _ => {
            return Err(SevenZipError::Listing(
                "sidecar input is a directory".into(),
            ));
        }
    };
    let packed = files.iter().try_fold(0_u64, |sum, path| {
        let size = path.metadata()?.len();
        sum.checked_add(size)
            .ok_or_else(|| std::io::Error::other("source size overflow"))
    })?;
    Ok((primary, packed))
}
