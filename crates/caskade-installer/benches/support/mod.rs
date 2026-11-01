//! Shared, generated T10 workloads. Setup and cleanup are outside timed spans.
#![allow(dead_code)]
use caskade_installer::{
    Format,
    discover::{DiscoveryConfig, DiscoveryHints, discover_with_config},
    formats::archive::{self, ArchiveOptions, NoopControl},
    host::HostCaps,
    probe::{Probe, ProbeEngine, ProbeLimits},
    safety::{Limits, SafeDir, open_staging},
};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const GIB: u64 = 1024 * 1024 * 1024;
pub const PROBE_BYTES: u64 = 20 * GIB;
pub const RSS_BYTES: u64 = 10 * GIB;
pub const THROUGHPUT_BYTES: u64 = 128 * 1024 * 1024;
pub fn temporary(prefix: &str) -> Result<tempfile::TempDir> {
    // Tests explicitly use /tmp, never a committed fixture or user library.
    Ok(tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in("/tmp")?)
}
pub fn sparse_installer(root: &Path) -> Result<PathBuf> {
    let path = root.join("twenty-gib-setup.exe");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/bin/windows/inno60-basic/setup.exe"),
        &path,
    )?;
    fs::OpenOptions::new()
        .write(true)
        .open(&path)?
        .set_len(PROBE_BYTES)?;
    Ok(path)
}
pub fn probe(path: &Path) -> Result<(Probe, Duration)> {
    let engine = ProbeEngine::with_builtin_formats(ProbeLimits::default());
    let start = Instant::now();
    let result = engine.probe_path(path)?;
    Ok((result, start.elapsed()))
}
pub fn discovery_tree(root: &Path) -> Result<()> {
    // Exactly 100,000 regular files plus 100 directories. A real executable
    // competes with 99,999 data assets; data are nonempty, never mock entries.
    for group in 0..100 {
        let directory = root.join(format!("assets-{group:03}"));
        fs::create_dir(&directory)?;
        for file in 0..1000 {
            if group == 0 && file == 0 {
                continue;
            }
            fs::write(
                directory.join(format!("texture-{file:04}.dat")),
                b"fixture asset data\n",
            )?;
        }
    }
    let game = root.join("FixtureGame.x86_64");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/fixture-game-linux-x86_64"),
        game,
    )?;
    Ok(())
}
pub fn discovery(root: &Path) -> Result<Duration> {
    let host: HostCaps = toml::from_str(include_str!("../../tests/hosts/linux.toml"))?;
    let config = DiscoveryConfig {
        max_entries: 100_100,
        write_icon: false,
        ..Default::default()
    };
    let start = Instant::now();
    let found = discover_with_config(root, &host, &DiscoveryHints::default(), config)?;
    let elapsed = start.elapsed();
    if found.question.is_some()
        || found
            .launch_targets
            .first()
            .is_none_or(|target| target.path != Path::new("FixtureGame.x86_64"))
    {
        return Err("discovery failed to select the real game in the 100k-file tree".into());
    }
    Ok(elapsed)
}
pub fn sparse_tar(path: &Path, size: u64) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_path("payload.bin")?;
    header.set_size(size);
    header.set_mode(0o644);
    header.set_cksum();
    let mut file = fs::File::create(path)?;
    file.write_all(header.as_bytes())?;
    file.seek(SeekFrom::Start(512 + size))?;
    file.write_all(&[0; 1024])?;
    Ok(())
}
pub fn extract(path: &Path, destination: &Path, size: u64) -> Result<Duration> {
    fs::create_dir(destination)?;
    let mut safe = SafeDir::new(
        open_staging(destination)?,
        Limits::default(),
        path.metadata()?.len(),
        true,
    );
    let start = Instant::now();
    archive::extract(
        fs::File::open(path)?,
        Format::Tar,
        &mut safe,
        &ArchiveOptions::default(),
        &mut NoopControl,
    )?;
    let elapsed = start.elapsed();
    if safe.actual_bytes() != size {
        return Err("extracted byte count differs from generated workload".into());
    }
    let mut file = fs::File::open(destination.join("payload.bin"))?;
    if file.metadata()?.len() != size {
        return Err("extracted file size differs".into());
    }
    for offset in [0, size / 2, size - 4] {
        file.seek(SeekFrom::Start(offset))?;
        let mut sample = [1; 4];
        file.read_exact(&mut sample)?;
        if sample != [0; 4] {
            return Err("extracted sparse TAR payload differs".into());
        }
    }
    Ok(elapsed)
}
pub fn median(mut values: Vec<Duration>) -> Duration {
    values.sort();
    values[values.len() / 2]
}
pub fn evidence(name: &str, value: serde_json::Value) -> Result<()> {
    eprintln!("T10 {name}: {value}");
    if let Some(directory) = std::env::var_os("CASKADE_PERF_OUTPUT") {
        fs::create_dir_all(&directory)?;
        fs::write(
            Path::new(&directory).join(format!("{name}.json")),
            serde_json::to_vec_pretty(&value)?,
        )?;
    }
    Ok(())
}
