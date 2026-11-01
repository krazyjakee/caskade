//! Finalizers and external extractors.
use super::*;

struct TestFinalizer {
    fail: bool,
}

impl exec::Finalizer for TestFinalizer {
    fn finalize(&self, context: &exec::FinalizationContext<'_>) -> exec::Result<()> {
        assert!(context.game_dir.join("game.bin").is_file());
        assert!(
            !context.final_dir.exists(),
            "discovery and manifests must precede commit"
        );
        assert_eq!(context.relative_dir, Path::new("Fixture Game"));
        fs::write(context.game_dir.join("manifest.json"), b"manifest")?;
        context.steps.checkpoint(0)?;
        if self.fail {
            return Err(ExecError::InvalidRequest {
                message: "finalization failed".into(),
            });
        }
        Ok(())
    }
}

#[tokio::test]
async fn discovery_and_manifest_finalization_are_part_of_the_staged_transaction() {
    for fail in [false, true] {
        let root = TestRoot::new().unwrap();
        let worker = executor(
            Arc::new(FakeRuntime::default()),
            Arc::new(RecordingSink::default()),
        )
        .with_finalizer(Arc::new(TestFinalizer { fail }));
        let result = worker
            .execute(
                request(&root, vec![candidate(happy_steps())]),
                CancellationToken::new(),
            )
            .await;
        if fail {
            assert!(result.is_err());
        } else {
            let report = result.unwrap();
            assert_eq!(
                fs::read(report.dir.join("manifest.json")).unwrap(),
                b"manifest"
            );
            fs::remove_dir_all(report.dir).unwrap();
        }
        root.assert_clean();
    }
}

struct ScratchExtractor;

impl Extractor for ScratchExtractor {
    fn extract(
        &self,
        _: &ArtifactId,
        _: &[String],
        _: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> exec::Result<()> {
        let scratch = context.create_scratch()?;
        assert!(
            scratch
                .path()
                .parent()
                .unwrap()
                .ends_with(Path::new(".caskade/staging/test-job"))
        );
        fs::write(scratch.path().join("quarantined"), b"safe")?;
        let mut writer = context.open_destination(destination, 4)?;
        writer.write_file(
            "imported",
            fs::File::open(scratch.path().join("quarantined"))?,
            Some(4),
            0,
            0o644,
        )?;
        context.progress(4, 1)?;
        drop(scratch);
        Ok(())
    }
}

#[tokio::test]
async fn external_extractor_scratch_is_private_to_staging_and_never_committed() {
    let root = TestRoot::new().unwrap();
    let worker = executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(Arc::new(ScratchExtractor));
    let report = worker
        .execute(
            request(&root, vec![candidate(vec![extraction()])]),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(fs::read(report.dir.join("imported")).unwrap(), b"safe");
    assert_eq!(fs::read_dir(&report.dir).unwrap().count(), 1);
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}

#[derive(Default)]
struct RoutedExtractor(Mutex<Vec<HandlerId>>);

impl Extractor for RoutedExtractor {
    fn extract(
        &self,
        _: &ArtifactId,
        _: &[String],
        _: Option<&Path>,
        to: &Path,
        context: &StepContext,
    ) -> exec::Result<()> {
        let handler = context.handler_id().expect("active candidate handler");
        self.0.lock().unwrap().push(handler.clone());
        let mut writer = context.open_destination(to, 5)?;
        if handler.0 == "native" {
            writer.write_file("native-partial", b"hello".as_slice(), Some(5), 0, 0o644)?;
            return Err(ExecError::InvalidRequest {
                message: "native extraction failed".into(),
            });
        }
        writer.write_file("game.bin", b"hello".as_slice(), Some(5), 0, 0o644)?;
        context.progress(5, 1)
    }
}

#[tokio::test]
async fn extractors_can_route_each_fallback_attempt_by_active_handler() {
    let root = TestRoot::new().unwrap();
    let extractor = Arc::new(RoutedExtractor::default());
    let mut native = candidate(vec![extraction()]);
    native.handler = HandlerId("native".into());
    let mut sidecar = candidate(vec![extraction()]);
    sidecar.handler = HandlerId("sidecar".into());
    let report = executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(extractor.clone())
    .execute(
        request(&root, vec![native, sidecar]),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        *extractor.0.lock().unwrap(),
        vec![HandlerId("native".into()), HandlerId("sidecar".into())]
    );
    assert_eq!(report.candidate_index, 1);
    assert!(!report.dir.join("native-partial").exists());
    assert_eq!(fs::read(report.dir.join("game.bin")).unwrap(), b"hello");
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}

struct ReferenceExtractor;

impl Extractor for ReferenceExtractor {
    fn extract(
        &self,
        _: &ArtifactId,
        _: &[String],
        _: Option<&Path>,
        to: &Path,
        context: &StepContext,
    ) -> exec::Result<()> {
        let base = context.final_destination_path(Path::new("."))?;
        let target = context.final_destination_path(Path::new("bin/game"))?;
        assert_eq!(target, base.join("bin/game"));
        assert!(
            context
                .final_destination_path(Path::new("../outside"))
                .is_err()
        );
        let text = target.to_string_lossy();
        let mut writer = context.open_destination(to, text.len() as u64)?;
        writer.write_file(
            "launcher-reference",
            text.as_bytes(),
            Some(text.len() as u64),
            0,
            0o644,
        )?;
        context.progress(text.len() as u64, 1)
    }
}

#[tokio::test]
async fn generated_references_use_the_collision_adjusted_final_tree() {
    let root = TestRoot::new().unwrap();
    fs::create_dir(root.path().join("Fixture Game")).unwrap();
    let report = executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(Arc::new(ReferenceExtractor))
    .execute(
        request(&root, vec![candidate(vec![extraction()])]),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let reference = fs::read_to_string(report.dir.join("launcher-reference")).unwrap();
    assert_eq!(Path::new(&reference), report.dir.join("bin/game"));
    assert!(!reference.contains(".caskade/staging"));
    fs::remove_dir_all(report.dir).unwrap();
    fs::remove_dir(root.path().join("Fixture Game")).unwrap();
    root.assert_clean();
}
