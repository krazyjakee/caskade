//! A separate process exits mid-move: destructors cannot compensate it.
use caskade_installer::{
    events::{EventSink, JobEvent},
    relocate::{
        RelocateRequest, Relocator, RunningCheck,
        backends::{NoRegistry, NoShortcuts},
    },
    runtime::alias::AliasStore,
    scan::{self, ScanInput, ScanLimits},
    testing::FakeDiskSpace,
};
use std::{fs, path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;
struct Idle;
impl RunningCheck for Idle {
    fn running(&self, _: &Path) -> bool {
        false
    }
}
struct KillAt(Option<String>);
impl EventSink for KillAt {
    fn emit(&self, event: JobEvent) {
        if self.0.as_deref() == Some(&event.message) {
            std::process::exit(77);
        }
    }
}
fn engine(kill: Option<String>) -> Relocator {
    Relocator {
        disk: Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        sink: Arc::new(KillAt(kill)),
        registry: Arc::new(NoRegistry),
        shortcuts: Arc::new(NoShortcuts),
        running: Arc::new(Idle),
    }
}
fn request(base: &Path, copy: bool) -> RelocateRequest {
    let source = base.join("source").join("Game");
    let refs = scan::scan_references(&ScanInput {
        game_dir: &source,
        real_path: &source.to_string_lossy(),
        alias_path: None,
        real_variants: &[],
        registry: &[],
        external_files: &[],
        delegated: false,
        limits: ScanLimits::default(),
    })
    .map(|r| r.references)
    .unwrap_or_else(|_| {
        vec![scan::Reference {
            location: scan::Location::GameFile {
                path: "config.ini".into(),
                rewritable: true,
            },
            via: scan::Via::Real,
            encoding: scan::Encoding::Utf8,
        }]
    });
    RelocateRequest {
        job_id: "killed-move".into(),
        game_dir: source,
        target_root: base.join("target"),
        references: refs,
        spellings: vec![],
        alias: Some((AliasStore::new(base.join("aliases")), "fixture".into())),
        force_copy: copy,
    }
}
#[test]
fn killed_move_worker() {
    let Some(base) = std::env::var_os("CASKADE_MOVE_RECOVERY_BASE") else {
        return;
    };
    let phase = std::env::var("CASKADE_MOVE_RECOVERY_PHASE").unwrap();
    engine(Some(phase))
        .relocate(
            &request(
                Path::new(&base),
                std::env::var("CASKADE_MOVE_RECOVERY_COPY").unwrap() == "1",
            ),
            &CancellationToken::new(),
        )
        .unwrap();
    panic!("worker did not exit at the requested phase");
}
#[test]
fn restart_rolls_back_killed_copy_rename_and_reference_rewrite() {
    for (copy, phase) in [
        (true, "copying"),
        (true, "copied"),
        (true, "rewriting"),
        (false, "moved"),
        (false, "rewriting"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(temp.path()).unwrap();
        let game = base.join("source").join("Game");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir(base.join("target")).unwrap();
        let original = format!("InstallPath={}\n", game.display());
        fs::write(game.join("config.ini"), &original).unwrap();
        let aliases = AliasStore::new(base.join("aliases"));
        aliases.link("fixture", &game).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "relocate_recovery::killed_move_worker",
                "--nocapture",
            ])
            .env("CASKADE_MOVE_RECOVERY_BASE", &base)
            .env("CASKADE_MOVE_RECOVERY_PHASE", phase)
            .env("CASKADE_MOVE_RECOVERY_COPY", if copy { "1" } else { "0" })
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(77));
        let req = request(&base, copy);
        assert!(engine(None).recover_move(&req).unwrap());
        assert_eq!(
            fs::read_to_string(game.join("config.ini")).unwrap(),
            original
        );
        assert!(!base.join("target").join("Game").exists());
        assert_eq!(
            aliases.target("fixture").unwrap().as_deref(),
            Some(game.as_path())
        );
        assert!(
            !engine(None).recover_move(&req).unwrap(),
            "recovery is idempotent"
        );
    }
}

#[test]
fn restart_finishes_a_durably_committed_copy_instead_of_reverting_it() {
    let temp = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(temp.path()).unwrap();
    let game = base.join("source").join("Game");
    fs::create_dir_all(&game).unwrap();
    fs::create_dir(base.join("target")).unwrap();
    fs::write(
        game.join("config.ini"),
        format!("InstallPath={}\n", game.display()),
    )
    .unwrap();
    let aliases = AliasStore::new(base.join("aliases"));
    aliases.link("fixture", &game).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "relocate_recovery::killed_move_worker",
            "--nocapture",
        ])
        .env("CASKADE_MOVE_RECOVERY_BASE", &base)
        .env("CASKADE_MOVE_RECOVERY_PHASE", "committed")
        .env("CASKADE_MOVE_RECOVERY_COPY", "1")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(77));
    assert!(game.is_dir());
    let destination = base.join("target").join("Game");
    assert!(destination.is_dir());
    assert!(engine(None).recover_move(&request(&base, true)).unwrap());
    assert!(!game.exists());
    assert!(destination.is_dir());
    assert_eq!(
        fs::read_to_string(destination.join("config.ini")).unwrap(),
        format!("InstallPath={}\n", destination.display())
    );
    assert_eq!(aliases.target("fixture").unwrap(), Some(destination));
}
