//! Why an install stopped and what can be done about it (#158).
//!
//! [`classify`] turns any pipeline error into a [`Failure`]: what went wrong
//! ([`FailureKind`]), at which [`FailureStage`], with which file, whether
//! retrying on its own may help, and the [`RecoveryAction`]s that suit the
//! cause. It walks the error's causes for the installer's own errors and
//! I/O errors, then looks at the filesystem itself (is the source still
//! there, is the storage root still reachable), because a file can vanish
//! between a preflight check and its use, and an error that crossed a
//! boundary as text has lost its cause.
//!
//! Security software is named only as a possibility
//! ([`SecurityNote::Possible`]) unless the OS said so: Windows reports a
//! file blocked or removed by an antivirus as `ERROR_VIRUS_INFECTED` /
//! `ERROR_VIRUS_DELETED` ([`SecurityNote::Confirmed`]). Nothing here, or
//! anywhere in Caskade, disables security software, adds exclusions or
//! restores quarantined files.
//!
//! Diagnostics are redacted: URLs lose their query, fragment and
//! credentials, which may carry tokens.

use std::{
    error::Error,
    io,
    path::{Path, PathBuf},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::exec::ExecError;

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    /// The input file or folder (or the downloaded file) is gone.
    MissingSource,
    /// A file the install wrote on its way (a partial download, an
    /// extracted or staged file) is gone.
    MissingStaged,
    /// The install finished without a file it needs, e.g. its launch target.
    MissingOutput,
    /// The OS refused access to a file or folder.
    AccessDenied,
    /// Another program has the file open; usually passes.
    Locked,
    /// The content isn't what was expected: a checksum mismatch.
    Integrity,
    /// The storage root, or the drive it is on, can't be reached.
    StorageDisconnected,
    /// Not enough free space.
    DiskFull,
    /// The download failed: the network or the server.
    Download,
    /// Caskade has no way to install the input: a kind of file or folder it
    /// doesn't install, a build for another system, a missing runner or an
    /// installer that may not run. Its [`Reason`](crate::Reason) is the
    /// detail ([`Failure::unsupported`]).
    Unsupported,
    Other,
}

/// Where in the install it went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub enum FailureStage {
    Download,
    /// Reading and inspecting the input.
    Inspect,
    /// Extracting files or running the installer.
    Install,
    /// Finding the game in what was installed and recording it.
    Finalize,
    /// Moving the finished install into place, and checking it there.
    Commit,
}

/// Whether security software may be involved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub enum SecurityNote {
    /// A file vanished or was refused for no reason Caskade can see;
    /// security software is one possible cause.
    Possible,
    /// The OS said security software blocked or removed the file.
    Confirmed,
}

/// What the user can do about a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryAction {
    /// Run again, keeping a valid partial or finished download.
    Retry,
    /// Run again after discarding the job's download.
    Redownload,
    /// Install from another file or folder.
    LocateSource,
    /// Install into another storage root.
    ChooseDestination,
    Cancel,
}

/// A classified failure, safe to show: no credentials or tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub kind: FailureKind,
    pub stage: FailureStage,
    /// The file or folder concerned, when known.
    pub path: Option<PathBuf>,
    /// Retrying unchanged may work (network trouble, a file in use).
    pub transient: bool,
    pub security: Option<SecurityNote>,
    /// A plain-language explanation.
    pub summary: String,
    /// The error and its causes, redacted.
    pub detail: String,
    /// Suitable recovery actions, most useful first.
    pub actions: Vec<RecoveryAction>,
    /// How many runs of this job in a row failed this way (callers count).
    pub occurrences: u32,
    /// Unix milliseconds of a scheduled automatic retry (callers set it).
    pub retry_at_ms: Option<i64>,
}

/// What the caller knows when classifying.
#[derive(Debug, Clone, Copy)]
pub struct Probe<'a> {
    /// The stage that was running; a download error always means
    /// [`FailureStage::Download`].
    pub stage: FailureStage,
    /// The input file or folder (for a URL, the downloaded file), when known.
    pub source: Option<&'a Path>,
    /// Whether the input is a URL, so a lost source can be downloaded again.
    pub remote: bool,
    /// The storage root being installed into.
    pub root: Option<&'a Path>,
}

/// Classifies `error`; see the module docs.
pub fn classify(error: &(dyn Error + 'static), probe: &Probe<'_>) -> Failure {
    // Only a download error (the `fetch` feature) changes the stage.
    #[cfg_attr(not(feature = "fetch"), allow(unused_mut))]
    let mut stage = probe.stage;
    let mut found: Option<(FailureKind, Option<PathBuf>)> = None;
    let mut transient = false;
    let mut confirmed = false;
    let mut not_found = false;
    let mut consider = |kind: FailureKind, path: Option<PathBuf>| {
        if found
            .as_ref()
            .is_none_or(|(current, _)| rank(kind) > rank(*current))
        {
            found = Some((kind, path));
        }
    };
    for cause in causes(error) {
        #[cfg(feature = "fetch")]
        if let Some(fetch) = cause.downcast_ref::<crate::fetch::FetchError>() {
            use crate::fetch::FetchError;
            stage = FailureStage::Download;
            match fetch {
                FetchError::ChecksumMismatch => consider(FailureKind::Integrity, None),
                FetchError::DiskFull { .. } => consider(FailureKind::DiskFull, None),
                // The I/O error underneath says more.
                FetchError::Io(_) | FetchError::Cancelled => {}
                other => {
                    transient |= other.is_transient();
                    consider(FailureKind::Download, None);
                }
            }
        }
        if let Some(exec) = cause.downcast_ref::<ExecError>() {
            match exec {
                ExecError::DiskFull { .. } => consider(FailureKind::DiskFull, None),
                ExecError::MissingSource { .. } => consider(FailureKind::MissingSource, None),
                ExecError::MissingExpected { path } => {
                    consider(FailureKind::MissingOutput, Some(path.clone()));
                }
                _ => {}
            }
        }
        if let Some(io) = cause.downcast_ref::<io::Error>() {
            match io_kind(io) {
                Some(IoKind::Kind(kind)) => {
                    transient |= kind == FailureKind::Locked;
                    consider(kind, None);
                }
                Some(IoKind::Security) => {
                    confirmed = true;
                    consider(FailureKind::AccessDenied, None);
                }
                Some(IoKind::NotFound) => not_found = true,
                None => {}
            }
        }
    }
    // What the filesystem says now outranks what the error text implies.
    if let Some(root) = probe.root
        && !reachable(root)
    {
        consider(FailureKind::StorageDisconnected, Some(root.to_path_buf()));
    } else if let Some(source) = probe.source
        && std::fs::symlink_metadata(source)
            .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
    {
        consider(FailureKind::MissingSource, Some(source.to_path_buf()));
    }
    if not_found {
        consider(
            match stage {
                // The input, or the file downloaded for it.
                FailureStage::Inspect => FailureKind::MissingSource,
                FailureStage::Commit => FailureKind::MissingOutput,
                _ => FailureKind::MissingStaged,
            },
            None,
        );
    }
    let (kind, path) = found.unwrap_or((FailureKind::Other, None));
    let path = path.or_else(|| {
        matches!(kind, FailureKind::MissingSource | FailureKind::Integrity)
            .then(|| probe.source.map(Path::to_path_buf))
            .flatten()
    });
    let security = if confirmed {
        Some(SecurityNote::Confirmed)
    } else {
        possibly_security(kind).then_some(SecurityNote::Possible)
    };
    let mut failure = Failure {
        kind,
        stage,
        path,
        transient: transient && matches!(kind, FailureKind::Download | FailureKind::Locked),
        security,
        summary: String::new(),
        detail: redact(&chain(error)),
        actions: actions(kind, stage, probe.remote),
        occurrences: 1,
        retry_at_ms: None,
    };
    failure.summary = summary(&failure);
    failure
}

impl Failure {
    /// A failure found without an error: `kind` at `stage`, with `path`.
    pub fn new(
        kind: FailureKind,
        stage: FailureStage,
        path: Option<PathBuf>,
        remote: bool,
        detail: &str,
    ) -> Self {
        let mut failure = Self {
            kind,
            stage,
            path,
            transient: false,
            security: possibly_security(kind).then_some(SecurityNote::Possible),
            summary: String::new(),
            detail: redact(detail),
            actions: actions(kind, stage, remote),
            occurrences: 1,
            retry_at_ms: None,
        };
        failure.summary = summary(&failure);
        failure
    }

    /// The summary again, after `occurrences` changed.
    pub fn resummarize(&mut self) {
        self.summary = summary(self);
    }

    /// An install of `path` the installer has no way to do
    /// ([`InstallOutcome::Unsupported`](crate::InstallOutcome::Unsupported)),
    /// and why: the [`Reason`](crate::Reason) is kept as the detail.
    pub fn unsupported(reason: &crate::Reason, path: Option<PathBuf>) -> Self {
        Self::new(
            FailureKind::Unsupported,
            FailureStage::Inspect,
            path,
            false,
            &serde_json::to_string(reason).unwrap_or_default(),
        )
    }

    /// Why an [`FailureKind::Unsupported`] failure happened.
    pub fn reason(&self) -> Option<crate::Reason> {
        if self.kind != FailureKind::Unsupported {
            return None;
        }
        serde_json::from_str(&self.detail).ok()
    }
}

/// Whether an executor error is about the environment (the source, the
/// disk, access) rather than the strategy, so trying another strategy
/// would only fail the same way or run code the user didn't choose.
pub(crate) fn stops_fallback(error: &ExecError, sources: &[&Path]) -> bool {
    if matches!(error, ExecError::DiskFull { .. }) {
        return true;
    }
    // A program that couldn't start or failed is the strategy's problem;
    // another strategy may not need it.
    let program = matches!(
        error,
        ExecError::Runtime(_) | ExecError::ProcessFailed { .. }
    );
    let environmental = !program
        && causes(error).any(|cause| {
            cause
                .downcast_ref::<io::Error>()
                .and_then(io_kind)
                .is_some_and(|kind| !matches!(kind, IoKind::NotFound))
        });
    environmental
        || sources.iter().any(|source| {
            std::fs::symlink_metadata(source)
                .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
        })
}

/// Files that vanish or are refused for no visible reason: security
/// software is one possible cause, never stated as the cause.
fn possibly_security(kind: FailureKind) -> bool {
    matches!(
        kind,
        FailureKind::MissingSource
            | FailureKind::MissingStaged
            | FailureKind::MissingOutput
            | FailureKind::AccessDenied
            | FailureKind::Locked
    )
}

/// Which finding wins when an error says several things.
fn rank(kind: FailureKind) -> u8 {
    match kind {
        FailureKind::Other | FailureKind::Unsupported => 0,
        FailureKind::Download => 1,
        FailureKind::MissingStaged => 2,
        FailureKind::MissingOutput => 3,
        FailureKind::Locked => 4,
        FailureKind::AccessDenied => 5,
        FailureKind::MissingSource => 6,
        FailureKind::Integrity => 7,
        FailureKind::DiskFull => 8,
        FailureKind::StorageDisconnected => 9,
    }
}

enum IoKind {
    Kind(FailureKind),
    NotFound,
    /// Windows: blocked or removed by security software.
    #[cfg_attr(not(windows), allow(dead_code))]
    Security,
}

fn io_kind(error: &io::Error) -> Option<IoKind> {
    use FailureKind::*;
    #[cfg(windows)]
    match error.raw_os_error() {
        // ERROR_VIRUS_INFECTED, ERROR_VIRUS_DELETED
        Some(225 | 226) => return Some(IoKind::Security),
        // ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION, ERROR_USER_MAPPED_FILE
        Some(32 | 33 | 1224) => return Some(IoKind::Kind(Locked)),
        // ERROR_NOT_READY, ERROR_DEV_NOT_EXIST, ERROR_DEVICE_NOT_CONNECTED,
        // ERROR_NO_MEDIA_IN_DRIVE, ERROR_BAD_NETPATH, ERROR_NETNAME_DELETED
        Some(21 | 55 | 1167 | 1112 | 53 | 64) => return Some(IoKind::Kind(StorageDisconnected)),
        // ERROR_HANDLE_DISK_FULL, ERROR_DISK_FULL
        Some(39 | 112) => return Some(IoKind::Kind(DiskFull)),
        _ => {}
    }
    #[cfg(unix)]
    // EIO, ENXIO, ENODEV: a removed or failing drive.
    if let Some(5 | 6 | 19) = error.raw_os_error() {
        return Some(IoKind::Kind(StorageDisconnected));
    }
    Some(match error.kind() {
        io::ErrorKind::NotFound => IoKind::NotFound,
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
            IoKind::Kind(AccessDenied)
        }
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => IoKind::Kind(DiskFull),
        io::ErrorKind::ResourceBusy | io::ErrorKind::ExecutableFileBusy => IoKind::Kind(Locked),
        io::ErrorKind::StaleNetworkFileHandle => IoKind::Kind(StorageDisconnected),
        _ => return None,
    })
}

/// Whether a storage root can still be listed.
fn reachable(root: &Path) -> bool {
    std::fs::read_dir(root).is_ok()
}

fn actions(kind: FailureKind, stage: FailureStage, remote: bool) -> Vec<RecoveryAction> {
    use RecoveryAction::*;
    let source = if remote { Redownload } else { LocateSource };
    let mut actions = match kind {
        FailureKind::MissingSource | FailureKind::Integrity => vec![source],
        FailureKind::MissingStaged | FailureKind::MissingOutput => vec![Retry, source],
        FailureKind::AccessDenied | FailureKind::Locked => match stage {
            FailureStage::Download | FailureStage::Inspect => vec![Retry, source],
            _ => vec![Retry, ChooseDestination],
        },
        FailureKind::StorageDisconnected | FailureKind::DiskFull => {
            vec![Retry, ChooseDestination]
        }
        FailureKind::Download => vec![Retry, Redownload],
        // The same input does the same again: only another one helps.
        FailureKind::Unsupported => vec![LocateSource],
        FailureKind::Other => vec![Retry],
    };
    if remote && kind == FailureKind::MissingSource {
        // The download is gone; a plain retry downloads it again.
        actions = vec![Redownload];
    }
    actions.push(Cancel);
    actions
}

fn summary(failure: &Failure) -> String {
    let file = failure
        .path
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| format!("“{}”", name.to_string_lossy()));
    let stage = match failure.stage {
        FailureStage::Download => "while downloading",
        FailureStage::Inspect => "while reading the installer",
        FailureStage::Install => "while installing",
        FailureStage::Finalize => "while finishing the install",
        FailureStage::Commit => "after installing",
    };
    let file_or = |fallback: &str| file.clone().unwrap_or_else(|| fallback.to_owned());
    let mut text = match failure.kind {
        FailureKind::MissingSource => format!(
            "{} is no longer there ({stage}). It may have been moved, deleted or on a drive that was removed.",
            file_or("The installer")
        ),
        FailureKind::MissingStaged => {
            format!("A file Caskade had just written disappeared {stage}.")
        }
        FailureKind::MissingOutput => format!(
            "{} is missing from the install, so the game was not added to the library.",
            file_or("A file the game needs")
        ),
        FailureKind::AccessDenied => format!(
            "Access to {} was refused {stage}.",
            file_or("a file Caskade needs")
        ),
        FailureKind::Locked => format!(
            "{} is in use by another program {stage}.",
            file_or("A file Caskade needs")
        ),
        FailureKind::Integrity => format!(
            "{} isn't what was expected: its checksum doesn't match. It was discarded.",
            file_or("The download")
        ),
        FailureKind::StorageDisconnected => format!(
            "The storage location {} can't be reached. Reconnect the drive or choose another location.",
            failure
                .path
                .as_deref()
                .map_or_else(String::new, |path| format!("“{}”", path.display()))
        ),
        FailureKind::DiskFull => {
            "There isn't enough free space to finish. Free some space or choose another location."
                .into()
        }
        // Torrents (#159): nobody to download from, or metadata Caskade
        // can't use.
        FailureKind::Download if failure.detail.contains("no BitTorrent peers") => {
            "No one sharing this torrent sent anything, so the download can't go on. It may have no active seeders right now; try again later.".into()
        }
        FailureKind::Download if failure.detail.contains("unusable torrent") => {
            "This torrent can't be used: its metadata is damaged or of a kind Caskade doesn't support.".into()
        }
        FailureKind::Download if failure.transient => {
            "The download failed because the network or the server had trouble.".into()
        }
        FailureKind::Download => "The download failed.".into(),
        FailureKind::Unsupported => {
            let file = file_or("The file");
            match failure.reason() {
                Some(crate::Reason::UnsupportedFormat {
                    format: crate::Format::Directory,
                }) => format!(
                    "{file} has no game program Caskade recognises, so it wasn't installed. Choose the game's installer or the folder with its program."
                ),
                Some(crate::Reason::PlatformMismatch { .. }) => format!(
                    "{file} is made for another system, so it wasn't installed. Choose a version for this one."
                ),
                Some(crate::Reason::MissingRuntime { .. }) => format!(
                    "{file} needs a program to run it that this device doesn't have, so it wasn't installed."
                ),
                Some(crate::Reason::ExecutionDisabled) => format!(
                    "{file} has to run its installer, and running installers is turned off in Settings."
                ),
                _ => format!(
                    "{file} isn't a kind of installer, archive or game folder Caskade can install, so it wasn't installed."
                ),
            }
        }
        FailureKind::Other => format!("The install stopped {stage}."),
    };
    match failure.security {
        Some(SecurityNote::Confirmed) => text.push_str(
            " Your security software reported a threat in this file and blocked or removed it. Caskade won't work around it; check the security software's history if you trust the file.",
        ),
        Some(SecurityNote::Possible) if failure.occurrences > 1 => text.push_str(
            " This happened again. Security software may be removing or blocking the file; check its history before trying again.",
        ),
        Some(SecurityNote::Possible) => text.push_str(
            " Security software can cause this, but Caskade can't tell for sure.",
        ),
        None => {}
    }
    text
}

fn causes<'a>(
    error: &'a (dyn Error + 'static),
) -> impl Iterator<Item = &'a (dyn Error + 'static)> + 'a {
    std::iter::successors(Some(error), |error: &&'a (dyn Error + 'static)| {
        (*error).source()
    })
}

/// The error and its causes, each once.
fn chain(error: &(dyn Error + 'static)) -> String {
    let mut text = String::new();
    for cause in causes(error) {
        let message = cause.to_string();
        if !text.contains(&message) {
            if !text.is_empty() {
                text.push_str(": ");
            }
            text.push_str(&message);
        }
    }
    text
}

/// `text` with every URL's credentials, query and fragment removed.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| rest.find(scheme))
        .min()
    {
        out.push_str(&rest[..start]);
        let end = rest[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')'))
            .map_or(rest.len(), |end| start + end);
        let candidate = &rest[start..end];
        match url::Url::parse(candidate) {
            Ok(mut url) => {
                let _ = url.set_username("");
                let _ = url.set_password(None);
                url.set_query(None);
                url.set_fragment(None);
                out.push_str(url.as_str());
            }
            Err(_) => out.push_str("<url>"),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(stage: FailureStage) -> Probe<'static> {
        Probe {
            stage,
            source: None,
            remote: false,
            root: None,
        }
    }

    fn io_error(kind: io::ErrorKind) -> ExecError {
        ExecError::Io(io::Error::new(kind, "fault"))
    }

    #[test]
    fn io_errors_name_their_cause_and_only_unconfirmed_security_software() {
        for (kind, expected, security) in [
            (
                io::ErrorKind::PermissionDenied,
                FailureKind::AccessDenied,
                Some(SecurityNote::Possible),
            ),
            (io::ErrorKind::StorageFull, FailureKind::DiskFull, None),
            (
                io::ErrorKind::ResourceBusy,
                FailureKind::Locked,
                Some(SecurityNote::Possible),
            ),
            (
                io::ErrorKind::NotFound,
                FailureKind::MissingStaged,
                Some(SecurityNote::Possible),
            ),
        ] {
            let failure = classify(&io_error(kind), &probe(FailureStage::Install));
            assert_eq!(failure.kind, expected, "{kind:?}");
            assert_eq!(failure.security, security, "{kind:?}");
            assert!(failure.actions.contains(&RecoveryAction::Cancel));
            if security == Some(SecurityNote::Possible) {
                assert!(
                    failure.summary.contains("can't tell for sure"),
                    "{failure:?}"
                );
            }
        }
        let locked = classify(
            &io_error(io::ErrorKind::ResourceBusy),
            &probe(FailureStage::Install),
        );
        assert!(locked.transient);
        let denied = classify(
            &io_error(io::ErrorKind::PermissionDenied),
            &probe(FailureStage::Commit),
        );
        assert!(!denied.transient);
        assert!(denied.actions.contains(&RecoveryAction::ChooseDestination));
    }

    #[cfg(windows)]
    #[test]
    fn windows_reports_quarantine_as_confirmed() {
        let error = ExecError::Io(io::Error::from_raw_os_error(225));
        let failure = classify(&error, &probe(FailureStage::Install));
        assert_eq!(failure.security, Some(SecurityNote::Confirmed));
        assert!(failure.summary.contains("reported a threat"));
    }

    #[test]
    fn a_vanished_source_is_found_whatever_the_error_says() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("setup.exe");
        let error: Box<dyn Error + Send + Sync> = "probe failed".into();
        let failure = classify(
            &*error,
            &Probe {
                stage: FailureStage::Install,
                source: Some(&source),
                remote: false,
                root: Some(temp.path()),
            },
        );
        assert_eq!(failure.kind, FailureKind::MissingSource);
        assert_eq!(failure.path.as_deref(), Some(source.as_path()));
        assert!(failure.summary.contains("setup.exe"));
        assert_eq!(
            failure.actions,
            [RecoveryAction::LocateSource, RecoveryAction::Cancel]
        );
        // A download that vanished is downloaded again instead.
        let failure = classify(
            &*error,
            &Probe {
                stage: FailureStage::Install,
                source: Some(&source),
                remote: true,
                root: Some(temp.path()),
            },
        );
        assert_eq!(
            failure.actions,
            [RecoveryAction::Redownload, RecoveryAction::Cancel]
        );
        assert_eq!(failure.security, Some(SecurityNote::Possible));
    }

    #[test]
    fn an_unreachable_root_outranks_everything_else() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("unplugged");
        let failure = classify(
            &ExecError::DiskFull {
                available: 0,
                required: 1,
            },
            &Probe {
                stage: FailureStage::Install,
                source: None,
                remote: false,
                root: Some(&root),
            },
        );
        assert_eq!(failure.kind, FailureKind::StorageDisconnected);
        assert!(failure.actions.contains(&RecoveryAction::ChooseDestination));
    }

    #[test]
    fn disk_full_and_missing_output_keep_their_kind() {
        let full = classify(
            &ExecError::DiskFull {
                available: 1,
                required: 2,
            },
            &probe(FailureStage::Install),
        );
        assert_eq!(full.kind, FailureKind::DiskFull);
        assert!(!full.transient);
        let missing = classify(
            &ExecError::MissingExpected {
                path: "Game/game.exe".into(),
            },
            &probe(FailureStage::Commit),
        );
        assert_eq!(missing.kind, FailureKind::MissingOutput);
        assert!(missing.summary.contains("game.exe"));
    }

    #[test]
    fn repeats_are_named_in_the_summary() {
        let mut failure = classify(
            &io_error(io::ErrorKind::NotFound),
            &probe(FailureStage::Install),
        );
        failure.occurrences = 2;
        failure.resummarize();
        assert!(failure.summary.contains("happened again"), "{failure:?}");
    }

    #[test]
    fn urls_lose_their_secrets() {
        assert_eq!(
            redact("GET https://user:pw@cdn.test/game.zip?token=abc#sha256=1 failed"),
            "GET https://cdn.test/game.zip failed"
        );
        assert_eq!(redact("no url here"), "no url here");
        let error: Box<dyn Error + Send + Sync> =
            "download from http://host.test/a?sig=secret refused".into();
        let failure = classify(&*error, &probe(FailureStage::Download));
        assert!(!failure.detail.contains("secret"), "{failure:?}");
    }
}
