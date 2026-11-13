//! Running a script's process: no shell unless the script is one, stdin
//! closed, output kept within bounds, a time limit and cancellation. On
//! Unix the script gets its own process group, so a time limit or a
//! cancel also stops what it started.
use super::{ExecOutcome, ScriptStatus};
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

/// The output kept of each stream: its last 64 KiB.
pub const OUTPUT_LIMIT: usize = 64 * 1024;

/// One process to run. The environment adds to Caskade's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// On Windows, `args` go onto the command line as written, joined by
    /// spaces, for programs that parse it themselves (cmd). Elsewhere, and
    /// when false, each is passed as one argument.
    pub verbatim_args: bool,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub timeout: Duration,
}

#[async_trait]
pub trait ScriptExecutor: Send + Sync {
    async fn run(&self, request: ScriptRequest, cancel: CancellationToken) -> ExecOutcome;
}

/// Runs scripts as processes.
#[derive(Debug, Clone, Default)]
pub struct ProcessExecutor;

/// `program` on `PATH` (or in `search_path` when one is set: tests), with
/// Windows' extensions, or as given when it is a path. `None` when it isn't
/// there.
pub fn find_program(program: &Path, search_path: Option<&[PathBuf]>) -> Option<PathBuf> {
    if program.components().count() > 1 || program.is_absolute() {
        return program.is_file().then(|| program.to_owned());
    }
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .map(str::to_owned)
            .chain([String::new()])
            .collect()
    } else {
        vec![String::new()]
    };
    let dirs: Vec<PathBuf> = match search_path {
        Some(dirs) => dirs.to_vec(),
        None => std::env::split_paths(&std::env::var_os("PATH")?).collect(),
    };
    dirs.iter().find_map(|dir| {
        extensions.iter().find_map(|ext| {
            let mut name = program.as_os_str().to_owned();
            name.push(ext);
            let candidate = dir.join(name);
            candidate.is_file().then_some(candidate)
        })
    })
}

fn not_started(error: String) -> ExecOutcome {
    ExecOutcome {
        status: ScriptStatus::NotStarted,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        truncated: false,
        error: Some(error),
    }
}

#[async_trait]
impl ScriptExecutor for ProcessExecutor {
    async fn run(&self, request: ScriptRequest, cancel: CancellationToken) -> ExecOutcome {
        let Some(program) = find_program(&request.program, None) else {
            return not_started(crate::tr!(
                "“{program}” wasn't found; check that it is installed and the path is right",
                program = request.program.display()
            ));
        };
        if let Some(cwd) = &request.cwd
            && !cwd.is_dir()
        {
            return not_started(crate::tr!(
                "the working folder “{path}” doesn't exist",
                path = cwd.display()
            ));
        }
        let mut command = tokio::process::Command::from(crate::quiet::command(&program));
        crate::managed::outside_own_appimage(command.as_std_mut());
        #[cfg(windows)]
        if request.verbatim_args {
            for arg in &request.args {
                command.raw_arg(arg);
            }
        } else {
            command.args(&request.args);
        }
        #[cfg(not(windows))]
        command.args(&request.args);
        command
            .envs(&request.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = &request.cwd {
            command.current_dir(cwd);
        }
        #[cfg(unix)]
        command.process_group(0);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => return not_started(error.to_string()),
        };
        let pid = child.id();
        let stdout = tokio::spawn(read_bounded(child.stdout.take()));
        let stderr = tokio::spawn(read_bounded(child.stderr.take()));
        let (status, exit_code) = tokio::select! {
            waited = child.wait() => match waited {
                Ok(status) if status.success() => (ScriptStatus::Succeeded, status.code()),
                Ok(status) => (ScriptStatus::Failed, status.code()),
                Err(error) => {
                    stop(&mut child, pid).await;
                    return not_started(error.to_string());
                }
            },
            _ = tokio::time::sleep(request.timeout) => {
                stop(&mut child, pid).await;
                (ScriptStatus::TimedOut, None)
            }
            _ = cancel.cancelled() => {
                stop(&mut child, pid).await;
                (ScriptStatus::Cancelled, None)
            }
        };
        // Whatever the script left running with its output open (killed
        // above when it ran out of time or was cancelled) can't hold the
        // result back for long.
        let collect = |task: tokio::task::JoinHandle<(String, bool)>| async move {
            match tokio::time::timeout(Duration::from_secs(2), task).await {
                Ok(Ok(read)) => read,
                _ => (String::new(), false),
            }
        };
        let (stdout, out_cut) = collect(stdout).await;
        let (stderr, err_cut) = collect(stderr).await;
        ExecOutcome {
            status,
            exit_code,
            stdout,
            stderr,
            truncated: out_cut || err_cut,
            error: match status {
                ScriptStatus::TimedOut => Some(crate::tr!(
                    "it didn't finish within {seconds} seconds and was stopped",
                    seconds = request.timeout.as_secs()
                )),
                _ => None,
            },
        }
    }
}

/// Stops the script and, on Unix, everything in its process group.
async fn stop(child: &mut tokio::process::Child, pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        // The script leads its own group (`process_group(0)`): signal the
        // whole group, without unsafe code.
        let _ = tokio::process::Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    #[cfg(not(unix))]
    let _ = pid;
    let _ = child.kill().await;
}

/// Reads a stream to its end, keeping its last [`OUTPUT_LIMIT`] bytes as
/// text without control characters (tabs and line breaks stay).
async fn read_bounded(stream: Option<impl AsyncRead + Unpin>) -> (String, bool) {
    let Some(mut stream) = stream else {
        return (String::new(), false);
    };
    let mut kept: Vec<u8> = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                kept.extend_from_slice(&buffer[..read]);
                if kept.len() > OUTPUT_LIMIT {
                    kept.drain(..kept.len() - OUTPUT_LIMIT);
                    truncated = true;
                }
            }
        }
    }
    let text = String::from_utf8_lossy(&kept)
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    (text, truncated)
}
