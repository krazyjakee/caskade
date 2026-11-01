//! Standalone development CLI; stdout is reserved for final results.
mod cli;

use caskade_installer::exec::CancellationToken;
use clap::{Parser, Subcommand};
use serde_json::json;
use std::{path::PathBuf, process::ExitCode};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    version,
    about = "Analyze, download, install and uninstall managed games"
)]
struct Cli {
    /// Emit one machine-readable JSON result on stdout.
    #[arg(long, global = true)]
    json: bool,
    /// Explicit path to the SHA-verified official 7-Zip sidecar.
    #[arg(long, global = true)]
    seven_zip: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Resolve and download HTTP(S) URLs; retain files for later installation.
    Fetch {
        #[arg(required = true, num_args = 1..)]
        urls: Vec<String>,
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Stable identifier for resuming a download job after restart.
        #[arg(long)]
        job_id: Option<String>,
    },
    /// Statically identify a local input or downloaded URL without running it.
    Probe {
        input: String,
        /// Download cache root for URL inputs.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        job_id: Option<String>,
    },
    /// Show ranked alternatives for a local input or downloaded URL.
    Plan {
        input: String,
        /// Built-in profile name or path to a HostCaps TOML file.
        #[arg(long)]
        host: Option<String>,
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        job_id: Option<String>,
    },
    /// Install an archive or portable folder, or hand a ROM to emulation.
    Install {
        input: String,
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        keep_downloads: bool,
        /// Prefer static extraction and disable installer code execution.
        #[arg(long)]
        extract: bool,
        #[arg(long)]
        job_id: Option<String>,
    },
    /// Run recorded uninstall steps and remove a managed game directory.
    Uninstall {
        game_dir: PathBuf,
        /// Bind a recorded external scope to a directory on this host.
        #[arg(long = "external-root", value_name = "SCOPE=PATH")]
        external_roots: Vec<String>,
    },
    /// Scan a storage root and repair games moved outside Caskade; moves no folders.
    Reconcile {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Repair only this game-relative directory instead of scanning the root.
        #[arg(long)]
        game: Option<PathBuf>,
        /// Previous absolute location for a legacy manifest without an anchor.
        #[arg(long, requires = "game")]
        from: Option<String>,
        /// Explicit alias store; defaults to Caskade's per-user alias directory.
        #[arg(long)]
        alias_dir: Option<PathBuf>,
        /// Explicit directory containing recorded external files to repair.
        #[arg(long = "external-root")]
        external_roots: Vec<PathBuf>,
    },
    /// Move a managed game into another storage root with transactional rewrites.
    Move {
        game_dir: PathBuf,
        destination: Option<PathBuf>,
        /// Explicit alias store; defaults to Caskade's per-user alias directory.
        #[arg(long)]
        alias_dir: Option<PathBuf>,
        /// Explicit directory containing recorded external files to repair.
        #[arg(long = "external-root")]
        external_roots: Vec<PathBuf>,
    },
}

fn main() -> ExitCode {
    let args = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()))
        .with_writer(std::io::stderr)
        .init();
    let cancel = CancellationToken::new();
    let result = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { Box::new(error) })
        .and_then(|runtime| {
            runtime.block_on(async {
                let token = cancel.clone();
                let json_output = args.json;
                let signals = tokio::spawn(async move {
                    if tokio::signal::ctrl_c().await.is_ok() {
                        if !json_output {
                            eprintln!("Cancelling; waiting for cleanup...");
                        }
                        token.cancel();
                    }
                });
                let result =
                    cli::run(args.command, args.json, cancel.clone(), args.seven_zip).await;
                signals.abort();
                result
            })
        });
    match result {
        Ok(output) => render(output, args.json),
        Err(error) => {
            let question = error
                .downcast_ref::<cli::NeedsDecision>()
                .map(|decision| &decision.0)
                .or_else(
                    || match error.downcast_ref::<caskade_installer::exec::ExecError>() {
                        Some(caskade_installer::exec::ExecError::NeedsUser { question }) => {
                            Some(question)
                        }
                        _ => None,
                    },
                );
            if let Some(question) = question
                && let Ok(output) = cli::Output::outcome(
                    caskade_installer::InstallOutcome::NeedsUser(question.clone()),
                )
            {
                return render(output, args.json);
            }
            if args.json {
                let report = error
                    .downcast_ref::<caskade_installer::manifest::UninstallError>()
                    .and_then(|error| error.report());
                println!(
                    "{}",
                    json!({"type": if cancel.is_cancelled() { "cancelled" } else { "error" }, "value": {"message": error.to_string(), "report": report}})
                );
            } else {
                eprintln!("error: {error}");
            }
            ExitCode::from(if cancel.is_cancelled() { 130 } else { 1 })
        }
    }
}

fn render(output: cli::Output, json_output: bool) -> ExitCode {
    if json_output {
        println!("{}", output.value);
    } else {
        println!("{}", output.human);
    }
    ExitCode::from(output.code)
}
