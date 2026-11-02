//! `caskade --launch <game id> [--entry <id>] [--action <id>]` (#256):
//! another program (Sunshine, a desktop shortcut, another launcher) starts
//! a library game through Caskade's launch pipeline, and its process lives
//! exactly as long as the game's session.
//!
//! The caller hands the launch to the running Caskade over a local channel
//! that only the same user can reach ([`channel`]: a socket in a private
//! folder, or a named pipe on Windows), starting Caskade first when it
//! isn't running. The single-instance plugin can't carry the launch: it
//! ends the second process at once and sends nothing back.
//!
//! Caskade answers `started`, then `ended` when the session does, and the
//! caller exits with a code saying how ([`code`]). A launch it refuses (an
//! age limit, nobody signed in, a release that would ask which action)
//! ends the caller with [`code::REFUSED`] and the reason on stderr; no
//! dialog opens, since nobody may be in front of the device. Ending the
//! caller (SIGTERM, Ctrl-C, Windows' WM_CLOSE, being killed) closes the
//! channel, and Caskade then stops the game the way Stop does.

pub mod channel;
pub mod server;

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io,
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub use channel::Endpoint;

/// The argument that makes this process a caller.
pub const LAUNCH_ARG: &str = "--launch";

/// The caller's exit codes: how the launch ended.
pub mod code {
    /// The game exited.
    pub const EXITED: i32 = 0;
    /// The game ended with an error, or wasn't seen to end.
    pub const CRASHED: i32 = 1;
    /// The command line was wrong.
    pub const USAGE: i32 = 2;
    /// Caskade refused the launch: an age limit, nobody signed in, the game
    /// already running, or a release that would ask which action to start.
    pub const REFUSED: i32 = 3;
    /// The game couldn't start, or Caskade couldn't be reached.
    pub const FAILED: i32 = 4;
    /// Caskade's Stop ended the game, or the caller was asked to close.
    pub const STOPPED: i32 = 5;
}

/// Environment the caller passes on to the game: Sunshine's description of
/// the client's display, for display flags. Nothing else crosses.
const CLIENT_ENV: [&str; 4] = [
    "SUNSHINE_CLIENT_WIDTH",
    "SUNSHINE_CLIENT_HEIGHT",
    "SUNSHINE_CLIENT_FPS",
    "SUNSHINE_CLIENT_HDR",
];

/// How long a caller waits for a Caskade it started to take launches.
const START_PATIENCE: Duration = Duration::from_secs(90);
/// The longest request line Caskade reads.
const MAX_REQUEST: u64 = 16 * 1024;

/// What the caller asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub game_id: String,
    /// One of the game's releases; `None` is the one Play would start.
    #[serde(default)]
    pub entry_id: Option<String>,
    /// The play action to start; `None` is the release's default.
    #[serde(default)]
    pub action_id: Option<String>,
    /// The caller's [`CLIENT_ENV`], filtered again by Caskade.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Caskade's answers, one JSON line each.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    Started {
        #[serde(rename = "sessionId")]
        session_id: String,
        title: String,
    },
    Ended {
        how: Ending,
    },
    Refused {
        message: String,
    },
    Failed {
        message: String,
    },
}

/// How a session that started ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ending {
    Exited,
    Crashed,
    Stopped,
}

impl Ending {
    pub fn code(self) -> i32 {
        match self {
            Self::Exited => code::EXITED,
            Self::Crashed => code::CRASHED,
            Self::Stopped => code::STOPPED,
        }
    }
}

/// The request in `args` (the program name first), when they ask for a
/// launch: `Some(Err(usage))` for a malformed one.
pub fn from_args<I, S>(args: I) -> Option<Result<Request, String>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    let at = args.iter().position(|arg| arg == LAUNCH_ARG)?;
    let usage = || {
        format!("usage: caskade {LAUNCH_ARG} <game id> [--entry <entry id>] [--action <action id>]")
    };
    let mut rest = args[at + 1..].iter();
    let Some(game_id) = rest
        .next()
        .filter(|id| !id.starts_with('-') && !id.is_empty())
    else {
        return Some(Err(usage()));
    };
    let mut request = Request {
        game_id: game_id.clone(),
        env: client_env(std::env::vars()),
        ..Default::default()
    };
    while let Some(flag) = rest.next() {
        let slot = match flag.as_str() {
            "--entry" => &mut request.entry_id,
            "--action" => &mut request.action_id,
            _ => return Some(Err(usage())),
        };
        match rest
            .next()
            .filter(|value| !value.starts_with('-') && !value.is_empty())
        {
            Some(value) if slot.is_none() => *slot = Some(value.clone()),
            _ => return Some(Err(usage())),
        }
    }
    Some(Ok(request))
}

/// The variables of `vars` a game may see: [`CLIENT_ENV`] with plausible
/// values (sizes and rates as numbers, HDR as true or false).
pub fn client_env(vars: impl IntoIterator<Item = (String, String)>) -> BTreeMap<String, String> {
    vars.into_iter()
        .filter(|(key, value)| match key.as_str() {
            "SUNSHINE_CLIENT_HDR" => matches!(value.as_str(), "true" | "false"),
            key if CLIENT_ENV.contains(&key) => {
                !value.is_empty() && value.len() <= 6 && value.bytes().all(|b| b.is_ascii_digit())
            }
            _ => false,
        })
        .collect()
}

/// What the caller ends with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub code: i32,
    /// For stderr: why it was refused or failed.
    pub message: Option<String>,
}

impl Outcome {
    fn failed(message: String) -> Self {
        Self {
            code: code::FAILED,
            message: Some(message),
        }
    }
}

/// Runs the caller for `request` and exits with its [`Outcome`]. Never
/// returns.
pub fn main(request: Request) -> ! {
    let work = move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => return finish(Outcome::failed(error.to_string())),
        };
        let endpoint = match caskade_core::AppPaths::standard() {
            Ok(paths) => Endpoint::for_paths(&paths),
            Err(error) => return finish(Outcome::failed(error.to_string())),
        };
        finish(runtime.block_on(run(&endpoint, &request, start_caskade, START_PATIENCE)))
    };
    hold(work)
}

/// Prints the outcome's message and gives its code.
fn finish(outcome: Outcome) -> i32 {
    if let Some(message) = &outcome.message {
        eprintln!("caskade: {message}");
        tracing::info!(code = outcome.code, %message, "launch from another program ended");
    }
    outcome.code
}

/// Waits for `work` on POSIX, where a signal ends the process and so the
/// channel.
#[cfg(not(windows))]
fn hold(work: impl FnOnce() -> i32) -> ! {
    std::process::exit(work())
}

/// Waits for `work` with a hidden window, so that WM_CLOSE (what Sunshine
/// sends a program when a stream ends) ends the caller, and so the game.
#[cfg(windows)]
fn hold(work: impl FnOnce() -> i32 + Send + 'static) -> ! {
    use tao::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoopBuilder},
        window::WindowBuilder,
    };
    let event_loop = EventLoopBuilder::<i32>::with_user_event().build();
    let window = match WindowBuilder::new()
        .with_visible(false)
        .with_title("Caskade")
        .build(&event_loop)
    {
        Ok(window) => window,
        Err(error) => {
            tracing::warn!(%error, "no window for WM_CLOSE; waiting without one");
            std::process::exit(work());
        }
    };
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        let code = work();
        // Without the loop, the code is all that is left to give.
        if proxy.send_event(code).is_err() {
            std::process::exit(code);
        }
    });
    event_loop.run(move |event, _, flow| {
        let _ = &window;
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(code) => std::process::exit(code),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => std::process::exit(code::STOPPED),
            _ => {}
        }
    })
}

/// Starts Caskade on its own, so it outlives the caller: in a process group
/// of its own on POSIX, out of the caller's job on Windows where allowed.
fn start_caskade() -> io::Result<Option<std::process::Child>> {
    let program = std::env::current_exe()?;
    let command = || {
        let mut command = std::process::Command::new(&program);
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        command
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command().process_group(0).spawn().map(Some)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        // A job that doesn't allow breaking away refuses the flag.
        command()
            .creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB)
            .spawn()
            .or_else(|_| command().creation_flags(flags).spawn())
            .map(Some)
    }
}

/// Hands `request` to the Caskade at `endpoint`, starting one with `start`
/// when none answers, and follows the launch to its end.
pub async fn run(
    endpoint: &Endpoint,
    request: &Request,
    start: impl FnOnce() -> io::Result<Option<std::process::Child>>,
    patience: Duration,
) -> Outcome {
    let stream = match connect_or_start(endpoint, start, patience).await {
        Ok(stream) => stream,
        Err(message) => return Outcome::failed(message),
    };
    follow(stream, request).await
}

async fn connect_or_start(
    endpoint: &Endpoint,
    start: impl FnOnce() -> io::Result<Option<std::process::Child>>,
    patience: Duration,
) -> Result<channel::Stream, String> {
    if let Ok(stream) = channel::connect(endpoint).await {
        return Ok(stream);
    }
    let mut child = start().map_err(|error| {
        caskade_core::tr!("Caskade couldn't start: {error}", error = error.to_string())
    })?;
    let deadline = Instant::now() + patience;
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(stream) = channel::connect(endpoint).await {
            return Ok(stream);
        }
        // A Caskade that found another one running hands it the start and
        // exits cleanly; that one may still be on its way up.
        if let Some(Ok(Some(status))) = child.as_mut().map(std::process::Child::try_wait)
            && !status.success()
        {
            return Err(caskade_core::tr!("Caskade couldn't start"));
        }
        if Instant::now() >= deadline {
            return Err(caskade_core::tr!("Caskade didn't start in time"));
        }
    }
}

async fn follow(stream: channel::Stream, request: &Request) -> Outcome {
    let (read, mut write) = tokio::io::split(stream);
    let mut line = serde_json::to_vec(request).unwrap_or_default();
    line.push(b'\n');
    if let Err(error) = write.write_all(&line).await {
        return Outcome::failed(error.to_string());
    }
    let mut lines = BufReader::new(read).lines();
    loop {
        let reply = match lines.next_line().await {
            Ok(Some(line)) => serde_json::from_str::<Reply>(&line),
            Ok(None) => return Outcome::failed(caskade_core::tr!("Caskade closed")),
            Err(error) => return Outcome::failed(error.to_string()),
        };
        match reply {
            Ok(Reply::Started { title, .. }) => {
                tracing::info!(game = %title, "started from another program");
            }
            Ok(Reply::Ended { how }) => {
                return Outcome {
                    code: how.code(),
                    message: None,
                };
            }
            Ok(Reply::Refused { message }) => {
                return Outcome {
                    code: code::REFUSED,
                    message: Some(message),
                };
            }
            Ok(Reply::Failed { message }) => return Outcome::failed(message),
            Err(error) => return Outcome::failed(error.to_string()),
        }
    }
}

/// Reads the request line on Caskade's side, refusing an overlong one.
async fn read_request<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Request, String> {
    let mut line = String::new();
    let read = (&mut *reader)
        .take(MAX_REQUEST)
        .read_line(&mut line)
        .await
        .map_err(|error| error.to_string())?;
    if read == 0 || !line.ends_with('\n') {
        return Err(caskade_core::tr!("the launch request was incomplete"));
    }
    serde_json::from_str(&line).map_err(|error| error.to_string())
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
