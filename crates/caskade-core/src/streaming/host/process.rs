//! Running the host (STREAMING-PLAN §4.2): only while Caskade runs.
//!
//! - Linux: Sunshine is Caskade's child, in a process group of its own,
//!   started through `setpriv --pdeathsig TERM` so it ends with Caskade
//!   even after a crash. The death signal belongs to the thread that
//!   started it, so each run gets a thread of its own that lives as long as
//!   the host. A record (`run.json`) lets the next start stop a host a crash
//!   left behind.
//! - Windows: the on-demand `CaskadeStreaming` service, started and stopped
//!   with `sc.exe` (the interactive user may, without a prompt).

use crate::{Result, tracking::ProcessIdentity};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// How long the host gets to quit before it is ended.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// What starts the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLaunch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, PathBuf)>,
    pub cwd: PathBuf,
}

/// Starts, stops and watches the host process.
pub trait HostRunner: Send + Sync {
    /// Starts the host; a host already running from an earlier Caskade is
    /// stopped first.
    fn start(&self, launch: &HostLaunch) -> Result<()>;
    /// Stops it, waiting for it to quit (blocking).
    fn stop(&self) -> Result<()>;
    /// Whether it is running now.
    fn running(&self) -> bool;
}

/// Sends `signal` to process group `pid`.
#[cfg(unix)]
fn signal_group(pid: u32, signal: &str) {
    let _ = std::process::Command::new("kill")
        .args(["-s", signal, "--", &format!("-{pid}")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Linux (and macOS, for tests): the host as Caskade's child.
pub struct ChildRunner {
    record: PathBuf,
    running: Mutex<Option<Running>>,
}

struct Running {
    exited: Arc<AtomicBool>,
    stop: mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

impl ChildRunner {
    /// `record`: where the running host is noted (`run.json`).
    pub fn new(record: PathBuf) -> Self {
        Self {
            record,
            running: Mutex::new(None),
        }
    }

    /// Stops a host an earlier Caskade left running, by its record; a
    /// process that merely reuses its id is left alone.
    fn stop_left_behind(&self) {
        let Ok(bytes) = std::fs::read(&self.record) else {
            return;
        };
        let _ = std::fs::remove_file(&self.record);
        let Ok(record) = serde_json::from_slice::<ProcessIdentity>(&bytes) else {
            return;
        };
        let record = record.current();
        if ProcessIdentity::lookup(record.pid) != Some(record) {
            return;
        }
        tracing::info!(pid = record.pid, "stopping a streaming host left running");
        #[cfg(unix)]
        {
            signal_group(record.pid, "TERM");
            let deadline = Instant::now() + STOP_GRACE;
            while ProcessIdentity::lookup(record.pid) == Some(record) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            if ProcessIdentity::lookup(record.pid) == Some(record) {
                signal_group(record.pid, "KILL");
            }
        }
    }
}

fn command(launch: &HostLaunch) -> std::process::Command {
    // Ends with the thread that started it, so with Caskade.
    let setpriv = Path::new("/usr/bin/setpriv");
    let mut command = if cfg!(target_os = "linux") && setpriv.is_file() {
        let mut command = std::process::Command::new(setpriv);
        command
            .args(["--pdeathsig", "TERM", "--"])
            .arg(&launch.program);
        command
    } else {
        std::process::Command::new(&launch.program)
    };
    command
        .args(&launch.args)
        .current_dir(&launch.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    crate::managed::outside_own_appimage(&mut command);
    for (key, value) in &launch.env {
        command.env(key, value);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

impl HostRunner for ChildRunner {
    fn start(&self, launch: &HostLaunch) -> Result<()> {
        self.stop()?;
        self.stop_left_behind();
        let (started, pid) = mpsc::channel();
        let (stop, stopping) = mpsc::channel::<()>();
        let exited = Arc::new(AtomicBool::new(false));
        let record = self.record.clone();
        let mut command = command(launch);
        let thread = std::thread::Builder::new()
            .name("streaming host".into())
            .spawn({
                let exited = exited.clone();
                move || {
                    let mut child = match command.spawn() {
                        Ok(child) => child,
                        Err(error) => {
                            let _ = started.send(Err(error));
                            exited.store(true, Ordering::SeqCst);
                            return;
                        }
                    };
                    let pid = child.id();
                    if let Some(identity) = ProcessIdentity::lookup(pid) {
                        let _ = std::fs::create_dir_all(record.parent().unwrap_or(&record));
                        let _ = std::fs::write(
                            &record,
                            serde_json::to_vec(&identity).unwrap_or_default(),
                        );
                    }
                    let _ = started.send(Ok(pid));
                    loop {
                        if let Ok(Some(_)) = child.try_wait() {
                            break;
                        }
                        match stopping.recv_timeout(Duration::from_millis(200)) {
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            // Asked to stop, or the runner is gone.
                            _ => {
                                #[cfg(unix)]
                                signal_group(pid, "TERM");
                                let deadline = Instant::now() + STOP_GRACE;
                                while !matches!(child.try_wait(), Ok(Some(_)))
                                    && Instant::now() < deadline
                                {
                                    std::thread::sleep(Duration::from_millis(50));
                                }
                                #[cfg(unix)]
                                signal_group(pid, "KILL");
                                let _ = child.kill();
                                let _ = child.wait();
                                break;
                            }
                        }
                    }
                    let _ = std::fs::remove_file(&record);
                    exited.store(true, Ordering::SeqCst);
                }
            })?;
        let pid = pid
            .recv()
            .map_err(|_| crate::Error::Invalid(crate::tr!("the streaming host didn't start")))??;
        tracing::info!(pid, "streaming host started");
        *crate::lock(&self.running) = Some(Running {
            exited,
            stop,
            thread,
        });
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        let running = crate::lock(&self.running).take();
        if let Some(running) = running {
            let _ = running.stop.send(());
            let _ = running.thread.join();
            tracing::info!("streaming host stopped");
        }
        Ok(())
    }

    fn running(&self) -> bool {
        crate::lock(&self.running)
            .as_ref()
            .is_some_and(|running| !running.exited.load(Ordering::SeqCst))
    }
}

impl Drop for ChildRunner {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Windows: the on-demand service.
pub struct ServiceRunner;

fn sc(args: &[&str]) -> Option<std::process::Output> {
    std::process::Command::new("sc.exe")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
}

/// Whether `sc query` output says the service runs or is starting.
pub fn service_running(query: &str) -> bool {
    query
        .lines()
        .find(|line| line.trim_start().starts_with("STATE"))
        .is_some_and(|line| line.contains("RUNNING") || line.contains("START_PENDING"))
}

impl HostRunner for ServiceRunner {
    fn start(&self, _launch: &HostLaunch) -> Result<()> {
        // One left running by a crash is restarted fresh.
        if self.running() {
            self.stop()?;
        }
        let service = crate::elevation::windows::SERVICE;
        match sc(&["start", service]) {
            Some(output) if output.status.success() => Ok(()),
            // 1056: already running.
            Some(output) if output.status.code() == Some(1056) => Ok(()),
            _ => Err(crate::Error::Invalid(crate::tr!(
                "the streaming host didn't start"
            ))),
        }
    }

    fn stop(&self) -> Result<()> {
        let service = crate::elevation::windows::SERVICE;
        let _ = sc(&["stop", service]);
        let deadline = Instant::now() + STOP_GRACE * 2;
        while self.running() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
        }
        Ok(())
    }

    fn running(&self) -> bool {
        sc(&["query", crate::elevation::windows::SERVICE])
            .is_some_and(|output| service_running(&String::from_utf8_lossy(&output.stdout)))
    }
}
