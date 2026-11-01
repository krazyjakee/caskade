//! Genuine RPM and Debian fixture construction; invoked only explicitly.
use std::{fs, path::Path, process::Command};

pub fn build(root: &Path) -> crate::Result<()> {
    let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".into());
    let script = root.join("xtask/package-fixtures.py");
    if !Command::new(&python)
        .arg(&script)
        .arg("--root")
        .arg(root)
        .status()?
        .success()
    {
        return Err("Debian fixture builder failed".into());
    }
    let fixtures = root.join("crates/caskade-installer/tests/fixtures");
    let mut builder = rpm::PackageBuilder::new(
        "caskade-rpm-self-contained",
        "1.0.0",
        "MIT",
        "x86_64",
        "Caskade Fixture Game",
    );
    builder.using_config(
        rpm::BuildConfig::v4()
            .compression(rpm::CompressionType::Gzip)
            .source_date(1_700_000_000),
    );
    builder.with_file(
        fixtures.join("bin/fixture-game-linux-x86_64"),
        rpm::FileOptions::new("/opt/caskade-fixture/FixtureGame.x86_64").permissions(0o755),
    )?;
    builder.with_file(
        fixtures.join("src/layouts/linux/start.sh"),
        rpm::FileOptions::new("/opt/caskade-fixture/start.sh").permissions(0o755),
    )?;
    builder.with_file(
        fixtures.join("src/packages/icon.svg"),
        rpm::FileOptions::new("/opt/caskade-fixture/icon.svg").permissions(0o644),
    )?;
    builder.with_file(
        fixtures.join("src/packages/fixture.desktop"),
        rpm::FileOptions::new("/usr/share/applications/caskade-fixture.desktop").permissions(0o644),
    )?;
    builder.post_install_script(fs::read_to_string(fixtures.join("src/packages/postinst"))?);
    builder.post_uninstall_script(fs::read_to_string(fixtures.join("src/packages/postrm"))?);
    builder
        .build()?
        .write_file(fixtures.join("bin/packages/rpm-self-contained.rpm"))?;
    if !Command::new(python)
        .arg(script)
        .args(["--rpm-expectation-only", "--root"])
        .arg(root)
        .status()?
        .success()
    {
        return Err("RPM fixture expectation builder failed".into());
    }
    crate::checks::write_lock(root)?;
    crate::checks::check_stale(root)?;
    crate::checks::check_size(root)?;
    Ok(())
}
