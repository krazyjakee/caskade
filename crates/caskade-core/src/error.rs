use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors that core operations return to the app edge.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// No home directory, so the per-OS app directories are unknown.
    #[error("cannot determine the application directories")]
    NoAppDirs,
    #[error("{0}")]
    Invalid(String),
    /// The signed-in account may not do that, or nobody is signed in
    /// (`crate::accounts::Permission`, #205). The text is for people.
    #[error("{0}")]
    Forbidden(String),
}

/// A network failure as an [`Error`], for people: what went wrong and why,
/// naming the host but not the URL (URLs can carry signed query
/// parameters). A host that can't be reached or doesn't answer in time is
/// [`unreachable`].
#[cfg(feature = "net")]
pub(crate) fn net(error: reqwest::Error) -> Error {
    use std::io::ErrorKind;
    let kind = if error.is_timeout() {
        ErrorKind::TimedOut
    } else if error.is_connect() {
        ErrorKind::NotConnected
    } else {
        ErrorKind::Other
    };
    // Kept as a message, so a stored failure (an update check's) is
    // translated when it is read (#214).
    Error::Io(std::io::Error::new(
        kind,
        crate::i18n::TextError(net_text(error)),
    ))
}

/// [`net`]'s text in the language in use.
#[cfg(feature = "net")]
pub(crate) fn describe_net(error: reqwest::Error) -> String {
    net_text(error).render()
}

/// [`net`]'s text: `<host> timed out`, `can't reach <host>: <why>`, or
/// reqwest's message followed by its causes.
#[cfg(feature = "net")]
fn net_text(error: reqwest::Error) -> crate::i18n::Text {
    let host: crate::i18n::Text = match error.url().and_then(|url| url.host_str()) {
        Some(host) => host.into(),
        None => crate::msg!("the server").into(),
    };
    if error.is_timeout() {
        return crate::msg!("{host} timed out", host = host).into();
    }
    let error = error.without_url();
    let mut causes: Vec<String> = Vec::new();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        causes.push(cause.to_string());
        source = cause.source();
    }
    if error.is_connect() {
        // The innermost cause says why: refused, no such host, …
        return match causes.last() {
            Some(why) => crate::msg!("can't reach {host}: {why}", host = host, why = why),
            None => crate::msg!("can't reach {host}", host = host),
        }
        .into();
    }
    let mut text = error.to_string();
    for cause in causes {
        // Causes often repeat the one they wrap.
        if !text.contains(&cause) {
            text.push_str(": ");
            text.push_str(&cause);
        }
    }
    text.into()
}

/// Whether `error` is [`net`]'s for a host that couldn't be reached or
/// timed out: the network may be down, so other requests would fail too.
pub(crate) fn unreachable(error: &Error) -> bool {
    matches!(
        error,
        Error::Io(error) if matches!(
            error.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::NotConnected
        )
    )
}

// ── Structured reports (#160) ──────────────────────────────────────────────
//
// What crosses to the webview and into notifications when something fails:
// a stable code to match on (never the text), a sentence for people, the
// redacted causes for diagnostics, and what can be done about it. See
// `docs/TECH-SPEC.md`, Errors and recovery.

/// What kind of failure an [`ErrorReport`] is. Stable: the webview,
/// notifications and recovery code match on it, never on the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The request can't be done as asked: bad input, an unsupported case.
    Invalid,
    /// The signed-in account may not do that, or nobody is signed in.
    Forbidden,
    /// The state changed under the request (already running, edited
    /// elsewhere): reload and try again.
    Conflict,
    /// Someone cancelled it.
    Cancelled,
    /// Something it runs first (a plugin's hook, a profile's command)
    /// stopped it.
    Refused,
    /// A file or folder is gone.
    MissingFile,
    /// The OS refused access to a file or folder.
    AccessDenied,
    /// Not enough free space.
    DiskFull,
    /// A storage location or its drive can't be reached.
    StorageOffline,
    /// A file or the database is in use; usually passes.
    Busy,
    /// Caskade's own work kept every library connection busy for too long
    /// (a pool timeout): nothing outside Caskade holds it. Usually passes.
    Overloaded,
    /// A server can't be reached or didn't answer in time.
    Offline,
    /// A server answered with an error or something unusable.
    Server,
    /// A server refused the credentials.
    Auth,
    /// Content isn't what was expected (a checksum mismatch).
    Integrity,
    /// Stored data can't be read: a damaged file, cache or record.
    Corrupt,
    /// The library database failed.
    Database,
    /// A part of Caskade isn't running this session.
    Unavailable,
    /// A bug or an unexpected state inside Caskade.
    Internal,
    Other,
}

/// What the user (or Caskade) can do about a failure, most useful first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryHint {
    /// Trying again unchanged may work.
    Retry,
    /// The other side has trouble; try again in a while.
    RetryLater,
    CheckConnection,
    /// Reconnect the drive or network share.
    ReconnectStorage,
    FreeSpace,
    CheckPermissions,
    CheckCredentials,
    SignIn,
    /// Reload the view: what it shows is out of date.
    Reload,
    /// Restart Caskade.
    Restart,
    /// A newer Caskade wrote this data.
    UpdateCaskade,
}

/// A failure, safe to show and log: credentials, tokens and URL queries are
/// redacted. Commands return it as their error (`CommandError` in the app).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ErrorReport {
    pub code: ErrorCode,
    /// A plain-language sentence: what failed and, when known, why.
    pub summary: String,
    /// The error and its causes, redacted; `None` when the summary says it
    /// all.
    pub detail: Option<String>,
    pub hints: Vec<RecoveryHint>,
    /// Retrying unchanged may work (the network, a file in use).
    pub transient: bool,
    /// A short reference, logged with the detail, to find it in the logs.
    pub reference: String,
}

impl ErrorReport {
    /// A report of `code` with `summary`, its usual hints and no detail.
    pub fn new(code: ErrorCode, summary: impl Into<String>) -> Self {
        Self {
            code,
            summary: capitalize(&sanitize(&summary.into())),
            detail: None,
            hints: hints(code),
            transient: matches!(
                code,
                ErrorCode::Offline | ErrorCode::Busy | ErrorCode::Overloaded
            ),
            reference: reference(),
        }
    }

    /// Classifies `error` by walking its causes for the core's own errors,
    /// I/O and database errors. Text the error wrote for people stays the
    /// summary; an OS or database message becomes the detail under a
    /// plain-language summary.
    pub fn from_error(error: &(dyn std::error::Error + 'static)) -> Self {
        let chain = chain(error);
        let (code, summary, hints) = match classify(error) {
            Some(Found {
                code,
                summary,
                hints,
            }) => (code, summary, hints),
            None => (ErrorCode::Other, Summary::Own, None),
        };
        let summary = match summary {
            Summary::Own => error.to_string(),
            Summary::Text(text) => text,
            Summary::Plain => plain(code),
        };
        let mut report = Self::new(code, summary);
        if let Some(hints) = hints {
            report.hints = hints;
        }
        let detail = sanitize(&chain);
        if !report.summary.eq_ignore_ascii_case(&detail) {
            report.detail = Some(detail);
        }
        report
    }
}

impl std::fmt::Display for ErrorReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary)
    }
}

impl From<&Error> for ErrorReport {
    fn from(error: &Error) -> Self {
        Self::from_error(error)
    }
}

impl From<Error> for ErrorReport {
    fn from(error: Error) -> Self {
        Self::from_error(&error)
    }
}

/// A message written for people, e.g. an adapter's own check.
impl From<String> for ErrorReport {
    fn from(text: String) -> Self {
        Self::new(ErrorCode::Other, text)
    }
}

impl From<&str> for ErrorReport {
    fn from(text: &str) -> Self {
        Self::new(ErrorCode::Other, text)
    }
}

/// Where a report's summary comes from.
enum Summary {
    /// The error's own text: written for people.
    Own,
    /// Text found on the way.
    Text(String),
    /// [`plain`]'s sentence for the code: the error's text is the OS's or
    /// the database's, and goes in the detail.
    Plain,
}

struct Found {
    code: ErrorCode,
    summary: Summary,
    /// Hints other than the code's usual ones.
    hints: Option<Vec<RecoveryHint>>,
}

fn found(code: ErrorCode, summary: Summary) -> Option<Found> {
    Some(Found {
        code,
        summary,
        hints: None,
    })
}

/// Data a newer Caskade wrote: only updating helps.
fn newer(text: String) -> Option<Found> {
    Some(Found {
        code: ErrorCode::Unavailable,
        summary: Summary::Text(text),
        hints: Some(vec![RecoveryHint::UpdateCaskade]),
    })
}

/// The first cause that says what kind of failure this is.
fn classify(error: &(dyn std::error::Error + 'static)) -> Option<Found> {
    use ErrorCode::*;
    let mut cause = Some(error);
    while let Some(error) = cause {
        let hit = if let Some(error) = error.downcast_ref::<Error>() {
            core_error(error)
        } else if let Some(error) = error.downcast_ref::<crate::launch::LaunchError>() {
            use crate::launch::LaunchError as L;
            match error {
                L::Invalid(text) => found(Invalid, Summary::Text(text.clone())),
                L::AlreadyRunning => found(
                    Conflict,
                    Summary::Text(crate::tr!("The game is already running.")),
                ),
                L::Cancelled => found(
                    Cancelled,
                    Summary::Text(crate::tr!("The launch was cancelled.")),
                ),
                L::Refused(_) => found(Refused, Summary::Own),
                L::NeedsChoice(_) | L::AgeRestricted(_) => found(Invalid, Summary::Own),
                L::HostBusy(busy) => found(
                    Conflict,
                    Summary::Text(crate::tr!(
                        "{device} is playing another game.",
                        device = busy.device
                    )),
                ),
                L::Unsupported(runner) => found(
                    Invalid,
                    Summary::Text(crate::tr!(
                        "Caskade has nothing that can start this game ({runner}).",
                        runner = runner
                    )),
                ),
                L::NotStarted => found(
                    Other,
                    Summary::Text(crate::tr!(
                        "The game didn't start: no game process appeared in time."
                    )),
                ),
                L::Io(io) => io_error(io),
                L::Tracking(_) => found(Internal, Summary::Own),
                L::Store(_) => found(Database, Summary::Plain),
            }
        } else if let Some(error) = error.downcast_ref::<crate::settings::SettingsError>() {
            use crate::settings::SettingsError as S;
            match error {
                // The store error underneath says more.
                S::Store(_) => None,
                S::Json(_) | S::Stored | S::Migration => found(
                    Corrupt,
                    Summary::Text(crate::tr!(
                        "These settings can't be read; they may be damaged."
                    )),
                ),
                S::Version => newer(crate::tr!(
                    "These settings were saved by a newer version of Caskade."
                )),
                S::Conflict => found(
                    Conflict,
                    Summary::Text(crate::tr!(
                        "These settings changed elsewhere. Reload them and try again."
                    )),
                ),
                // The section's name and the fields stay in the detail.
                S::Unknown(_) => found(
                    Unavailable,
                    Summary::Text(crate::tr!("These settings aren't available right now.")),
                ),
                S::Validation { .. } => found(
                    Invalid,
                    Summary::Text(crate::tr!(
                        "Some of these settings aren't valid. Check them and try again."
                    )),
                ),
                S::Limit => found(
                    Invalid,
                    Summary::Text(crate::tr!("These settings are too large to save.")),
                ),
                S::Registration(_) | S::Internal => found(Internal, Summary::Plain),
            }
        } else if let Some(error) = error.downcast_ref::<crate::romm::RommError>() {
            use crate::romm::RommError as R;
            match error {
                R::Unreachable(_) | R::Interrupted(_) => found(Offline, Summary::Own),
                R::Auth(_) => found(Auth, Summary::Own),
                R::Server(_) => found(Server, Summary::Own),
                R::Config(_) => found(Invalid, Summary::Own),
                R::Cancelled => found(Cancelled, Summary::Own),
                R::Verify(_) => found(Integrity, Summary::Own),
                R::Extract(_) => found(Corrupt, Summary::Own),
                R::Core(error) => core_error(error),
            }
        } else if let Some(error) = error.downcast_ref::<crate::storage::StorageError>() {
            use crate::storage::StorageError as St;
            match error {
                // Transparent: what they wrap isn't among their causes.
                St::Store(error) => core_error(error),
                St::Io(error) => io_error(error),
                St::Json(_) => found(Corrupt, Summary::Plain),
                St::Settings(error) => classify(error),
                St::Executor(_) | St::Manifest(_) => None,
                St::Invalid(text) => found(Invalid, Summary::Text(text.clone())),
                St::Offline => found(
                    StorageOffline,
                    Summary::Text(crate::tr!(
                        "The storage location is offline, or a different drive is mounted in its place. Reconnect its drive, then try again."
                    )),
                ),
                St::ReadOnly => found(
                    AccessDenied,
                    Summary::Text(crate::tr!("Caskade can't write to this storage location.")),
                ),
                St::DefaultRoot => found(Invalid, Summary::Own),
                St::Running => found(
                    Conflict,
                    Summary::Text(crate::tr!(
                        "The game is running, or another move or uninstall of it is under way."
                    )),
                ),
                St::Worker(_) => found(Internal, Summary::Own),
            }
        } else if let Some(error) = error.downcast_ref::<crate::store_accounts::ProviderError>() {
            use crate::store_accounts::ProviderError as P;
            match error {
                P::Offline(_) => found(Offline, Summary::Own),
                P::Expired(_) => Some(Found {
                    code: Auth,
                    summary: Summary::Own,
                    hints: Some(vec![RecoveryHint::SignIn]),
                }),
                P::RateLimited { .. } => Some(Found {
                    code: Server,
                    summary: Summary::Own,
                    hints: Some(vec![RecoveryHint::RetryLater]),
                }),
                P::Cancelled => found(Cancelled, Summary::Own),
                P::Pending => found(Busy, Summary::Own),
                P::Unavailable(_) => found(Unavailable, Summary::Own),
                P::Rejected(_) => Some(Found {
                    code: Auth,
                    summary: Summary::Own,
                    hints: Some(vec![RecoveryHint::CheckCredentials]),
                }),
                P::Other(_) => found(Other, Summary::Own),
            }
        } else if let Some(error) = error.downcast_ref::<crate::store_accounts::ConnectError>() {
            use crate::store_accounts::ConnectError as C;
            match error {
                C::Expired | C::Mismatch => found(Invalid, Summary::Own),
                C::OtherAccount { .. } => found(Conflict, Summary::Own),
                // What they wrap says more.
                C::Provider(_) | C::Store(_) => None,
            }
        } else if let Some(error) = error.downcast_ref::<crate::backup::BackupError>() {
            use crate::backup::BackupError as B;
            match error {
                B::Busy => found(Busy, Summary::Own),
                B::Cancelled => found(Cancelled, Summary::Own),
                B::DestinationMissing(_) => found(StorageOffline, Summary::Own),
                B::NoSpace { .. } => found(DiskFull, Summary::Own),
                B::Damaged(_) => found(Corrupt, Summary::Own),
                B::Newer => newer(error.to_string()),
                B::Io(error) => io_error(error),
                B::Store(error) => core_error(error),
            }
        } else if let Some(error) = error.downcast_ref::<std::io::Error>() {
            io_error(error)
        } else if let Some(error) = error.downcast_ref::<sqlx::Error>() {
            sql_error(error)
        } else if error.downcast_ref::<serde_json::Error>().is_some() {
            found(Corrupt, Summary::Plain)
        } else {
            None
        };
        if hit.is_some() {
            return hit;
        }
        cause = error.source();
    }
    None
}

fn core_error(error: &Error) -> Option<Found> {
    use ErrorCode::*;
    match error {
        Error::Io(io) => io_error(io),
        Error::Db(error) => sql_error(error),
        Error::Migration(sqlx::migrate::MigrateError::VersionMissing(_)) => newer(crate::tr!(
            "The library database was written by a newer version of Caskade."
        )),
        Error::Migration(_) => found(Database, Summary::Plain),
        Error::Json(_) => found(Corrupt, Summary::Plain),
        Error::NoAppDirs => found(
            Unavailable,
            Summary::Text(crate::tr!("cannot determine the application directories")),
        ),
        Error::Invalid(_) => found(Invalid, Summary::Own),
        Error::Forbidden(_) => found(Forbidden, Summary::Own),
    }
}

fn io_error(error: &std::io::Error) -> Option<Found> {
    use ErrorCode::*;
    use std::io::ErrorKind as K;
    // A message Caskade wrote (`io::Error::new(kind, text)`) is for people;
    // the OS's own ("No such file or directory (os error 2)") is not.
    let summary = if error.get_ref().is_some() {
        Summary::Own
    } else {
        Summary::Plain
    };
    #[cfg(unix)]
    // EIO, ENXIO, ENODEV: a removed or failing drive.
    if let Some(5 | 6 | 19) = error.raw_os_error() {
        return found(StorageOffline, summary);
    }
    #[cfg(windows)]
    match error.raw_os_error() {
        // ERROR_NOT_READY, ERROR_DEV_NOT_EXIST, ERROR_DEVICE_NOT_CONNECTED,
        // ERROR_NO_MEDIA_IN_DRIVE, ERROR_BAD_NETPATH, ERROR_NETNAME_DELETED
        Some(21 | 55 | 1167 | 1112 | 53 | 64) => return found(StorageOffline, summary),
        // ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION
        Some(32 | 33) => return found(Busy, summary),
        _ => {}
    }
    let code = match error.kind() {
        K::NotFound => MissingFile,
        K::PermissionDenied | K::ReadOnlyFilesystem => AccessDenied,
        K::StorageFull | K::QuotaExceeded => DiskFull,
        K::ResourceBusy | K::ExecutableFileBusy | K::WouldBlock => Busy,
        K::TimedOut
        | K::NotConnected
        | K::ConnectionRefused
        | K::ConnectionReset
        | K::ConnectionAborted
        | K::HostUnreachable
        | K::NetworkUnreachable
        | K::NetworkDown => Offline,
        K::StaleNetworkFileHandle => StorageOffline,
        K::InvalidData | K::UnexpectedEof => Corrupt,
        // Caskade's own text with no telling kind: let the caller's
        // context (or `Other`) decide.
        _ if error.get_ref().is_some() => return None,
        _ => Other,
    };
    found(code, summary)
}

fn sql_error(error: &sqlx::Error) -> Option<Found> {
    use ErrorCode::*;
    match error {
        sqlx::Error::Io(io) => io_error(io),
        // Every connection was Caskade's own and stayed busy: not another
        // program's lock (#77).
        sqlx::Error::PoolTimedOut => found(Overloaded, Summary::Plain),
        sqlx::Error::Database(db) => {
            // SQLite's primary result codes.
            let code = db
                .code()
                .and_then(|code| code.parse::<i32>().ok())
                .map(|code| code & 0xff);
            found(
                match code {
                    Some(5 | 6) => Busy,
                    Some(11 | 26) => Corrupt,
                    Some(13) => DiskFull,
                    Some(8 | 14) => AccessDenied,
                    _ => Database,
                },
                Summary::Plain,
            )
        }
        _ => found(Database, Summary::Plain),
    }
}

/// The summary for a code when the error's own text isn't for people.
fn plain(code: ErrorCode) -> String {
    use ErrorCode::*;
    match code {
        Invalid => crate::tr!("That can't be done as asked."),
        Forbidden => crate::tr!("This account isn't allowed to do that."),
        Conflict => crate::tr!("Something changed in the meantime. Reload and try again."),
        Cancelled => crate::tr!("It was cancelled."),
        Refused => crate::tr!("Something Caskade runs first stopped it."),
        MissingFile => crate::tr!("A file or folder Caskade needs is missing."),
        AccessDenied => crate::tr!("Access to a file or folder Caskade needs was refused."),
        DiskFull => crate::tr!("There isn't enough free space."),
        StorageOffline => crate::tr!("A drive or network location Caskade needs can't be reached."),
        Busy => crate::tr!("A file or the library is in use. Caskade will manage once it's free."),
        Overloaded => crate::tr!(
            "Caskade's own work is keeping the library busy. It catches up by itself, so try again in a moment."
        ),
        Offline => crate::tr!("A server can't be reached. Check the connection."),
        Server => crate::tr!("A server answered with an error."),
        Auth => crate::tr!("The server refused the sign-in."),
        Integrity => crate::tr!("A download isn't what was expected, so it was discarded."),
        Corrupt => crate::tr!("Stored data Caskade needs can't be read; it may be damaged."),
        Database => crate::tr!("The library database couldn't be read or written."),
        Unavailable => crate::tr!("This part of Caskade isn't available right now."),
        Internal => crate::tr!("Caskade ran into an unexpected problem."),
        Other => crate::tr!("Caskade couldn't finish this; the details say why."),
    }
}

fn hints(code: ErrorCode) -> Vec<RecoveryHint> {
    use ErrorCode as C;
    use RecoveryHint::*;
    match code {
        C::Offline => vec![CheckConnection, Retry],
        C::Busy | C::Overloaded => vec![Retry],
        C::Server => vec![RetryLater],
        C::Auth => vec![CheckCredentials],
        C::Forbidden => vec![SignIn],
        C::Conflict => vec![Reload],
        C::AccessDenied => vec![CheckPermissions, Retry],
        C::DiskFull => vec![FreeSpace, Retry],
        C::StorageOffline => vec![ReconnectStorage, Retry],
        C::Database | C::Internal => vec![Retry, Restart],
        C::Unavailable => vec![Restart],
        C::MissingFile | C::Corrupt | C::Integrity | C::Other => vec![Retry],
        C::Invalid | C::Cancelled | C::Refused => vec![],
    }
}

/// The error and its causes, each once.
fn chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = String::new();
    let mut cause = Some(error);
    while let Some(error) = cause {
        let message = error.to_string();
        if !message.is_empty() && !text.contains(&message) {
            if !text.is_empty() {
                text.push_str(": ");
            }
            text.push_str(&message);
        }
        cause = error.source();
    }
    text
}

/// `text` without credentials, tokens or URL queries: the installer's URL
/// redaction (#158), then `key=value` and `Bearer` secrets.
pub fn sanitize(text: &str) -> String {
    use std::sync::LazyLock;
    // `token=…`, `"password": "…"`, `secret:…`; prose like "the token
    // expired" keeps its words.
    static SECRET: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r#"(?i)\b(access_token|refresh_token|token|api[_-]?key|apikey|client_secret|secret|password|passwd)("?\s*=\s*"?|"\s*:\s*"|:)([^\s"',;&]+)"#,
        )
        .expect("valid pattern")
    });
    static BEARER: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/=-]+").expect("valid pattern")
    });
    let text = caskade_installer::failure::redact(text);
    let text = SECRET.replace_all(&text, "${1}${2}<redacted>");
    BEARER.replace_all(&text, "${1} <redacted>").into_owned()
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => first.to_uppercase().chain(chars).collect(),
        _ => text.to_owned(),
    }
}

/// A short reference for one report: the random tail of a v7 UUID.
fn reference() -> String {
    let id = uuid::Uuid::now_v7().simple().to_string();
    id[id.len() - 8..].to_owned()
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
