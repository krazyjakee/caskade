//! Host-native process runtime. Commands run with an explicit argv, env
//! overrides and cwd, never through a shell and never self-elevated: an
//! installer that demands elevation fails (Windows error 740) unless the user
//! already runs elevated, so any UAC decision stays with the OS and the user.
//!
//! Every process gets its own kill scope (a Unix process group, a Windows job
//! object closed with kill-on-close), so `kill` also ends descendants after
//! the parent has exited. Unix exit is observed without reaping, keeping the
//! group ID reserved until `kill` reaps it, so a recycled ID is never signalled.
use super::{ProcessCommand, ProcessExit, ProcessId, Runtime, RuntimeError};
use std::{
    collections::BTreeMap,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(20);

#[derive(Debug, Default)]
pub struct NativeRuntime {
    next: AtomicU64,
    processes: Mutex<BTreeMap<ProcessId, Arc<Process>>>,
}

#[derive(Debug)]
struct Process {
    child: Mutex<Child>,
    exit: Mutex<Option<ProcessExit>>,
    killed: AtomicBool,
    #[cfg(windows)]
    job: Mutex<Option<win32job::Job>>,
}

fn io(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::Io {
        message: error.to_string(),
    }
}

/// How one argument reaches a Windows command line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WindowsArg {
    /// Standard `CommandLineToArgvW` quoting.
    Quoted(String),
    /// Appended verbatim.
    Raw(String),
}

/// Installers parse their own command line. NSIS reads `/D=` (and its
/// uninstaller's `_?=`) raw to the end, so those go unquoted and must be the
/// final argument; Inno's `/DIR="..."` wrapper quotes are normalised to a
/// standard argument. Any other embedded quote, and control characters, are
/// rejected rather than passed through.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_arg(arg: &str, last: bool) -> Result<WindowsArg, RuntimeError> {
    let invalid = |reason: &str| io(format!("invalid installer argument {arg:?}: {reason}"));
    if arg.chars().any(char::is_control) {
        return Err(invalid("control characters"));
    }
    let upper = arg.to_ascii_uppercase();
    if upper.starts_with("/D=") || arg.starts_with("_?=") {
        if !last {
            return Err(invalid("a raw directory tail must be the final argument"));
        }
        if arg.contains('"') {
            return Err(invalid("quotes in a raw directory tail"));
        }
        return Ok(WindowsArg::Raw(arg.into()));
    }
    if upper.starts_with("/DIR=\"") && arg.ends_with('"') && arg.len() > 7 {
        let inner = &arg[6..arg.len() - 1];
        if inner.contains('"') {
            return Err(invalid("embedded quotes"));
        }
        return Ok(WindowsArg::Quoted(format!("{}{inner}", &arg[..5])));
    }
    if arg.contains('"') {
        return Err(invalid("embedded quotes"));
    }
    Ok(WindowsArg::Quoted(arg.into()))
}

impl NativeRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// The operating-system process ID of a running or unreaped process.
    pub fn system_pid(&self, id: ProcessId) -> Result<u32, RuntimeError> {
        Ok(self
            .process(id)?
            .child
            .lock()
            .expect("native process poisoned")
            .id())
    }

    fn start(
        &self,
        command: &ProcessCommand,
        input: Option<&[u8]>,
    ) -> Result<ProcessId, RuntimeError> {
        let mut builder = Command::new(&command.program);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let last = command.args.len().saturating_sub(1);
            for (index, arg) in command.args.iter().enumerate() {
                match windows_arg(arg, index == last)? {
                    WindowsArg::Quoted(arg) => builder.arg(arg),
                    WindowsArg::Raw(arg) => builder.raw_arg(arg),
                };
            }
        }
        #[cfg(not(windows))]
        builder.args(&command.args);
        builder
            .envs(&command.env)
            .current_dir(&command.cwd)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut builder, 0);
        #[cfg(windows)]
        let job = {
            let mut limits = win32job::ExtendedLimitInfo::new();
            limits.limit_kill_on_job_close();
            win32job::Job::create_with_limit_info(&limits).map_err(io)?
        };
        let child = builder.spawn().map_err(|error| {
            if cfg!(windows) && error.raw_os_error() == Some(740) {
                io(
                    "the installer requires administrator elevation (Windows error 740); \
                    Caskade never elevates itself",
                )
            } else {
                io(format!("{}: {error}", command.program.display()))
            }
        })?;
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            // A child that spawns before assignment escapes the job; the
            // executor's timeout and cancellation still end the parent.
            job.assign_process(child.as_raw_handle() as isize)
                .map_err(io)?;
        }
        let mut child = child;
        if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
            use std::io::Write;
            // Bounded below the pipe buffer; a child that exits early is fine.
            let _ = stdin.write_all(input);
        }
        let id = ProcessId(self.next.fetch_add(1, Ordering::Relaxed) + 1);
        self.processes
            .lock()
            .expect("native runtime poisoned")
            .insert(
                id,
                Arc::new(Process {
                    child: Mutex::new(child),
                    exit: Mutex::new(None),
                    killed: AtomicBool::new(false),
                    #[cfg(windows)]
                    job: Mutex::new(Some(job)),
                }),
            );
        Ok(id)
    }

    fn process(&self, id: ProcessId) -> Result<Arc<Process>, RuntimeError> {
        self.processes
            .lock()
            .expect("native runtime poisoned")
            .get(&id)
            .cloned()
            .ok_or(RuntimeError::UnknownProcess { id })
    }

    /// Exit status without reaping (Unix) or from the process handle (Windows).
    fn poll(process: &Process) -> Result<Option<ProcessExit>, RuntimeError> {
        let mut exit = process.exit.lock().expect("native process poisoned");
        if exit.is_some() {
            return Ok(*exit);
        }
        let child = process.child.lock().expect("native process poisoned");
        #[cfg(unix)]
        let status = {
            use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
            let pid = Pid::from_raw(child.id() as i32).ok_or_else(|| io("invalid child pid"))?;
            waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            )
            .map_err(io)?
            .map(|status| ProcessExit {
                code: status
                    .exit_status()
                    .or_else(|| status.terminating_signal().map(|signal| 128 + signal))
                    .unwrap_or(-1),
            })
        };
        #[cfg(not(unix))]
        let status = {
            let mut child = child;
            child.try_wait().map_err(io)?.map(|status| ProcessExit {
                code: status.code().unwrap_or(-1),
            })
        };
        *exit = status;
        Ok(status)
    }
}

impl Runtime for NativeRuntime {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        self.start(command, None)
    }

    fn spawn_with_input(
        &self,
        command: &ProcessCommand,
        input: &[u8],
    ) -> Result<ProcessId, RuntimeError> {
        if input.len() > super::MAX_STDIN_BYTES {
            return Err(io("process input exceeds the stdin limit"));
        }
        self.start(command, Some(input))
    }

    fn wait(&self, id: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError> {
        let process = self.process(id)?;
        let deadline = Instant::now() + timeout;
        loop {
            if process.killed.load(Ordering::Acquire) {
                return Err(RuntimeError::Killed { id });
            }
            match Self::poll(&process) {
                Ok(Some(exit)) => return Ok(exit),
                Ok(None) => {}
                // A concurrent kill may reap the child between the two checks.
                Err(_) if process.killed.load(Ordering::Acquire) => {
                    return Err(RuntimeError::Killed { id });
                }
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                self.kill(id)?;
                return Err(RuntimeError::TimedOut { id });
            }
            std::thread::sleep(POLL);
        }
    }

    fn kill(&self, id: ProcessId) -> Result<(), RuntimeError> {
        let Some(process) = self
            .processes
            .lock()
            .expect("native runtime poisoned")
            .remove(&id)
        else {
            return Ok(());
        };
        let exited = Self::poll(&process)?.is_some();
        process.killed.store(!exited, Ordering::Release);
        let mut child = process.child.lock().expect("native process poisoned");
        #[cfg(unix)]
        {
            use rustix::process::{Pid, Signal, kill_process_group};
            // The unreaped leader keeps this group ID reserved.
            if let Some(group) = Pid::from_raw(child.id() as i32) {
                match kill_process_group(group, Signal::KILL) {
                    Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                    // Only processes of another user are left (pkexec's
                    // command runs as root); they are not ours to end.
                    Err(rustix::io::Errno::PERM) if exited => {}
                    Err(error) => return Err(io(error)),
                }
            }
        }
        #[cfg(windows)]
        drop(process.job.lock().expect("native process poisoned").take());
        if !exited {
            let _ = child.kill();
        }
        child.wait().map_err(io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_directory_arguments_follow_each_family_parser() {
        let raw = |arg: &str| WindowsArg::Raw(arg.into());
        let quoted = |arg: &str| WindowsArg::Quoted(arg.into());
        // NSIS: raw, unquoted, final.
        assert_eq!(
            windows_arg(r"/D=C:\Games\Café Space", true).unwrap(),
            raw(r"/D=C:\Games\Café Space")
        );
        assert_eq!(
            windows_arg(r"_?=C:\Games\A B", true).unwrap(),
            raw(r"_?=C:\Games\A B")
        );
        assert!(windows_arg(r"/D=C:\x", false).is_err());
        assert!(windows_arg(r#"/D="C:\x""#, true).is_err());
        // Inno: wrapper quotes normalised, then standard quoting.
        assert_eq!(
            windows_arg(r#"/DIR="C:\Games\Café Space""#, true).unwrap(),
            quoted(r"/DIR=C:\Games\Café Space")
        );
        assert_eq!(
            windows_arg(r"/DIR=G:\Game", false).unwrap(),
            quoted(r"/DIR=G:\Game")
        );
        assert!(windows_arg(r#"/DIR="C:\a"b""#, true).is_err());
        // Everything else: standard quoting; no embedded quotes or controls.
        assert_eq!(
            windows_arg("/VERYSILENT", false).unwrap(),
            quoted("/VERYSILENT")
        );
        assert!(windows_arg("say \"hi\"", false).is_err());
        assert!(windows_arg("a\nb", false).is_err());
    }
}
