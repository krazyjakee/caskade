use super::*;
pub(super) use caskade_installer::pipeline::relocate::{engine, engine_for_move};
use caskade_installer::{
    reconcile::{ReconcileRequest, ReconcileStatus, Reconciler},
    runtime::alias::AliasStore,
};
pub(super) struct Options {
    pub root: PathBuf,
    pub game: Option<PathBuf>,
    pub from: Option<String>,
    pub alias_dir: Option<PathBuf>,
    pub external_roots: Vec<PathBuf>,
}

pub(super) async fn run(options: Options, ctx: &Context) -> CliResult<Output> {
    let host = actual_host(ctx)?;
    let base = engine(None, &host, ctx.sink.clone())?;
    let sink = ctx.sink.clone();
    let factory = move |game: &Path, manifest: &InstallManifest| {
        engine(Some((game, manifest)), &host, sink.clone())
    };
    let mut scanner = Reconciler::new(base).with_repair_factory(Arc::new(factory));
    let aliases = options
        .alias_dir
        .map(AliasStore::new)
        .or_else(AliasStore::for_host);
    if let Some(aliases) = aliases {
        scanner = scanner.with_aliases(aliases);
    }
    scanner = scanner.with_external_roots(options.external_roots);
    if let Some(game) = options.game {
        let job = input::job_id(
            &format!("reconcile:{}:{}", options.root.display(), game.display()),
            None,
        )?;
        let report = scanner
            .reconcile(
                ReconcileRequest {
                    root: options.root,
                    relative_dir: game,
                    old_dir: options.from,
                    job_id: job,
                },
                ctx.cancel.clone(),
            )
            .await?;
        let code = match report.status {
            ReconcileStatus::NeedsUser => 2,
            ReconcileStatus::Failed => 1,
            _ => 0,
        };
        let human = format!(
            "{}: {:?} ({} file, {} registry rewrites)",
            report.relative_dir.display(),
            report.status,
            report.rewritten_files,
            report.rewritten_values
        );
        let mut output = Output::new(json!({"type":"reconciled","value":report}), human)?;
        output.code = code;
        Ok(output)
    } else {
        let report = scanner.scan_root(&options.root, ctx.cancel.clone()).await?;
        let code = if report
            .games
            .iter()
            .any(|entry| entry.status == ReconcileStatus::Failed)
        {
            1
        } else if report
            .games
            .iter()
            .any(|entry| entry.status == ReconcileStatus::NeedsUser)
        {
            2
        } else {
            0
        };
        let mut human = format!(
            "Scanned root {}: {} managed game(s)",
            report.root_id,
            report.games.len()
        );
        for game in &report.games {
            human.push_str(&format!(
                "\n{}: {:?}",
                game.relative_dir.display(),
                game.status
            ));
            if let Some(error) = &game.error {
                human.push_str(&format!(" — {error}"));
            }
            if let Some(question) = &game.question {
                human.push_str(&format!("\nNeeds a decision: {question:?}"));
            }
        }
        let mut output = Output::new(json!({"type":"reconciled","value":report}), human)?;
        output.code = code;
        Ok(output)
    }
}
