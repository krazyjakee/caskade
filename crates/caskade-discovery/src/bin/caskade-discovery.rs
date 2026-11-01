//! Standalone development CLI; stdout is reserved for final results, progress
//! and logs (`RUST_LOG`) go to stderr.

use caskade_discovery::{
    Cancel, Config, DeepScans, Error, HostEnv, SourceId, SourceSelection, data::Data,
    detect_launchers, scan,
};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::{path::PathBuf, process::ExitCode};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    version,
    about = "Find games installed by launchers, stores and the OS"
)]
struct Cli {
    /// Emit one machine-readable JSON result on stdout.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the launchers found on this machine (or fixture).
    Launchers {
        /// Read a fixture root instead of this machine.
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Scan for installed games.
    Scan {
        /// Comma-separated source ids (default: all).
        #[arg(long, value_delimiter = ',')]
        source: Vec<String>,
        /// Opt-in deep scans, e.g. `folders=/games`.
        #[arg(long)]
        deep: Vec<String>,
        /// Read a fixture root instead of this machine.
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Score a directory or executable as a game, with evidence.
    Classify {
        path: PathBuf,
        /// Read a fixture root instead of this machine.
        #[arg(long)]
        root: Option<PathBuf>,
        /// Minimum score for the review list.
        #[arg(long, default_value_t = 20)]
        maybe_score: i32,
        /// Minimum score for a likely game.
        #[arg(long, default_value_t = 60)]
        likely_score: i32,
    },
    /// Turn this machine's launcher files into a fixture.
    Dump {
        /// Replace user names, paths and ids with placeholders (required).
        #[arg(long)]
        anonymize: bool,
        /// Read a fixture root instead of this machine.
        #[arg(long)]
        root: Option<PathBuf>,
        out: PathBuf,
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let result = run(&cli.command);
    match result {
        Ok(value) => {
            if cli.json {
                println!("{value}");
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("json value")
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if cli.json {
                println!("{}", json!({ "error": error.to_string() }));
            } else {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn progress(message: &str) {
    eprintln!("{message}");
}

fn host(root: Option<&std::path::Path>) -> Result<HostEnv, Error> {
    match root {
        Some(root) => Ok(HostEnv::fixture(root)?),
        None => Ok(HostEnv::live()),
    }
}

/// `folders=<path>` (repeatable) or a flag: `external_drives`, `portable`,
/// `wine_prefixes`, `full_disk`.
fn deep_scans(args: &[String]) -> Result<DeepScans, Error> {
    let mut deep = DeepScans::default();
    for arg in args {
        match arg.split_once('=') {
            Some(("folders", path)) => deep.folders.push(PathBuf::from(path)),
            None if arg == "external_drives" => deep.external_drives = true,
            None if arg == "portable" => deep.portable = true,
            None if arg == "wine_prefixes" => deep.wine_prefixes = true,
            None if arg == "full_disk" => deep.full_disk = true,
            _ => return Err(Error::Usage(format!("unknown deep scan {arg:?}"))),
        }
    }
    Ok(deep)
}

fn run(command: &Command) -> Result<serde_json::Value, Error> {
    Data::builtin()?;
    match command {
        Command::Launchers { root } => {
            let env = host(root.as_deref())?;
            progress("detecting launchers");
            Ok(serde_json::to_value(detect_launchers(&env)).expect("serializable"))
        }
        Command::Scan { source, deep, root } => {
            let env = host(root.as_deref())?;
            let mut config = Config {
                deep: deep_scans(deep)?,
                ..Config::default()
            };
            if !source.is_empty() {
                config.sources = SourceSelection::only(source.iter().cloned().map(SourceId::from));
            }
            progress("scanning");
            let report = scan(&env, &config, &Cancel::new())?;
            for source in &report.sources {
                match &source.error {
                    Some(error) => progress(&format!("{}: {error}", source.source)),
                    None => progress(&format!("{}: {} games", source.source, source.count)),
                }
            }
            Ok(serde_json::to_value(report).expect("serializable"))
        }
        Command::Classify {
            path,
            root,
            maybe_score,
            likely_score,
        } => {
            let env = host(root.as_deref())?;
            let capabilities = if env.is_fixture() {
                use caskade_installer::host::{FileSystemCaps, HostCaps, RunnerInfo, RunnerKind};
                HostCaps {
                    os: env.os(),
                    arch: "x86_64".into(),
                    runners: vec![RunnerInfo {
                        kind: RunnerKind::Native,
                        program: PathBuf::new(),
                    }],
                    seven_zip: None,
                    proton_builds: Vec::new(),
                    filesystem: FileSystemCaps {
                        writable: false,
                        exec_bits: true,
                        symlinks: true,
                        case_sensitive: env.os() != caskade_discovery::OperatingSystem::Windows,
                    },
                    packages: Default::default(),
                }
            } else {
                caskade_installer::host::HostCaps::detect()
            };
            let metadata = caskade_discovery::classify::Metadata {
                known_game_folders: vec![env.home().join("Games")],
                ..Default::default()
            };
            let config = caskade_discovery::ClassifierConfig {
                maybe_score: *maybe_score,
                likely_score: *likely_score,
                ..Default::default()
            };
            progress("classifying");
            let result = caskade_discovery::classify::classify_path(
                &env,
                &capabilities,
                path,
                &metadata,
                &config,
            )?;
            serde_json::to_value(result).map_err(|error| Error::Usage(error.to_string()))
        }
        Command::Dump {
            anonymize,
            root,
            out,
        } => {
            if !anonymize {
                return Err(Error::Usage(
                    "dump only writes anonymized fixtures; pass --anonymize".into(),
                ));
            }
            let env = host(root.as_deref())?;
            progress("dumping launcher files");
            let report = caskade_discovery::dump::dump_anonymized(&env, out)?;
            Ok(serde_json::to_value(report).expect("serializable"))
        }
    }
}
