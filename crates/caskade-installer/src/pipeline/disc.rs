use super::*;
use crate::{
    disk::DiskSpace,
    formats::{
        archive::{ArchiveError, EventProgress, ExtractionControl},
        disc::DiscSession,
    },
};

pub(super) async fn open(
    prepared: &input::Prepared,
    job: &str,
    ctx: &Context,
) -> PipelineResult<Option<DiscSession>> {
    if !matches!(
        prepared.artifact.format,
        crate::Format::Iso | crate::Format::Cue | crate::Format::Bin | crate::Format::Chd
    ) {
        return Ok(None);
    }
    let tool = ctx
        .tool
        .clone()
        .ok_or("disc input requires a verified 7-Zip sidecar (--seven-zip)")?;
    let path = prepared.path.clone();
    let job = job.to_owned();
    let cancel = ctx.cancel.clone();
    let sink = ctx.sink.clone();
    let session = caskade_jobs::blocking(
        "Reading disc image",
        move || -> PipelineResult<DiscSession> {
            let scratch = std::env::temp_dir();
            let mut control = DiscControl {
                progress: EventProgress::new(&job, &cancel, sink.as_ref()),
                scratch: &scratch,
            };
            Ok(DiscSession::open(
                &path,
                &tool,
                &scratch,
                crate::safety::Limits::default(),
                &mut control,
            )?)
        },
    )
    .await??;
    Ok(Some(session))
}

struct DiscControl<'a> {
    progress: EventProgress<'a>,
    scratch: &'a Path,
}
impl ExtractionControl for DiscControl<'_> {
    fn checkpoint(&mut self, next_bytes: u64) -> crate::formats::archive::Result<()> {
        self.progress.checkpoint(next_bytes)?;
        if FileSystemDiskSpace.available_bytes(self.scratch)?
            < next_bytes.saturating_add(16 * 1024 * 1024)
        {
            return Err(ArchiveError::Io(std::io::Error::other(
                "insufficient disc scratch space",
            )));
        }
        Ok(())
    }
    fn progress(&mut self, bytes: u64, files: u64) -> crate::formats::archive::Result<()> {
        self.checkpoint(0)?;
        self.progress.progress(bytes, files)
    }
}
