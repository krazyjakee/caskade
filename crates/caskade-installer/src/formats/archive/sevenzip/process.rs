use super::{Result, SevenZip, SevenZipError};
use crate::formats::archive::ExtractionControl;
use std::{
    ffi::OsString,
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

enum Message {
    Data(bool, Vec<u8>),
    Failure(std::io::Error),
}
struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            if let Some(pid) = rustix::process::Pid::from_raw(self.0.id() as i32) {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
        }
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/T", "/F", "/PID", &self.0.id().to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn pump(
    mut reader: impl Read + Send + 'static,
    stderr: bool,
    send: mpsc::SyncSender<Message>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if send
                        .send(Message::Data(stderr, buffer[..count].to_vec()))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    let _ = send.send(Message::Failure(error));
                    break;
                }
            }
        }
    })
}

impl SevenZip {
    pub(super) fn run(
        &self,
        args: &[OsString],
        scratch: Option<&Path>,
        control: &mut dyn ExtractionControl,
    ) -> Result<Vec<u8>> {
        control.checkpoint(0)?;
        let mut command = Command::new(&self.program);
        crate::runtime::outside_appimage(&mut command);
        // A single queued extraction must not start one decoder per host CPU.
        // Leave a core available where possible, and use at most two threads.
        let threads = thread::available_parallelism()
            .map_or(1, |cores| cores.get().saturating_sub(1).clamp(1, 2));
        command
            .args(args.iter().take(1))
            .arg(format!("-mmt={threads}"))
            .args(args.iter().skip(1))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("LC_ALL", "C.UTF-8")
            .env("LANG", "C.UTF-8");
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = Running(command.spawn()?);
        let (send, receive) = mpsc::sync_channel(32);
        let stdout = pump(
            child.0.stdout.take().expect("piped stdout"),
            false,
            send.clone(),
        );
        let stderr = pump(child.0.stderr.take().expect("piped stderr"), true, send);
        let start = Instant::now();
        let mut next_scan = Instant::now();
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let mut progress = Vec::new();
        let mut last_percent = None;
        let mut exit = None;
        let mut disconnected = false;
        loop {
            control.checkpoint(0)?;
            if start.elapsed() > self.timeout {
                return Err(SevenZipError::TimedOut);
            }
            if next_scan <= Instant::now() {
                if let Some(path) = scratch {
                    super::import::monitor(path, self.limits, control)?;
                }
                next_scan = Instant::now() + Duration::from_millis(25);
            }
            match receive.recv_timeout(Duration::from_millis(10)) {
                Ok(Message::Data(is_error, data)) => {
                    let target = if is_error { &mut errors } else { &mut output };
                    let limit = if is_error {
                        64 * 1024
                    } else {
                        self.options.max_metadata_bytes
                    };
                    if target
                        .len()
                        .checked_add(data.len())
                        .is_none_or(|size| size > limit)
                    {
                        return Err(SevenZipError::OutputLimit);
                    }
                    target.extend_from_slice(&data);
                    if !is_error {
                        for byte in data {
                            if byte == b'%' {
                                if let Ok(text) = std::str::from_utf8(&progress)
                                    && let Ok(percent) = text.parse::<u8>()
                                    && percent <= 100
                                    && last_percent != Some(percent)
                                {
                                    last_percent = Some(percent);
                                    if let Some(callback) = &self.progress {
                                        callback(percent);
                                    }
                                }
                                progress.clear();
                            } else if byte.is_ascii_digit() {
                                if progress.len() < 3 {
                                    progress.push(byte);
                                } else {
                                    progress.clear();
                                }
                            } else {
                                progress.clear();
                            }
                        }
                    }
                }
                Ok(Message::Failure(error)) => return Err(error.into()),
                Err(mpsc::RecvTimeoutError::Disconnected) => disconnected = true,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
            }
            if exit.is_none() {
                exit = child.0.try_wait()?;
            }
            if disconnected && exit.is_some() {
                break;
            }
        }
        stdout
            .join()
            .map_err(|_| SevenZipError::Listing("stdout reader panicked".into()))?;
        stderr
            .join()
            .map_err(|_| SevenZipError::Listing("stderr reader panicked".into()))?;
        let status = exit.expect("process exited");
        if !status.success() {
            return Err(SevenZipError::exit(
                status.code(),
                &String::from_utf8_lossy(&errors),
            ));
        }
        Ok(output)
    }
}
