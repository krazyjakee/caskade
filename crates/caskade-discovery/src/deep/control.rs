use super::{Options, Progress, ProgressSink, ScanKind};
use crate::{HostEnv, ScanCtx, SourceError};
use std::{
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    time::{Duration, Instant},
};

pub(super) struct Control<'a> {
    pub env: &'a HostEnv,
    pub ctx: &'a ScanCtx,
    pub options: &'a Options,
    pub kind: ScanKind,
    pub visited: u64,
    pub path_bytes: usize,
    started: Duration,
    real: Instant,
    progress: &'a ProgressSink<'a>,
}
impl<'a> Control<'a> {
    pub fn new(
        env: &'a HostEnv,
        ctx: &'a ScanCtx,
        options: &'a Options,
        kind: ScanKind,
        progress: &'a ProgressSink<'a>,
    ) -> Self {
        Self {
            env,
            ctx,
            options,
            kind,
            visited: 0,
            path_bytes: 0,
            started: env.monotonic(),
            real: Instant::now(),
            progress,
        }
    }
    pub fn check(&self) -> Result<(), SourceError> {
        self.ctx.check()?;
        let limit = Duration::from_millis(self.options.budget_ms);
        if self.real.elapsed() >= limit
            || self.env.monotonic().saturating_sub(self.started) >= limit
        {
            return Err(SourceError::TimedOut);
        }
        Ok(())
    }
    pub fn entry(&mut self, root: &Path, candidates: usize) -> Result<(), SourceError> {
        self.check()?;
        if self.visited >= self.options.max_entries || self.path_bytes > 16 * 1024 * 1024 {
            return Err(SourceError::parse(
                root,
                "deep entry/path byte budget reached",
            ));
        }
        self.visited += 1;
        self.emit(root, candidates, false);
        self.check()
    }
    pub fn emit(&self, root: &Path, candidates: usize, done: bool) {
        (self.progress)(Progress {
            kind: self.kind,
            root: root.into(),
            visited: self.visited,
            candidates,
            done,
        });
    }
    pub fn reader<R: Read + Seek>(&self, inner: R) -> Reader<'_, R> {
        Reader {
            inner,
            control: self,
        }
    }
}
/// Archive metadata readers remain interruptible at each read and seek.
pub(super) struct Reader<'a, R> {
    inner: R,
    control: &'a Control<'a>,
}
impl<R> Reader<'_, R> {
    fn check(&self) -> io::Result<()> {
        self.control.check().map_err(|e| {
            io::Error::new(
                if matches!(e, SourceError::Cancelled) {
                    // read_exact retries Interrupted; cancellation must abort.
                    io::ErrorKind::Other
                } else {
                    io::ErrorKind::TimedOut
                },
                e,
            )
        })
    }
}
impl<R: Read> Read for Reader<'_, R> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        self.inner.read(b)
    }
}
impl<R: Seek> Seek for Reader<'_, R> {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.check()?;
        self.inner.seek(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cancel;
    use std::{io::Cursor, sync::mpsc};

    #[test]
    fn cancelled_archive_read_exact_aborts_instead_of_retrying_interrupted() {
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let temp = tempfile::tempdir().unwrap();
            std::fs::create_dir(temp.path().join("root")).unwrap();
            let env = HostEnv::fixture(temp.path()).unwrap();
            let cancel = Cancel::new();
            cancel.cancel();
            let ctx = ScanCtx::new(cancel);
            let options = Options::default();
            let control = Control::new(&env, &ctx, &options, ScanKind::Portable, &|_| {});
            let mut reader = control.reader(Cursor::new(vec![0u8; 64]));
            let result = reader.read_exact(&mut [0; 64]);
            let _ = send.send(result.is_err());
        });
        assert!(receive.recv_timeout(Duration::from_millis(200)).unwrap());
    }
}
