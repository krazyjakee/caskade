use super::{DmgError, Result};
use crate::{
    formats::archive::ExtractionControl,
    runtime::{ProcessCommand, ProcessExit, ProcessId, Runtime},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(15);
/// Owns a private mount point. Detach ignores cancellation and escalates to
/// force only after a normal failure. A still-mounted directory is retained,
/// never recursively deleted or included in the game's staging rollback.
pub struct MountedImage {
    runtime: Arc<dyn Runtime>,
    tool: PathBuf,
    mount: PathBuf,
    temp: Option<tempfile::TempDir>,
    process: Option<ProcessId>,
    attach_finished: bool,
    active: bool,
}
impl MountedImage {
    pub fn attach(
        runtime: Arc<dyn Runtime>,
        tool: &Path,
        image: &Path,
        control: &mut dyn ExtractionControl,
        timeout: Duration,
    ) -> Result<Self> {
        control.checkpoint(0)?;
        if timeout.is_zero() {
            return Err(DmgError::Invalid("zero mount timeout".into()));
        }
        if !image.is_absolute() || !tool.is_absolute() {
            return Err(DmgError::Unsupported(
                "absolute injected image/tool paths required".into(),
            ));
        }
        let image_argument = image
            .to_str()
            .ok_or_else(|| DmgError::Unsupported("non-UTF-8 image path".into()))?
            .to_owned();
        let temp = tempfile::Builder::new().prefix("caskade-dmg-").tempdir()?;
        let mount = temp.path().join("volume");
        let mount_argument = mount
            .to_str()
            .ok_or_else(|| DmgError::Unsupported("non-UTF-8 temporary mount path".into()))?
            .to_owned();
        std::fs::create_dir(&mount)?;
        let mut guard = Self {
            runtime,
            tool: tool.into(),
            mount,
            temp: Some(temp),
            process: None,
            attach_finished: false,
            active: true,
        };
        let command = ProcessCommand {
            program: tool.into(),
            args: vec![
                "attach".into(),
                "-readonly".into(),
                "-nobrowse".into(),
                "-noautoopen".into(),
                "-mountpoint".into(),
                mount_argument,
                "-verify".into(),
                image_argument,
            ],
            env: BTreeMap::new(),
            cwd: guard.temp.as_ref().unwrap().path().into(),
        };
        // The fixed q leaves hdiutil's SLA pager; y accepts its subsequent
        // agreement question. Ordinary images may close stdin immediately.
        let process = match guard.runtime.spawn_with_input(&command, b"qy\n") {
            Ok(process) => process,
            Err(error) => {
                guard.active = false;
                return Err(error.into());
            }
        };
        guard.process = Some(process);
        let exit = guard.wait_attach(process, control, timeout)?;
        guard.attach_finished = true;
        if exit.code != 0 {
            return Err(DmgError::Process {
                operation: "attach",
                code: exit.code,
            });
        }
        control.checkpoint(0)?;
        Ok(guard)
    }
    fn wait_attach(
        &self,
        id: ProcessId,
        control: &mut dyn ExtractionControl,
        timeout: Duration,
    ) -> Result<ProcessExit> {
        let runtime = self.runtime.clone();
        let (send, receive) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let _ = send.send(runtime.wait(id, timeout));
        });
        let result = loop {
            if let Err(error) = control.checkpoint(0) {
                let _ = self.runtime.kill(id);
                break Err(DmgError::Control(error));
            }
            match receive.recv_timeout(Duration::from_millis(20)) {
                Ok(result) => break result.map_err(Into::into),
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break Err(DmgError::Invalid("hdiutil waiter disconnected".into()));
                }
            }
        };
        if result.is_err() {
            let _ = self.runtime.kill(id);
        }
        worker
            .join()
            .map_err(|_| DmgError::Invalid("hdiutil waiter panicked".into()))?;
        result
    }
    pub fn mountpoint(&self) -> &Path {
        &self.mount
    }
    fn detach_command(&self, force: bool) -> Result<()> {
        let mut args = vec![
            "detach".into(),
            self.mount
                .to_str()
                .expect("validated mount path")
                .to_owned(),
        ];
        if force {
            args.push("-force".into());
        }
        let command = ProcessCommand {
            program: self.tool.clone(),
            args,
            env: BTreeMap::new(),
            cwd: self.temp.as_ref().unwrap().path().into(),
        };
        let id = self.runtime.spawn(&command)?;
        let exit = self.runtime.wait(id, CLEANUP_TIMEOUT);
        let killed = self.runtime.kill(id);
        let exit = exit?;
        killed?;
        if exit.code != 0 {
            return Err(DmgError::Process {
                operation: "detach",
                code: exit.code,
            });
        }
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        // Stop a cancelled/incomplete attach before attempting detach, so it
        // cannot mount a filesystem after cleanup has already completed.
        if !self.attach_finished
            && let Some(id) = self.process.take()
        {
            let _ = self.runtime.kill(id);
        }
        let result = self
            .detach_command(false)
            .or_else(|_| self.detach_command(true));
        if let Some(id) = self.process.take() {
            let _ = self.runtime.kill(id);
        }
        match result {
            Ok(()) => {
                self.active = false;
                Ok(())
            }
            Err(error) => Err(DmgError::Detach {
                mountpoint: self.mount.clone(),
                message: error.to_string(),
            }),
        }
    }
    pub fn detach(mut self) -> Result<()> {
        self.finish()
    }
}
impl Drop for MountedImage {
    fn drop(&mut self) {
        if self.finish().is_err()
            && let Some(temp) = self.temp.take()
        {
            let _retained_mount = temp.keep();
        }
    }
}
