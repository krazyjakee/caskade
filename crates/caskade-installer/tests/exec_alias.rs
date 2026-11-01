//! #31: executor alias lifecycle with a real native process on Unix.
#![cfg(unix)]

use caskade_installer::{
    exec::{
        self, CancellationToken, ExecError, ExecReport, ExecRequest, Executor, FinalizationContext,
        Finalizer,
    },
    runtime::{NativeRuntime, alias::AliasStore},
    testing::{FakeDiskSpace, RecordingSink, TestRoot},
    *,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// Writes `InstallPath=<INSTALL_DIR>` like an Inno [INI] entry, then exits.
fn native(exit: u32) -> Candidate {
    candidate(
        Strategy::RunSilent,
        Step::Run {
            program: Program::Tool("/bin/sh".into()),
            args: vec![
                "-c".into(),
                r#"printf 'InstallPath=%s\n' "$1" > "$1/config.ini"; exit "$2""#.into(),
                "installer".into(),
                exec::INSTALL_DIR.into(),
                exit.to_string(),
            ],
            env: BTreeMap::new(),
            runtime: RuntimeSpec::Native,
            timeout_secs: 30,
            expect: vec!["config.ini".into()],
        },
    )
}

fn extract() -> Candidate {
    candidate(
        Strategy::Extract,
        Step::WriteFile {
            path: "game.bin".into(),
            contents: b"game".to_vec(),
        },
    )
}

fn candidate(strategy: Strategy, step: Step) -> Candidate {
    Candidate {
        handler: HandlerId("alias-test".into()),
        strategy,
        confidence: 1.0,
        fidelity: Fidelity::Exact,
        requires: Requirements {
            executes_code: strategy == Strategy::RunSilent,
            runtime: Some(RuntimeSpec::Native),
            may_prompt_elevation: false,
            interactive: false,
            est_disk_bytes: Some(64),
        },
        steps: vec![step],
        warnings: vec![],
    }
}

#[derive(Default)]
struct Seen(Mutex<Vec<Option<String>>>);
impl Finalizer for Seen {
    fn finalize(&self, context: &FinalizationContext<'_>) -> exec::Result<()> {
        self.0
            .lock()
            .unwrap()
            .push(context.alias.map(str::to_owned));
        Ok(())
    }
}

async fn run(
    root: &TestRoot,
    store: Option<&AliasStore>,
    candidate: Candidate,
    seen: Arc<Seen>,
) -> std::result::Result<ExecReport, ExecError> {
    let mut executor = Executor::new(
        Arc::new(NativeRuntime::new()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_finalizer(seen);
    if let Some(store) = store {
        executor = executor.with_aliases(store.clone(), "root-1");
    }
    executor
        .execute(
            ExecRequest {
                job_id: "alias-job".into(),
                root: root.path().into(),
                relative_dir: "Indie Game".into(),
                plans: PlanSet {
                    candidates: vec![candidate],
                    selected: Some(0),
                    question: None,
                },
                sources: BTreeMap::new(),
            },
            CancellationToken::new(),
        )
        .await
}

fn store() -> (tempfile::TempDir, AliasStore) {
    let home = tempfile::tempdir().unwrap();
    let store = AliasStore::new(home.path().canonicalize().unwrap().join("Caskade/g"));
    (home, store)
}

#[tokio::test]
async fn native_installer_receives_and_records_only_the_alias() {
    let root = TestRoot::new().unwrap();
    let (_home, store) = store();
    let seen = Arc::new(Seen::default());
    let report = run(&root, Some(&store), native(0), seen.clone())
        .await
        .unwrap();
    let id = report.alias.clone().expect("alias recorded");
    assert_eq!(id, AliasStore::short_id("root-1/Indie Game"));
    assert_eq!(*seen.0.lock().unwrap(), [Some(id.clone())]);
    let real = root.path().canonicalize().unwrap().join("Indie Game");
    assert_eq!(store.target(&id).unwrap().as_deref(), Some(real.as_path()));
    let config = fs::read_to_string(real.join("config.ini")).unwrap();
    let alias = store.path(&id).unwrap();
    assert_eq!(config, format!("InstallPath={}\n", alias.display()));
    assert!(!config.contains(&real.display().to_string()));
    root.assert_sentinel_untouched();
}

#[tokio::test]
async fn extracted_games_are_linked_after_commit() {
    let root = TestRoot::new().unwrap();
    let (_home, store) = store();
    let report = run(&root, Some(&store), extract(), Arc::default())
        .await
        .unwrap();
    let id = report.alias.unwrap();
    let alias: PathBuf = store.path(&id).unwrap();
    assert_eq!(fs::read(alias.join("game.bin")).unwrap(), b"game");
}

#[tokio::test]
async fn failed_install_removes_its_alias_and_disabled_aliases_use_the_real_path() {
    let root = TestRoot::new().unwrap();
    let (_home, store) = store();
    assert!(
        run(&root, Some(&store), native(5), Arc::default())
            .await
            .is_err()
    );
    let id = AliasStore::short_id("root-1/Indie Game");
    assert_eq!(store.target(&id).unwrap(), None);
    assert!(!root.path().join("Indie Game").exists());

    // A recipe that disables the alias simply omits with_aliases.
    let report = run(&root, None, native(0), Arc::default()).await.unwrap();
    assert_eq!(report.alias, None);
    let real = root.path().join("Indie Game");
    let config = fs::read_to_string(real.join("config.ini")).unwrap();
    assert!(config.contains("Indie Game"));
    assert_eq!(fs::read_dir(store.base()).unwrap().count(), 0);
    root.assert_sentinel_untouched();
}
