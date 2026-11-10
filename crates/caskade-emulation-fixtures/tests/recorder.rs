use caskade_emulation_fixtures::{FakeEmulator, Invocation};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn binary() -> FakeEmulator {
    FakeEmulator::from_binary(Path::new(env!("CARGO_BIN_EXE_fake-emulator"))).unwrap()
}

#[test]
fn native_binary_records_exact_tokens_unicode_cwd_allowlisted_environment_and_exit() {
    let temp = tempfile::tempdir().unwrap();
    let record = temp.path().join("invocation");
    let args = [
        "--core",
        "core with spaces.so",
        "Café 日本 (Disc 1).cue",
        "",
        "quoted \"token\"",
        "$(not a shell)",
    ];
    let output = binary()
        .command(&record)
        .args(args)
        .current_dir(temp.path())
        .env("CASKADE_FAKE_MARKER", "value with spaces \" and 日本")
        .env("CASKADE_FAKE_EXIT_CODE", "17")
        .env("CASKADE_FAKE_ENV_KEYS", "FIXTURE_CUSTOM_ENV")
        .env("FIXTURE_CUSTOM_ENV", "explicit test value")
        .env("PRIVATE_HOST_SECRET", "never record this")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(17));
    assert!(
        fs::metadata(record.with_extension("argv"))
            .unwrap()
            .is_file()
    );
    let invocation = Invocation::read(&record).unwrap();
    assert_eq!(invocation.argv, args);
    assert_eq!(invocation.cwd, temp.path().canonicalize().unwrap());
    assert_eq!(
        invocation.env["CASKADE_FAKE_MARKER"],
        "value with spaces \" and 日本"
    );
    assert_eq!(invocation.env["FIXTURE_CUSTOM_ENV"], "explicit test value");
    assert!(!invocation.env.contains_key("PRIVATE_HOST_SECRET"));
    assert!(invocation.pid > 0);
}

#[test]
fn detached_fake_child_outlives_launcher_and_has_a_separate_complete_record() {
    let temp = tempfile::tempdir().unwrap();
    let record = temp.path().join("launcher");
    let status = binary()
        .command(&record)
        .arg("game.cue")
        .env("CASKADE_FAKE_FORK", "detach")
        .env("CASKADE_FAKE_CHILD_SLEEP_MS", "250")
        .status()
        .unwrap();
    assert!(status.success());
    let parent = Invocation::read(&record).unwrap();
    let child_record = temp.path().join("launcher.child");
    let deadline = Instant::now() + Duration::from_secs(3);
    let child = loop {
        if let Ok(record) = Invocation::read(&child_record) {
            break record;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_ne!(child.pid, parent.pid);
    assert_eq!(child.argv, ["game.cue"]);
    assert_eq!(child.env["CASKADE_FAKE_PHASE"], "child");
    // Reap via an explicit wait-mode test as well; the detached process is
    // bounded and finishes before its temporary fixture goes away.
    std::thread::sleep(Duration::from_millis(300));
    let wait = temp.path().join("wait");
    assert!(
        binary()
            .command(&wait)
            .env("CASKADE_FAKE_FORK", "wait")
            .env("CASKADE_FAKE_CHILD_SLEEP_MS", "25")
            .status()
            .unwrap()
            .success()
    );
    assert!(Invocation::read(&temp.path().join("wait.child")).is_ok());
}

#[test]
fn a_fake_launcher_can_spawn_a_differently_named_emulator() {
    let temp = tempfile::tempdir().unwrap();
    let name = if cfg!(windows) {
        "retroarch.exe"
    } else {
        "retroarch"
    };
    let child = binary().copy_to(&temp.path().join(name)).unwrap();
    let record = temp.path().join("flatpak");
    assert!(
        binary()
            .command(&record)
            .arg("disc.cue")
            .env("CASKADE_FAKE_FORK", "wait")
            .env("CASKADE_FAKE_CHILD_PROGRAM", child.path())
            .env("CASKADE_FAKE_CHILD_SLEEP_MS", "25")
            .status()
            .unwrap()
            .success()
    );
    let child = Invocation::read(&temp.path().join("flatpak.child")).unwrap();
    assert_eq!(child.argv, ["disc.cue"]);
    assert_eq!(child.env["CASKADE_FAKE_PHASE"], "child");
}

#[cfg(unix)]
#[test]
fn generated_script_uses_the_same_record_contract_without_a_runtime_compiler() {
    // Copy the checked-in script source through the public fallback builder.
    // Use a subprocess to remove the CI override, never mutate global env.
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("script emulator");
    // Through `cp`, so this process never holds the program open for
    // writing while another test forks (#118, ETXTBSY).
    let status = std::process::Command::new("cp")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/src/recorder.sh"))
        .arg(&script)
        .status()
        .unwrap();
    assert!(status.success());
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let fake = FakeEmulator::from_binary(&script).unwrap();
    let record = temp.path().join("script");
    assert!(
        fake.command(&record)
            .args(["", "Café game.cue", "$(literal)"])
            .env("CASKADE_FAKE_MARKER", "script test")
            .status()
            .unwrap()
            .success()
    );
    let invocation = Invocation::read(&record).unwrap();
    assert_eq!(invocation.argv, ["", "Café game.cue", "$(literal)"]);
    assert_eq!(invocation.env["CASKADE_FAKE_MARKER"], "script test");
}
