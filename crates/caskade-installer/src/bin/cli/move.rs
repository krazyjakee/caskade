use super::*;
use caskade_installer::{
    exec::{ExecError, with_roots_exclusive},
    reconcile::{ReconcileError, Reconciler},
    relocate::{RelocateError, RelocateRequest},
    runtime::alias::AliasStore,
};

/// Move a managed game to another storage root (#60): rename or copy and
/// verify, retarget its alias and prefix links, rewrite recorded references,
/// all rolled back on failure or Ctrl-C.
pub(super) async fn run(
    game_dir: PathBuf,
    destination: Option<PathBuf>,
    alias_dir: Option<PathBuf>,
    external_roots: Vec<PathBuf>,
    ctx: &Context,
) -> CliResult<Output> {
    let destination = destination.ok_or("move needs a destination storage root")?;
    // Games record their folder as they were given it, which can go through a
    // link (macOS's /var is /private/var): that spelling moves too.
    let given = std::path::absolute(&game_dir)?;
    let given_target = std::path::absolute(&destination)?;
    let game = dunce::canonicalize(&game_dir)?;
    let root = game
        .parent()
        .ok_or("game directory has no storage root")?
        .to_path_buf();
    fs::create_dir_all(&destination)?;
    let destination = dunce::canonicalize(&destination)?;
    let host = actual_host(ctx)?;
    let sink = ctx.sink.clone();
    let source = game.clone();
    let target_root = destination.clone();
    let result = with_roots_exclusive(
        &[root.as_path(), destination.as_path()],
        ctx.cancel.clone(),
        move |_, token| {
            let manifest = InstallManifest::read(&source)?;
            let record = manifest.record();
            if record.relocation.verdict == caskade_installer::manifest::RelocationVerdict::Pinned
                || record.plan.candidate.strategy == caskade_installer::Strategy::Delegate
            {
                return Err(ReconcileError::Invalid(
                    "delegated or Pinned installations cannot be moved".into(),
                ));
            }
            let old = source.to_string_lossy().into_owned();
            if record.relocation.last_path.as_deref().is_some_and(|path| {
                path != old && dunce::canonicalize(path).ok().as_deref() != Some(source.as_path())
            }) {
                return Err(ReconcileError::Invalid(
                    "game already moved outside Caskade; reconcile it before moving again".into(),
                ));
            }
            let aliases = alias_dir.map(AliasStore::new).or_else(AliasStore::for_host);
            if record.alias.is_some() && aliases.is_none() {
                return Err(ReconcileError::MissingAliases(source));
            }
            if let (Some(id), Some(store)) = (&record.alias, &aliases)
                && let Some(target) = store.target(id).map_err(std::io::Error::other)?
                && target.try_exists()?
                && dunce::canonicalize(&target)? != source
            {
                return Err(ReconcileError::Invalid(
                    "alias names another existing installation; reconcile the copies first".into(),
                ));
            }
            let source_engine = reconcile::engine(Some((&source, &manifest)), &host, sink.clone())?;
            let mut inspector = Reconciler::new(source_engine).with_external_roots(external_roots);
            if let Some(store) = &aliases {
                inspector = inspector.with_aliases(store.clone());
            }
            let prepared = inspector.prepare_references(&source, &old, &manifest)?;
            if token.is_cancelled() {
                return Err(ReconcileError::Cancelled);
            }
            let final_dir =
                target_root.join(source.file_name().ok_or_else(|| {
                    ReconcileError::Invalid("game directory has no filename".into())
                })?);
            let relocator =
                reconcile::engine_for_move(&source, &final_dir, &manifest, &host, sink)?;
            let spellings = match given.file_name() {
                Some(name) if given != source => caskade_installer::relocate::spelling_pairs(
                    &given.to_string_lossy(),
                    &given_target.join(name).to_string_lossy(),
                ),
                _ => vec![],
            };
            let request = RelocateRequest {
                job_id: input::job_id(
                    &format!("move:{}:{}", source.display(), target_root.display()),
                    None,
                )
                .map_err(|error| ReconcileError::Invalid(error.to_string()))?,
                game_dir: source,
                target_root,
                references: prepared.references,
                spellings,
                alias: record
                    .alias
                    .clone()
                    .zip(aliases)
                    .map(|(id, store)| (store, id)),
                force_copy: false,
            };
            let report =
                relocator.relocate_with_verdict(&request, &token, Some(prepared.verdict))?;
            Ok::<_, ReconcileError>((report, prepared.skipped))
        },
    )
    .await;
    let (report, skipped) = match result {
        Ok(report) => report,
        Err(ReconcileError::Cancelled | ReconcileError::Repair(RelocateError::Cancelled)) => {
            return Err(ExecError::Cancelled.into());
        }
        Err(error) => return Err(error.into()),
    };
    let human = format!(
        "Moved {} to {} ({}; {} file, {} registry rewrites)",
        game.display(),
        report.game_dir.display(),
        if report.copied {
            "copied and verified"
        } else {
            "renamed"
        },
        report.rewritten_files,
        report.rewritten_values
    );
    Output::new(
        json!({
            "type": "moved",
            "value": {
                "from": game,
                "dir": report.game_dir,
                "copied": report.copied,
                "rewritten_files": report.rewritten_files,
                "rewritten_values": report.rewritten_values,
                "warnings": skipped,
            }
        }),
        human,
    )
}
