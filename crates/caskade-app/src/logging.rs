//! Synchronous daily logs: release Windows builds have no stderr console.
//! Keep seven days and flush each event, including in child player processes.
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Logs to stderr and the daily files, then the start banner naming the
/// shell, `app`, and its `version`.
pub fn init(app: &str, version: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let file = caskade_core::AppPaths::standard().ok().and_then(|paths| {
        tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("caskade")
            .filename_suffix("log")
            .max_log_files(7)
            .build(paths.logs)
            .map_err(|error| eprintln!("couldn't open Caskade's log: {error}"))
            .ok()
    });
    let file = file.map(|writer| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(writer)
    });
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(file)
        .try_init();
    tracing::info!(pid = std::process::id(), version, "{app} starting");
}
