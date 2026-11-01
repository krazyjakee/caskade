use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "caskade-fixture-game-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn executable(&self) -> PathBuf {
        let name = if cfg!(windows) {
            "FixtureGame.exe"
        } else {
            "FixtureGame"
        };
        let path = self.0.join(name);
        copy_executable(&path);
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Avoid the filesystem's optimized copy/reflink path: on this shared host it
// intermittently left the newly copied executable busy at immediate exec time.
// Write, sync and close the destination explicitly before launching it.
fn copy_executable(path: &std::path::Path) {
    use std::io::Write;
    let source = env!("CARGO_BIN_EXE_caskade-fixture-game");
    let bytes = fs::read(source).unwrap();
    let mut file = fs::File::create(path).unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    drop(file);
    fs::set_permissions(path, fs::metadata(source).unwrap().permissions()).unwrap();
}

#[test]
fn token_records_executable_even_with_another_working_directory() {
    let scratch = Scratch::new();
    let exe = scratch.executable();
    let token = scratch.0.join("launch-token.txt");
    let status = Command::new(&exe)
        .current_dir(std::env::temp_dir())
        .arg("--token")
        .arg(&token)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(token).unwrap().trim(),
        fs::canonicalize(exe).unwrap().to_string_lossy()
    );
}

// Windows HKLM self-checks are native-runner tests. These tests never edit HKLM.
#[cfg(not(windows))]
#[test]
fn self_check_detects_stale_config_after_a_move() {
    let scratch = Scratch::new();
    let old = scratch.0.join("old install ü");
    let new = scratch.0.join("new install ü");
    fs::create_dir(&old).unwrap();
    let exe = old.join("FixtureGame");
    copy_executable(&exe);
    fs::write(
        old.join("config.ini"),
        format!("[Game]\nInstallPath={}\n", old.display()),
    )
    .unwrap();
    let token = scratch.0.join("token");
    assert!(
        Command::new(&exe)
            .arg("--token")
            .arg(&token)
            .arg("--self-check")
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read_to_string(&token).unwrap(), "ok\n");
    fs::rename(&old, &new).unwrap();
    let result = Command::new(new.join("FixtureGame"))
        .arg("--token")
        .arg(&token)
        .arg("--self-check")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(fs::read_to_string(&token).unwrap().starts_with("mismatch:"));
    fs::write(
        new.join("config.ini"),
        format!("InstallPath={}\n", new.display()),
    )
    .unwrap();
    assert!(
        Command::new(new.join("FixtureGame"))
            .arg("--token")
            .arg(&token)
            .arg("--self-check")
            .status()
            .unwrap()
            .success()
    );
}

#[cfg(unix)]
#[test]
fn self_check_accepts_an_alias_to_the_install_directory() {
    let scratch = Scratch::new();
    let exe = scratch.executable();
    let alias = scratch.0.with_extension("alias");
    std::os::unix::fs::symlink(&scratch.0, &alias).unwrap();
    fs::write(
        scratch.0.join("config.ini"),
        format!("InstallPath={}\n", alias.display()),
    )
    .unwrap();
    let status = Command::new(exe)
        .arg("--token")
        .arg(scratch.0.join("token"))
        .arg("--self-check")
        .status()
        .unwrap();
    fs::remove_file(alias).unwrap();
    assert!(status.success());
}
