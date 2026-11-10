//! Streamed games (#264): a release another device
//! offers plays through Caskade's streaming client, which a
//! [`StreamLauncher`] gets ready and checks first.
//!
//! Streamed play counts on the device that runs the game: its own
//! `caskade --launch` records the session. Here nothing is written. The
//! stream is still followed like a session, so `game.started` and
//! `game.exited`, running games, Stop and the guide menu work as for any
//! game; `launch.after` (saves, plugins' after-hooks) never runs for it.
//!
//! The client's exit code is 0 whether it streamed or failed, and its
//! errors wait in dialogs. So the launch reads what it writes on stderr:
//! it returns once the stream is up, or fails with Caskade's own words
//! (the client ended) for the first error line. An error once the stream
//! is up ends it the same way.

use super::{
    LaunchChoice, LaunchError, LaunchEvent, LaunchHandle, LaunchService, LaunchTicket, Result,
    SessionStatus,
};
use crate::{
    model::{GameEntry, LaunchSpec, PlaySession},
    now_ms,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::{mpsc, oneshot, watch},
};
use tokio_util::sync::CancellationToken;

/// The device is playing another game: Play asks before stopping it, then
/// launches again with [`LaunchChoice::stop_host`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HostBusy {
    pub entry_id: String,
    /// The device's name.
    pub device: String,
    /// The game it plays, when the device says.
    pub playing: Option<String>,
}

/// What a line the client wrote on stderr means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientLine {
    /// The stream is up.
    Streaming,
    /// The client failed: the text to show, in Caskade's words.
    Failed(String),
    Other,
}

/// A stream about to start.
pub struct StreamStart {
    /// The client, ready to run; the launch adds stdio and its process
    /// group.
    pub command: std::process::Command,
    /// Kept until the stream ends (one client at a time).
    pub hold: Option<Box<dyn std::any::Any + Send>>,
}

/// Gets streams ready and reads the client (`caskade_streaming`'s
/// `Streams`; a fake one in tests).
#[async_trait]
pub trait StreamLauncher: Send + Sync {
    /// Checks that `host` (its id) can stream `app` now: paired, awake
    /// (woken first), offering it, and not playing another game unless
    /// `stop_host` (else [`LaunchError::HostBusy`]). Returns the client.
    async fn prepare(&self, host: &str, app: &str, stop_host: bool) -> Result<StreamStart>;
    /// What `line`, written by the client streaming `app` from `host`,
    /// means.
    fn read(&self, host: &str, app: &str, line: &str) -> ClientLine;
    /// The stream ended: the game stops on the host, unless it already
    /// did.
    async fn finish(&self, host: &str, app: &str);
    /// How long the stream may take to come up.
    fn ready_within(&self) -> Duration {
        Duration::from_secs(60)
    }
    /// How long errors still count as the stream not starting once the
    /// host answered.
    fn settle(&self) -> Duration {
        Duration::from_secs(3)
    }
    /// How long the client gets to quit before it is ended.
    fn stop_grace(&self) -> Duration {
        Duration::from_secs(5)
    }
}

/// The streams running now.
pub(super) type Streams = Arc<std::sync::Mutex<std::collections::BTreeMap<String, Running>>>;

/// A stream running now.
#[derive(Clone)]
pub(super) struct Running {
    session: PlaySession,
    started: Instant,
    stop: CancellationToken,
    ended: watch::Receiver<bool>,
}

impl Running {
    fn now(&self) -> PlaySession {
        PlaySession {
            duration_ms: super::service::millis(self.started.elapsed()),
            heartbeat_at_ms: now_ms(),
            ..self.session.clone()
        }
    }
}

/// The streaming target `entry` launches, when it is one.
pub(super) fn target(entry: &GameEntry) -> Option<(&str, &str)> {
    match &entry.launch {
        Some(LaunchSpec::Stream { host, app }) => Some((host, app)),
        _ => None,
    }
}

impl LaunchService {
    /// Streams `entry` from its host, once the play gate let it. No session
    /// is written.
    pub(super) async fn stream(
        &self,
        entry: &GameEntry,
        host: &str,
        app: &str,
        choice: &LaunchChoice,
    ) -> Result<LaunchHandle> {
        let streamer = self
            .streamer
            .get()
            .cloned()
            .ok_or_else(|| LaunchError::Unsupported("streamed games".into()))?;
        let lease = self.try_game_lease(&entry.game_id)?;
        // One stream at a time, as the client allows.
        if !crate::lock(&self.streams).is_empty() {
            return Err(LaunchError::Refused(crate::tr!(
                "Another game is streaming. End that stream first."
            )));
        }
        let start = tokio::select! {
            _ = lease.cancel.cancelled() => return Err(LaunchError::Cancelled),
            start = streamer.prepare(host, app, choice.stop_host) => start,
        };
        let StreamStart { command, hold } = start.map_err(|error| match error {
            LaunchError::HostBusy(mut busy) => {
                busy.entry_id = entry.id.clone();
                LaunchError::HostBusy(busy)
            }
            error => error,
        })?;
        let session = PlaySession::new(&entry.game_id, &entry.id, now_ms());
        // Caskade covers the client's own window until the stream is up.
        self.hooks
            .stream_starting(&super::service::ticket(&session))
            .await;
        let mut command = tokio::process::Command::from(command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            // Caskade going ends the stream.
            .kill_on_drop(true);
        // Its own group: the client's launcher script and the client end
        // together.
        #[cfg(unix)]
        command.process_group(0);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                streamer.finish(host, app).await;
                self.hooks
                    .exited(&stream_event(&session, SessionStatus::NotStarted))
                    .await;
                return Err(LaunchError::Io(std::io::Error::new(
                    error.kind(),
                    crate::tr!("couldn't start streaming: {error}", error = error),
                )));
            }
        };
        let pid = child.id();
        let (lines_tx, mut lines) = mpsc::unbounded_channel();
        if let Some(stderr) = child.stderr.take() {
            let (streamer, host, app) = (streamer.clone(), host.to_owned(), app.to_owned());
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let meaning = streamer.read(&host, &app, &line);
                    if meaning != ClientLine::Other && lines_tx.send(meaning).is_err() {
                        break;
                    }
                }
            });
        }
        let stop = CancellationToken::new();
        let (ended_tx, ended) = watch::channel(false);
        crate::lock(&self.streams).insert(
            session.id.clone(),
            Running {
                session: session.clone(),
                started: Instant::now(),
                stop: stop.clone(),
                ended,
            },
        );
        let settle = streamer.settle();
        let ready = tokio::time::timeout(streamer.ready_within(), async {
            // Once the host answered, errors in the next moments (the
            // connection failing) still count as not starting.
            let mut settled: Option<tokio::time::Instant> = None;
            loop {
                let settling = async {
                    match settled {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    line = lines.recv() => match line {
                        Some(ClientLine::Streaming) => {
                            settled.get_or_insert(tokio::time::Instant::now() + settle);
                        }
                        Some(ClientLine::Failed(why)) => return Err(Some(why)),
                        Some(ClientLine::Other) => {}
                        // It closed stderr: it is gone.
                        None => return Err(None),
                    },
                    _ = settling => return Ok(()),
                    _ = child.wait() => return Err(None),
                    _ = stop.cancelled() => return Err(None),
                    _ = lease.cancel.cancelled() => return Err(None),
                }
            }
        })
        .await;
        let failed = match ready {
            Ok(Ok(())) => None,
            Ok(Err(why)) => Some(why),
            Err(_) => Some(Some(crate::tr!(
                "The stream didn't start within {seconds} s.",
                seconds = streamer.ready_within().as_secs()
            ))),
        };
        if let Some(why) = failed {
            end_client(&mut child, pid, streamer.stop_grace()).await;
            streamer.finish(host, app).await;
            crate::lock(&self.streams).remove(&session.id);
            let _ = ended_tx.send(true);
            drop(hold);
            // Whatever covered the client stops covering it.
            let ended = PlaySession {
                ended_at_ms: Some(now_ms()),
                ..session.clone()
            };
            self.hooks
                .exited(&stream_event(&ended, SessionStatus::NotStarted))
                .await;
            return Err(if stop.is_cancelled() || lease.cancel.is_cancelled() {
                LaunchError::Cancelled
            } else {
                LaunchError::Refused(
                    why.unwrap_or_else(|| crate::tr!("The stream ended before it started.")),
                )
            });
        }
        self.hooks
            .started(&stream_event(&session, SessionStatus::Running))
            .await;
        let ticket = super::service::ticket(&session);
        let (done_tx, done) = oneshot::channel();
        let service = self.clone();
        let (host, app) = (host.to_owned(), app.to_owned());
        tokio::spawn(async move {
            let (status, asked) = tokio::select! {
                _ = child.wait() => (SessionStatus::Exited, false),
                failed = async {
                    loop {
                        match lines.recv().await {
                            Some(ClientLine::Failed(why)) => return why,
                            Some(_) => {}
                            None => std::future::pending::<()>().await,
                        }
                    }
                } => {
                    tracing::warn!(reason = %failed, "the stream failed");
                    (SessionStatus::Crashed, false)
                }
                _ = stop.cancelled() => (SessionStatus::Exited, true),
                _ = lease.cancel.cancelled() => (SessionStatus::Exited, true),
            };
            if asked {
                // The game stops on the host first; the stream then ends by
                // itself, and the client quits.
                streamer.finish(&host, &app).await;
                let _ = tokio::time::timeout(streamer.stop_grace(), child.wait()).await;
                end_client(&mut child, pid, streamer.stop_grace()).await;
            } else {
                end_client(&mut child, pid, streamer.stop_grace()).await;
                streamer.finish(&host, &app).await;
            }
            let ended = crate::lock(&service.streams)
                .remove(&session.id)
                .map_or(session.clone(), |running| running.now());
            let session = PlaySession {
                ended_at_ms: Some(now_ms()),
                ..ended
            };
            drop(hold);
            drop(lease);
            let _ = ended_tx.send(true);
            service.hooks.exited(&stream_event(&session, status)).await;
            let _ = done_tx.send(Ok((session, status)));
        });
        Ok(LaunchHandle {
            ticket,
            completed: done,
        })
    }

    /// The streams running now, as sessions that are never stored: of
    /// `game_id`, or all.
    pub fn running_streams(&self, game_id: Option<&str>) -> Vec<PlaySession> {
        crate::lock(&self.streams)
            .values()
            .filter(|running| game_id.is_none_or(|id| running.session.game_id == id))
            .map(Running::now)
            .collect()
    }

    pub(super) fn stream_tickets(&self) -> Vec<LaunchTicket> {
        crate::lock(&self.streams)
            .values()
            .map(|running| super::service::ticket(&running.session))
            .collect()
    }

    /// Ends the stream `session_id` (End stream): the client quits and the
    /// game stops on the host. `None` when it isn't a stream.
    pub(super) async fn stop_stream(&self, session_id: &str) -> Option<usize> {
        let running = crate::lock(&self.streams).get(session_id).cloned()?;
        crate::lock(&self.stopped).insert(session_id.to_owned());
        running.stop.cancel();
        let mut ended = running.ended;
        // Ending takes the client's grace and the host's answer.
        let _ = tokio::time::timeout(Duration::from_secs(30), ended.wait_for(|ended| *ended)).await;
        Some(1)
    }

    /// Streams `entry` with the client `streamer` makes ready (#264). Set
    /// once, when the streaming client exists; clones share it.
    pub fn set_streamer(&self, streamer: Arc<dyn StreamLauncher>) {
        let _ = self.streamer.set(streamer);
    }
}

/// The event of a stream: identity and timing only, no command.
fn stream_event(session: &PlaySession, status: SessionStatus) -> LaunchEvent {
    LaunchEvent {
        command: None,
        game_id: session.game_id.clone(),
        entry_id: session.entry_id.clone(),
        session_id: session.id.clone(),
        started_at_ms: session.started_at_ms,
        ended_at_ms: session.ended_at_ms,
        duration_ms: session.duration_ms,
        status,
    }
}

/// Asks the client to quit (SIGTERM to its group on Unix, so it saves its
/// settings), and ends it after `grace`.
async fn end_client(child: &mut tokio::process::Child, pid: Option<u32>, grace: Duration) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    #[cfg(unix)]
    if let Some(pid) = pid {
        signal_group(pid, "TERM").await;
        if tokio::time::timeout(grace, child.wait()).await.is_ok() {
            return;
        }
        // Not reaped yet, so the group is still the client's.
        signal_group(pid, "KILL").await;
    }
    #[cfg(not(unix))]
    let _ = (pid, grace);
    let _ = child.kill().await;
}

#[cfg(unix)]
async fn signal_group(pid: u32, signal: &str) {
    let _ = tokio::process::Command::new("kill")
        .args([&format!("-{signal}"), "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}
