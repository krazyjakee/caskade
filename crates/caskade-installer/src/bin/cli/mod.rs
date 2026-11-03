mod r#move;
mod output;
mod reconcile;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use caskade_installer::{
    exec::CancellationToken, host::HostCaps, manifest::InstallManifest, plan::Policy,
};
use serde_json::json;

use super::Command;
pub use caskade_installer::pipeline::NeedsDecision;
use caskade_installer::pipeline::{self, Context, actual_host, input};
pub use output::Output;

pub type CliResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub async fn run(
    command: Command,
    json: bool,
    cancel: CancellationToken,
    seven_zip: Option<PathBuf>,
) -> CliResult<Output> {
    let tool = match seven_zip {
        Some(path) => Some(caskade_installer::formats::archive::sevenzip::SevenZip::pinned(path)?),
        None => {
            caskade_installer::formats::archive::sevenzip::SevenZip::from_host(&HostCaps::detect())
                .ok()
        }
    };
    let ctx = Context {
        sink: Arc::new(output::Progress::new(!json)),
        cancel,
        tool,
        aliases: None,
        control: Default::default(),
        folder_template: None,
        config: Default::default(),
        host: None,
        handlers: Vec::new(),
        #[cfg(feature = "fetch")]
        resolvers: Vec::new(),
        #[cfg(feature = "fetch")]
        local: None,
        #[cfg(feature = "torrent")]
        torrent: Default::default(),
        review: None,
        install_id: None,
        hints: Default::default(),
        umu: None,
    };
    match command {
        Command::Fetch { urls, root, job_id } => fetch(urls, root, job_id, &ctx).await,
        Command::Probe {
            input,
            root,
            job_id,
        } => {
            let job = input::job_id(&input, job_id)?;
            let prepared = input::prepare(input, &root, &job, &input::host(None)?, &ctx).await?;
            Output::new(
                json!({ "input": prepared.source, "resolved": prepared.resolved, "artifacts": [prepared.artifact], "io": prepared.io }),
                format!(
                    "{}: {:?} ({:?}, {:?})",
                    prepared.artifact.path.display(),
                    prepared.artifact.format,
                    prepared.artifact.platform,
                    prepared.artifact.role
                ),
            )
        }
        Command::Plan {
            input,
            host,
            root,
            job_id,
        } => {
            let job = input::job_id(&input, job_id)?;
            let host = match host {
                Some(profile) => input::host(Some(&profile))?,
                None => actual_host(&ctx)?,
            };
            let prepared = input::prepare(input, &root, &job, &host, &ctx).await?;
            let plans = input::planner()?.plan(
                std::slice::from_ref(&prepared.artifact),
                &host,
                &Policy::default(),
            )?;
            let mut human = format!(
                "Plan for {:?} / {}: {} candidate(s)",
                host.os,
                host.arch,
                plans.candidates.len()
            );
            for (index, candidate) in plans.candidates.iter().enumerate() {
                human.push_str(&format!(
                    "\n{}{}. {} {:?} ({:.0}% confidence)",
                    if plans.selected == Some(index as u32) {
                        "* "
                    } else {
                        "  "
                    },
                    index + 1,
                    candidate.handler.0,
                    candidate.strategy,
                    candidate.confidence * 100.0
                ));
            }
            if let Some(question) = &plans.question {
                human.push_str(&format!("\nNeeds a decision: {question:?}"));
            }
            Output::new(
                json!({"host": host, "artifacts": [prepared.artifact], "plans": plans}),
                human,
            )
        }
        Command::Install {
            input,
            root,
            keep_downloads,
            extract,
            job_id,
        } => Output::outcome(
            pipeline::install(input, root, keep_downloads, extract, job_id, &ctx).await?,
        ),
        Command::Uninstall {
            game_dir,
            external_roots,
        } => uninstall(game_dir, external_roots, &ctx).await,
        Command::Reconcile {
            root,
            game,
            from,
            alias_dir,
            external_roots,
        } => {
            reconcile::run(
                reconcile::Options {
                    root,
                    game,
                    from,
                    alias_dir,
                    external_roots,
                },
                &ctx,
            )
            .await
        }
        Command::Move {
            game_dir,
            destination,
            alias_dir,
            external_roots,
        } => r#move::run(game_dir, destination, alias_dir, external_roots, &ctx).await,
    }
}

async fn fetch(
    urls: Vec<String>,
    root: PathBuf,
    job: Option<String>,
    ctx: &Context,
) -> CliResult<Output> {
    #[cfg(feature = "fetch")]
    {
        fs::create_dir_all(&root)?;
        let job = input::job_id(&urls.join("\n"), job)?;
        let engine = input::fetch_engine(true, ctx)?;
        let group_cancel = ctx.cancel.child_token();
        let host = input::host(None)?;
        let mut downloads = vec![];
        for value in urls {
            let url = input::remote(&value).ok_or("fetch requires HTTP or HTTPS URLs")?;
            let resolution = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(caskade_installer::fetch::FetchError::Cancelled.into()),
                result = engine.resolve(&url, &host) => result?,
            };
            let requests = match resolution {
                caskade_installer::fetch::Resolution::Downloads(requests) => requests,
                caskade_installer::fetch::Resolution::NeedsUser(question) => {
                    return Err(NeedsDecision(question).into());
                }
            };
            for request in requests {
                let engine = engine.clone();
                let root = root.clone();
                let job = job.clone();
                let cancel = group_cancel.clone();
                downloads.push(tokio::spawn(async move {
                    engine.download(&root, &job, &request, cancel).await
                }));
            }
        }
        let mut files = vec![];
        let mut error: Option<Box<dyn std::error::Error + Send + Sync>> = None;
        for task in downloads {
            match task.await {
                Ok(Ok(file)) => files.push(file),
                Ok(Err(failure)) => {
                    group_cancel.cancel();
                    if error.is_none() {
                        error = Some(failure.into());
                    }
                }
                Err(failure) => {
                    group_cancel.cancel();
                    if error.is_none() {
                        error = Some(failure.into());
                    }
                }
            }
        }
        if let Some(error) = error {
            return Err(error);
        }
        let human = files
            .iter()
            .map(|file| {
                format!(
                    "Downloaded {} ({} bytes, sha256 {})",
                    file.path.display(),
                    file.size,
                    file.sha256
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Output::new(
            json!({"type": "downloaded", "value": {"job_id": job, "files": files}}),
            human,
        )
    }
    #[cfg(not(feature = "fetch"))]
    {
        let _ = (urls, root, job, ctx);
        Err("fetch requires the fetch feature".into())
    }
}

async fn uninstall(
    game_dir: PathBuf,
    external_roots: Vec<String>,
    ctx: &Context,
) -> CliResult<Output> {
    let report = pipeline::uninstall(game_dir.clone(), external_roots, ctx).await?;
    Output::new(
        json!({"type": "uninstalled", "value": report}),
        format!("Uninstalled {}", game_dir.display()),
    )
}
