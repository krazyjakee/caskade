//! Running the host and watching it: restarts after a crash, and a stop
//! when Caskade quits.

use super::{HostChecks, HostLaunch, HostState, StreamingHost, not_working};
use caskade_core::{Error, Result, emulation::OperatingSystem};
use std::{
    collections::VecDeque,
    path::Path,
    time::{Duration, Instant},
};

/// Restarts after this many crashes within [`CRASH_WINDOW`] stop.
const CRASHES: usize = 5;
const CRASH_WINDOW: Duration = Duration::from_secs(10 * 60);

impl StreamingHost {
    /// Starts it again after a pause, with its configuration rewritten.
    pub(super) async fn resume(&self) {
        let prepared = async {
            let program = self.program(false).await?;
            if self.inner.os != OperatingSystem::Windows {
                self.configure(&program, false).await?;
            }
            Ok::<_, Error>(program)
        }
        .await;
        match prepared {
            Ok(program) => self.run(&program).await,
            Err(error) => {
                tracing::warn!(%error, "streaming host not started");
                self.set_status(HostState::NeedsAttention, None, Some(not_working()));
            }
        }
    }

    /// Starts the host and watches it.
    pub(super) async fn run(&self, program: &Path) {
        let launch = match self.launch(program) {
            Ok(launch) => launch,
            Err(error) => {
                tracing::warn!(%error, "streaming host not started");
                self.set_status(HostState::NeedsAttention, None, Some(not_working()));
                return;
            }
        };
        let this = self.clone();
        let watch = tokio::spawn(async move { this.watch(launch).await });
        if let Some(old) = caskade_core::lock(&self.inner.watch).replace(watch) {
            old.abort();
        }
    }

    /// Starts, waits for it to answer, and restarts it after a crash.
    async fn watch(&self, launch: HostLaunch) {
        let mut crashes: VecDeque<Instant> = VecDeque::new();
        loop {
            self.set_status(HostState::Starting, None, None);
            let runner = self.inner.runner.clone();
            let started = {
                let launch = launch.clone();
                tokio::task::spawn_blocking(move || runner.start(&launch)).await
            };
            if !matches!(started, Ok(Ok(()))) {
                tracing::warn!("streaming host didn't start");
                self.set_status(HostState::NeedsAttention, None, Some(not_working()));
                return;
            }
            match self.until_ready().await {
                Ok(version) => self.set_ready(version).await,
                Err(error) => {
                    tracing::warn!(%error, "streaming host isn't answering");
                    self.set_status(HostState::NeedsAttention, None, Some(not_working()));
                    let runner = self.inner.runner.clone();
                    let _ = tokio::task::spawn_blocking(move || runner.stop()).await;
                    return;
                }
            }
            // The network can change under a running host. A look is never
            // waited for here, so a slow one can't hide a crash, and there
            // is one at a time. Status reads get what the last one found.
            let mut look: Option<tokio::task::JoinHandle<HostChecks>> = None;
            let mut looked = Instant::now();
            while self.running().await {
                tokio::time::sleep(self.inner.timing.watch_every).await;
                if let Some(finished) = look.take_if(|look| look.is_finished())
                    && let Ok(checks) = finished.await
                {
                    self.show_checks(checks);
                }
                if look.is_none() && looked.elapsed() >= self.inner.timing.look_every {
                    looked = Instant::now();
                    look = Some(self.look());
                }
            }
            let now = Instant::now();
            crashes.push_back(now);
            while crashes
                .front()
                .is_some_and(|at| now.duration_since(*at) > CRASH_WINDOW)
            {
                crashes.pop_front();
            }
            tracing::warn!(
                crashes = crashes.len(),
                "streaming host stopped unexpectedly"
            );
            if crashes.len() >= CRASHES {
                self.set_status(HostState::NeedsAttention, None, Some(not_working()));
                return;
            }
            let wait = self.inner.timing.restart_after * 2u32.pow(crashes.len() as u32 - 1);
            tokio::time::sleep(wait).await;
        }
    }

    /// Waits for the started host to answer; its version.
    pub(super) async fn until_ready(&self) -> Result<Option<String>> {
        #[cfg(feature = "net")]
        {
            let deadline = Instant::now() + self.inner.timing.ready_wait;
            loop {
                let answer = async { self.checked(self.api().await?.version().await) }.await;
                match answer {
                    Ok(version) => return Ok(Some(version)),
                    // A refused sign-in won't change by waiting.
                    Err(error @ Error::Forbidden(_)) => return Err(error),
                    Err(_) if !self.running().await => {
                        return Err(Error::Invalid(not_working()));
                    }
                    Err(error) if Instant::now() >= deadline => return Err(error),
                    Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
                }
            }
        }
        #[cfg(not(feature = "net"))]
        Ok(None)
    }

    /// Whether the host runs, asked on a blocking thread: on Windows that
    /// runs `sc.exe`.
    async fn running(&self) -> bool {
        let runner = self.inner.runner.clone();
        tokio::task::spawn_blocking(move || runner.running())
            .await
            .unwrap_or(false)
    }

    /// Stops watching and stops the host.
    pub(super) async fn halt(&self) {
        if let Some(watch) = caskade_core::lock(&self.inner.watch).take() {
            watch.abort();
        }
        let runner = self.inner.runner.clone();
        let _ = tokio::task::spawn_blocking(move || runner.stop()).await;
        self.set_status(HostState::Off, None, None);
    }

    /// When Caskade quits: the host stops with it (blocking). A window
    /// closed to the tray doesn't quit, so the host keeps running.
    pub fn shutdown(&self) {
        if let Some(watch) = caskade_core::lock(&self.inner.watch).take() {
            watch.abort();
        }
        if let Err(error) = self.inner.runner.stop() {
            tracing::warn!(%error, "streaming host didn't stop");
        }
        self.set_status(HostState::Off, None, None);
    }
}
