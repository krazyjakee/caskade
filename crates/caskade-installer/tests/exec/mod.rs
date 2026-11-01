//! Executor tests: shared plans, runtimes and observers.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use caskade_installer::{
    events::{EventSink, JobEvent},
    exec::{
        CancellationToken, ExecConfig, ExecError, ExecRequest, Executor, Extractor, INSTALL_DIR,
        JournalEntry, JournalHandle, ProcessObservation, ProcessObserver, StepContext, folder_name,
    },
    runtime::ProcessCommand,
    testing::{FakeAction, FakeDiskSpace, FakeRuntime, FakeScript, RecordingSink, TestRoot},
    *,
};

mod consent;
mod extensions;
mod files;
mod in_place;
mod native;
mod transaction;

fn candidate(steps: Vec<Step>) -> Candidate {
    Candidate {
        handler: HandlerId("test".into()),
        strategy: Strategy::Extract,
        confidence: 1.0,
        fidelity: Fidelity::Exact,
        requires: Requirements {
            executes_code: false,
            runtime: None,
            may_prompt_elevation: false,
            interactive: false,
            est_disk_bytes: Some(131_077),
        },
        steps,
        warnings: vec![],
    }
}

fn request(root: &TestRoot, candidates: Vec<Candidate>) -> ExecRequest {
    ExecRequest {
        root: root.path().into(),
        job_id: "test-job".into(),
        relative_dir: "Fixture Game".into(),
        plans: PlanSet {
            candidates,
            selected: Some(0),
            question: None,
        },
        sources: BTreeMap::new(),
    }
}

fn executor(runtime: Arc<FakeRuntime>, sink: Arc<dyn EventSink>) -> Executor {
    Executor::new(runtime, Arc::new(FakeDiskSpace::fixed(u64::MAX)), sink).with_config(ExecConfig {
        disk_margin_bytes: 0,
        ..ExecConfig::default()
    })
}

fn happy_steps() -> Vec<Step> {
    vec![
        Step::WriteFile {
            path: "game.bin".into(),
            contents: vec![42; 131_072],
        },
        Step::WriteFile {
            path: "start.sh".into(),
            contents: b"game\n".to_vec(),
        },
        Step::SetExecutable {
            path: "start.sh".into(),
        },
    ]
}

struct CancelAt {
    events: RecordingSink,
    token: CancellationToken,
    index: usize,
    count: AtomicUsize,
}

impl EventSink for CancelAt {
    fn emit(&self, event: JobEvent) {
        self.events.emit(event);
        if self.count.fetch_add(1, Ordering::SeqCst) == self.index {
            self.token.cancel();
        }
    }
}

struct PausedExtractor {
    entered: tokio::sync::Notify,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl Extractor for PausedExtractor {
    fn extract(
        &self,
        _: &ArtifactId,
        _: &[String],
        _: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> exec::Result<()> {
        let mut writer = context.open_destination(destination, 5)?;
        writer.write_file("partial.bin", b"hello".as_slice(), Some(5), 0, 0o644)?;
        self.entered.notify_one();
        self.release.lock().unwrap().recv().unwrap();
        context.checkpoint(0)?;
        context.progress(5, 1)
    }
}

fn extraction() -> Step {
    Step::Extract {
        from: ArtifactId("archive".into()),
        select: vec![],
        strip: None,
        to: ".".into(),
    }
}

fn run(expect: Vec<PathBuf>) -> Step {
    Step::Run {
        program: Program::Source(SourcePath::Artifact {
            artifact: ArtifactId("setup".into()),
            path: ".".into(),
        }),
        args: vec![format!("/DIR={INSTALL_DIR}"), "/LANG=fr".into()],
        env: BTreeMap::new(),
        runtime: RuntimeSpec::Native,
        timeout_secs: 30,
        expect,
    }
}

struct ExternalFileObserver {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

impl ProcessObserver for ExternalFileObserver {
    fn start(
        &self,
        _: &ProcessCommand,
        journal: &JournalHandle,
        _: &StepContext,
    ) -> exec::Result<Box<dyn ProcessObservation>> {
        journal.record(JournalEntry::ExternalFile {
            path: self.path.clone(),
            original: self.original.clone(),
        })?;
        Ok(Box::new(NoopObservation))
    }
}

struct NoopObservation;

impl ProcessObservation for NoopObservation {
    fn finish(self: Box<Self>, _: &JournalHandle) -> exec::Result<()> {
        Ok(())
    }
}
