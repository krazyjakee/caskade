//! Explicit nightly performance gates. Generated files are private and dropped
//! on success, assertion failure or error. Run the timed gates in --release.
#![cfg(all(
    target_os = "linux",
    feature = "windows-formats",
    feature = "linux-formats"
))]
#[path = "../benches/support/mod.rs"]
mod support;
use serde_json::json;
use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};
use support::*;
#[test]
#[ignore = "T10: real 20 GiB sparse installer probe; nightly release gate"]
fn twenty_gib_installer_probe_stays_below_io_and_latency_budgets() -> Result<()> {
    let temp = temporary("caskade-perf-probe-")?;
    let path = sparse_installer(temp.path())?;
    let (found, elapsed) = probe(&path)?;
    evidence(
        "probe",
        json!({"input_bytes": PROBE_BYTES, "bytes_read": found.io.bytes_read, "elapsed_ms": elapsed.as_secs_f64()*1000.0, "format": found.root.format}),
    )?;
    assert_ne!(found.root.format, caskade_installer::Format::Unknown);
    assert!(found.io.bytes_read < 16 * 1024 * 1024, "{:?}", found.io);
    assert!(
        elapsed < Duration::from_millis(200),
        "probe took {elapsed:?}"
    );
    Ok(())
}
#[test]
#[ignore = "T10: creates 100,000 real files; nightly release gate"]
fn hundred_thousand_files_are_discovered_within_one_second() -> Result<()> {
    let temp = temporary("caskade-perf-discovery-")?;
    discovery_tree(temp.path())?;
    // Report cold and repeated warm walks; the gate covers the first complete
    // discovery after construction, without timing tree creation or cleanup.
    let elapsed = discovery(temp.path())?;
    evidence(
        "discovery",
        json!({"files":100000,"directories":100,"elapsed_ms":elapsed.as_secs_f64()*1000.0}),
    )?;
    assert!(
        elapsed < Duration::from_secs(1),
        "discovery took {elapsed:?}"
    );
    Ok(())
}
#[test]
#[ignore = "T10: pinned official 7zz comparison; nightly release gate"]
fn extraction_is_within_one_and_a_half_times_the_pinned_seven_zip() -> Result<()> {
    let program =
        std::env::var_os("CASKADE_7ZZ").ok_or("set CASKADE_7ZZ to the pinned official 7zz")?;
    let seven = caskade_installer::formats::archive::sevenzip::SevenZip::pinned(program)?;
    let temp = temporary("caskade-perf-throughput-")?;
    let source = temp.path().join("throughput.tar");
    sparse_tar(&source, THROUGHPUT_BYTES)?;
    let mut rust = vec![];
    let mut reference = vec![];
    // Alternate ordering over five independent destinations; use both medians
    // to reduce cache/order noise, without comparing unlike archive workloads.
    for attempt in 0..5 {
        for native in if attempt % 2 == 0 {
            [true, false]
        } else {
            [false, true]
        } {
            let destination = temp.path().join(format!("{attempt}-{native}"));
            let elapsed = if native {
                extract(&source, &destination, THROUGHPUT_BYTES)?
            } else {
                fs::create_dir(&destination)?;
                let start = Instant::now();
                let output = Command::new(seven.program())
                    .args(["x", "-y", "-bb0", "-bsp0"])
                    .arg(format!("-o{}", destination.display()))
                    .arg("--")
                    .arg(&source)
                    .output()?;
                let elapsed = start.elapsed();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    fs::metadata(destination.join("payload.bin"))?.len(),
                    THROUGHPUT_BYTES
                );
                elapsed
            };
            if native {
                rust.push(elapsed)
            } else {
                reference.push(elapsed)
            }
            fs::remove_dir_all(destination)?;
        }
    }
    let rust = median(rust);
    let reference = median(reference);
    let ratio = rust.as_secs_f64() / reference.as_secs_f64();
    evidence(
        "extraction",
        json!({"payload_bytes":THROUGHPUT_BYTES,"format":"tar","samples":5,"rust_ms":rust.as_secs_f64()*1000.0,"seven_zip_ms":reference.as_secs_f64()*1000.0,"ratio":ratio}),
    )?;
    assert!(
        ratio <= 1.5,
        "Rust extraction {rust:?}, pinned 7zz {reference:?}; ratio {ratio:.3}"
    );
    Ok(())
}
#[test]
#[ignore = "T10: generates and extracts 10 GiB in /tmp; isolated RSS gate"]
fn ten_gib_archive_stays_below_256_mib_peak_rss() -> Result<()> {
    if std::env::var_os("CASKADE_PERF_RSS_CHILD").is_none() {
        let output = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "ten_gib_archive_stays_below_256_mib_peak_rss",
                "--ignored",
                "--nocapture",
            ])
            .env("CASKADE_PERF_RSS_CHILD", "1")
            .output()?;
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        assert!(
            output.status.success(),
            "RSS worker failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        return Ok(());
    }
    let temp = temporary("caskade-perf-rss-")?;
    let source = temp.path().join("ten-gib.tar");
    sparse_tar(&source, RSS_BYTES)?;
    let elapsed = extract(&source, &temp.path().join("output"), RSS_BYTES)?;
    let status = fs::read_to_string("/proc/self/status")?;
    let peak_kib: u64 = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .ok_or("no VmHWM in procfs")?
        .split_whitespace()
        .next()
        .ok_or("empty VmHWM")?
        .parse()?;
    evidence(
        "rss",
        json!({"payload_bytes":RSS_BYTES,"peak_rss_kib":peak_kib,"elapsed_ms":elapsed.as_secs_f64()*1000.0,"isolated_process":true}),
    )?;
    assert!(peak_kib < 256 * 1024, "peak RSS {peak_kib} KiB");
    Ok(())
}
