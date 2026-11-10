//! One Caskade per user (#88): the first start takes a per-user endpoint
//! (a Unix socket in a folder only the user can open, or a local-only named
//! pipe: `launch_cli::channel`); a second start finds it taken, hands its
//! arguments and working folder to the running Caskade and ends. What the
//! arguments ask for ([`Intent`]) is the same for every shell.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::launch_cli::channel::{self, Endpoint, Listener};

/// The longest message a second start may send: its arguments are a few
/// paths or links.
const MESSAGE_LIMIT: u64 = 64 * 1024;
/// What the running Caskade answers once it has the arguments.
const TAKEN: &str = "taken";

/// A second start's arguments (the program first) and working folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forwarded {
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl Forwarded {
    /// This process's own.
    pub fn this_process() -> Self {
        Self {
            args: std::env::args().collect(),
            cwd: std::env::current_dir().unwrap_or_default(),
        }
    }
}

/// Where the running Caskade takes second starts; apart from the
/// `--launch` channel, and named after Caskade's data folders so a dev
/// build and an installed one keep apart.
pub fn endpoint(paths: &caskade_core::AppPaths) -> Endpoint {
    Endpoint::at(paths.cache.join("run").join("instance.sock"))
}

/// Whether this start is the one that runs.
pub enum Claim {
    /// It is: second starts arrive here (see [`serve`]).
    First(Listener),
    /// Another Caskade runs and has this start's arguments: end now.
    Forwarded,
}

/// Takes `endpoint` for this start, or hands `forwarded` to the Caskade
/// that holds it.
pub async fn claim(endpoint: &Endpoint, forwarded: &Forwarded) -> std::io::Result<Claim> {
    match Listener::bind(endpoint).await {
        Ok(listener) => Ok(Claim::First(listener)),
        // A Unix socket that answers, or a pipe someone else created.
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            send(endpoint, forwarded).await?;
            Ok(Claim::Forwarded)
        }
        Err(error) => Err(error),
    }
}

/// Hands `forwarded` to the Caskade at `endpoint`, and waits until it has
/// it.
pub async fn send(endpoint: &Endpoint, forwarded: &Forwarded) -> std::io::Result<()> {
    let stream = channel::connect(endpoint).await?;
    let (read, mut write) = tokio::io::split(stream);
    let mut message = serde_json::to_vec(forwarded).map_err(std::io::Error::other)?;
    message.push(b'\n');
    write.write_all(&message).await?;
    write.flush().await?;
    let mut answer = String::new();
    BufReader::new(read.take(64)).read_line(&mut answer).await?;
    if answer.trim() == TAKEN {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "the running Caskade didn't take the arguments",
        ))
    }
}

/// Takes second starts at `listener` for as long as the app runs, handing
/// each to `on`.
pub fn serve(mut listener: Listener, on: impl Fn(Forwarded) + Send + Sync + 'static) {
    let on = std::sync::Arc::new(on);
    crate::runtime::spawn(async move {
        loop {
            let stream = match listener.accept().await {
                Ok(stream) => stream,
                Err(error) => {
                    tracing::warn!(%error, "a second start couldn't connect");
                    continue;
                }
            };
            let on = on.clone();
            crate::runtime::spawn(async move {
                if let Err(error) = take(stream, &*on).await {
                    tracing::warn!(%error, "a second start's arguments weren't read");
                }
            });
        }
    });
}

async fn take(
    stream: channel::Stream,
    on: &(dyn Fn(Forwarded) + Send + Sync),
) -> std::io::Result<()> {
    let (read, mut write) = tokio::io::split(stream);
    let mut line = String::new();
    BufReader::new(read.take(MESSAGE_LIMIT))
        .read_line(&mut line)
        .await?;
    // A start checking whether the endpoint answers sends nothing.
    if line.trim().is_empty() {
        return Ok(());
    }
    let forwarded: Forwarded = serde_json::from_str(line.trim()).map_err(std::io::Error::other)?;
    on(forwarded);
    write.write_all(format!("{TAKEN}\n").as_bytes()).await?;
    write.flush().await
}

/// What a start's arguments ask the running Caskade for, besides showing
/// its window, which every second start does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// `caskade://…` links: installs, pairing, shares, store sign-ins.
    Links(Vec<url::Url>),
    /// A magnet link or `.torrent` file the OS opened with Caskade (#159).
    Install(String),
    /// `caskade dev <folder>`: load a plugin or theme in place (#131).
    DevFolder(PathBuf),
    /// `caskade --console` (#148).
    Console,
}

/// What `args` (the program first), started in `cwd`, ask for.
pub fn intents(args: &[String], cwd: &Path) -> Vec<Intent> {
    let mut intents = Vec::new();
    if let Some(folder) = crate::dev_plugins::folder_from_args(args, cwd) {
        intents.push(Intent::DevFolder(folder));
    }
    if let Some(input) = crate::torrent_links::input_from_args(args, cwd) {
        intents.push(Intent::Install(input));
    }
    let links: Vec<url::Url> = args
        .iter()
        .skip(1)
        .filter_map(|arg| url::Url::parse(arg).ok())
        .filter(|link| link.scheme() == crate::deep_link::SCHEME)
        .collect();
    if !links.is_empty() {
        intents.push(Intent::Links(links));
    }
    if crate::commands::console::wants_console(args) {
        intents.push(Intent::Console);
    }
    intents
}

#[cfg(test)]
#[path = "single_instance_tests.rs"]
mod tests;
