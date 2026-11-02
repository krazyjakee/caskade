//! Plans, records and undos with the runner injected; the Linux script run
//! for real against a temp root; the Windows elevated side with its
//! commands recorded.

use super::*;
use crate::{AppPaths, emulation::OperatingSystem};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

/// A system in memory: what is on it, the requests it was asked, and how
/// the next prompt ends.
#[derive(Default)]
struct Fake {
    present: Mutex<BTreeSet<Change>>,
    requests: Mutex<Vec<Request>>,
    answer: Mutex<Option<Prompt>>,
    fail: Mutex<bool>,
}

impl Runner for Fake {
    fn present(&self, change: Change) -> bool {
        self.present.lock().unwrap().contains(&change)
    }

    fn run(&self, request: &Request) -> Result<Prompt> {
        self.requests.lock().unwrap().push(request.clone());
        if *self.fail.lock().unwrap() {
            return Err(Error::Invalid("failed and undone".into()));
        }
        let answer = self.answer.lock().unwrap().unwrap_or(Prompt::Done);
        if answer == Prompt::Done {
            let mut present = self.present.lock().unwrap();
            for step in &request.steps {
                match step {
                    Step::Add(change) => present.insert(*change),
                    Step::Remove(change) => present.remove(change),
                };
            }
        }
        Ok(answer)
    }
}

fn elevation(os: OperatingSystem) -> (Elevation, Arc<Fake>, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake::default());
    let elevation = Elevation::with_runner(fake.clone(), os, &AppPaths::under(temp.path()));
    (elevation, fake, temp)
}

fn set(changes: &[Change]) -> BTreeSet<Change> {
    changes.iter().copied().collect()
}

#[tokio::test]
async fn one_prompt_adds_what_is_missing_and_the_undo_removes_only_that() {
    let (elevation, fake, _temp) = elevation(OperatingSystem::Linux);
    // uhid was loaded before Caskade asked: it stays out of the record.
    fake.present.lock().unwrap().insert(Change::UhidLoaded);
    let added = elevation.apply(Plan::StreamingHost).await.unwrap();
    assert_eq!(
        added,
        Outcome::Done(set(&[Change::UdevRules, Change::ModulesLoad]))
    );
    let requests = fake.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1, "exactly one prompt");
    assert_eq!(
        requests[0].steps,
        [Step::Add(Change::UdevRules), Step::Add(Change::ModulesLoad)]
    );
    assert_eq!(
        elevation.record(Plan::StreamingHost).unwrap().added,
        set(&[Change::UdevRules, Change::ModulesLoad])
    );
    // Everything there: no prompt at all.
    assert_eq!(
        elevation.apply(Plan::StreamingHost).await.unwrap(),
        Outcome::NothingToDo
    );
    assert_eq!(fake.requests.lock().unwrap().len(), 1);

    let removed = elevation.undo(Plan::StreamingHost).await.unwrap();
    assert_eq!(
        removed,
        Outcome::Done(set(&[Change::UdevRules, Change::ModulesLoad]))
    );
    let requests = fake.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    // In reverse order, and never the module that was there before.
    assert_eq!(
        requests[1].steps,
        [
            Step::Remove(Change::ModulesLoad),
            Step::Remove(Change::UdevRules)
        ]
    );
    assert_eq!(*fake.present.lock().unwrap(), set(&[Change::UhidLoaded]));
    assert!(
        elevation
            .record(Plan::StreamingHost)
            .unwrap()
            .added
            .is_empty()
    );
    // Nothing left to undo: no prompt.
    assert_eq!(
        elevation.undo(Plan::StreamingHost).await.unwrap(),
        Outcome::NothingToDo
    );
    assert_eq!(fake.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_cancelled_refused_or_failed_prompt_changes_nothing() {
    for (answer, fail, expected) in [
        (Some(Prompt::Cancelled), false, Some(Outcome::Cancelled)),
        (Some(Prompt::Refused), false, Some(Outcome::Refused)),
        (None, true, None),
    ] {
        let (elevation, fake, _temp) = elevation(OperatingSystem::Windows);
        *fake.answer.lock().unwrap() = answer;
        *fake.fail.lock().unwrap() = fail;
        let outcome = elevation.apply(Plan::StreamingHost).await;
        match expected {
            Some(expected) => {
                let outcome = outcome.unwrap();
                assert!(outcome.message().is_some());
                assert_eq!(outcome, expected);
            }
            None => assert!(outcome.is_err()),
        }
        assert_eq!(fake.requests.lock().unwrap().len(), 1, "one prompt");
        assert!(fake.present.lock().unwrap().is_empty());
        assert!(
            elevation
                .record(Plan::StreamingHost)
                .unwrap()
                .added
                .is_empty()
        );
    }
}

#[tokio::test]
async fn the_undo_spares_what_someone_else_added_and_survives_a_cancel() {
    let (elevation, fake, _temp) = elevation(OperatingSystem::Windows);
    fake.present.lock().unwrap().insert(Change::FirewallTcp);
    assert_eq!(
        elevation.apply(Plan::StreamingHost).await.unwrap(),
        Outcome::Done(set(&[Change::Service, Change::FirewallUdp]))
    );
    // A cancelled undo keeps the record for next time.
    *fake.answer.lock().unwrap() = Some(Prompt::Cancelled);
    assert_eq!(
        elevation.undo(Plan::StreamingHost).await.unwrap(),
        Outcome::Cancelled
    );
    assert_eq!(
        elevation.record(Plan::StreamingHost).unwrap().added,
        set(&[Change::Service, Change::FirewallUdp])
    );
    *fake.answer.lock().unwrap() = None;
    elevation.undo(Plan::StreamingHost).await.unwrap();
    let last = fake.requests.lock().unwrap().last().cloned().unwrap();
    assert_eq!(
        last.steps,
        [
            Step::Remove(Change::FirewallUdp),
            Step::Remove(Change::Service)
        ]
    );
    assert_eq!(*fake.present.lock().unwrap(), set(&[Change::FirewallTcp]));
}

/// A record holds names only; a file that says anything else is refused
/// before any prompt.
#[tokio::test]
async fn a_tampered_record_never_reaches_a_prompt() {
    let (elevation, fake, temp) = elevation(OperatingSystem::Linux);
    let path = temp.path().join("data/elevation/streaming-host.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, r#"{"added":["udev-rules","rm -rf /"]}"#).unwrap();
    assert!(elevation.undo(Plan::StreamingHost).await.is_err());
    assert!(fake.requests.lock().unwrap().is_empty());
}

#[test]
fn the_linux_command_is_constants_and_step_names() {
    let request = Request {
        steps: vec![
            Step::Add(Change::UdevRules),
            Step::Add(Change::ModulesLoad),
            Step::Add(Change::UhidLoaded),
        ],
        host_dir: "/home/u/.local/share/x/streaming/host".into(),
    };
    let argv = linux::command(&request).unwrap();
    assert_eq!(
        argv,
        [
            "pkexec",
            "/bin/sh",
            "-c",
            linux::SCRIPT,
            "caskade",
            "add-rules",
            "add-modules",
            "load-uhid"
        ]
    );
    assert!(!argv.iter().any(|arg| arg.contains("/home/u")));
    // A Windows change has no Linux step.
    let wrong = Request {
        steps: vec![Step::Add(Change::Service)],
        host_dir: "/x".into(),
    };
    assert!(linux::command(&wrong).is_err());
    assert_eq!(linux::prompt(Some(0), "").unwrap(), Prompt::Done);
    assert_eq!(linux::prompt(Some(126), "").unwrap(), Prompt::Cancelled);
    assert_eq!(linux::prompt(Some(127), "").unwrap(), Prompt::Refused);
    assert!(linux::prompt(Some(1), "couldn't load uhid").is_err());
}

/// The real script against a temp root, with logging stand-ins for
/// `modprobe` and `udevadm`.
#[cfg(unix)]
mod script {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};

    struct Root {
        temp: tempfile::TempDir,
    }

    impl Root {
        fn new(modprobe_fails: bool) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let bin = temp.path().join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let log = temp.path().join("log");
            for (name, fails) in [("modprobe", modprobe_fails), ("udevadm", false)] {
                let path = bin.join(name);
                let exit = if fails { 1 } else { 0 };
                std::fs::write(
                    &path,
                    format!(
                        "#!/bin/sh\necho \"{name} $*\" >> '{}'\nexit {exit}\n",
                        log.display()
                    ),
                )
                .unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            Self { temp }
        }

        fn run(&self, steps: &[&str]) -> i32 {
            let script = linux::SCRIPT.replacen(
                "R=\"\"",
                &format!("R=\"{}\"", self.temp.path().join("root").display()),
                1,
            );
            Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .arg("caskade")
                .args(steps)
                .env(
                    "PATH",
                    format!("{}:/usr/bin:/bin", self.temp.path().join("bin").display()),
                )
                .status()
                .unwrap()
                .code()
                .unwrap()
        }

        fn file(&self, absolute: &str) -> std::path::PathBuf {
            self.temp
                .path()
                .join("root")
                .join(absolute.trim_start_matches('/'))
        }

        fn log(&self) -> String {
            std::fs::read_to_string(self.temp.path().join("log")).unwrap_or_default()
        }
    }

    #[test]
    fn it_adds_and_removes_exactly_its_files_and_module() {
        let root = Root::new(false);
        let rules = root.file(linux::RULES_FILE);
        let modules = root.file(linux::MODULES_FILE);
        // Someone else's rules beside ours are never touched.
        let other = root.file("/etc/udev/rules.d/60-sunshine.rules");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(&other, "theirs").unwrap();

        assert_eq!(root.run(&["add-rules", "add-modules", "load-uhid"]), 0);
        let text = std::fs::read_to_string(&rules).unwrap();
        for node in [
            "KERNEL==\"uinput\"",
            "KERNEL==\"uhid\"",
            "TAG+=\"uaccess\"",
            "GROUP=\"input\"",
        ] {
            assert!(text.contains(node), "{node}");
        }
        assert_eq!(std::fs::read_to_string(&modules).unwrap(), "uhid\n");
        assert!(root.log().contains("modprobe uhid"));
        assert!(root.log().contains("udevadm control --reload-rules"));

        assert_eq!(
            root.run(&["unload-uhid", "remove-modules", "remove-rules"]),
            0
        );
        assert!(!rules.exists() && !modules.exists());
        assert!(root.log().contains("modprobe -r uhid"));
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "theirs");
    }

    #[test]
    fn a_failing_step_undoes_the_ones_before_it() {
        let root = Root::new(true);
        assert_eq!(root.run(&["add-rules", "add-modules", "load-uhid"]), 1);
        assert!(!root.file(linux::RULES_FILE).exists());
        assert!(!root.file(linux::MODULES_FILE).exists());
    }

    #[test]
    fn an_unknown_step_changes_nothing() {
        let root = Root::new(false);
        assert_eq!(root.run(&["add-rules", "rm -rf /"]), 64);
        assert!(!root.file(linux::RULES_FILE).exists());
        assert!(!Path::new(&root.temp.path().join("root")).exists());
    }
}

#[test]
fn the_windows_elevated_side_takes_names_and_rolls_back() {
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("data").join("streaming").join("host");
    std::fs::create_dir_all(host.join("tools")).unwrap();
    std::fs::write(host.join("sunshine.exe"), "").unwrap();
    std::fs::write(host.join("tools").join("sunshinesvc.exe"), "").unwrap();
    let args = |steps: &[&str], host: &Path| {
        let mut args = vec!["caskade.exe".to_owned(), windows::ELEVATED_ARG.to_owned()];
        args.extend(steps.iter().map(|step| (*step).to_owned()));
        args.push("--host".into());
        args.push(host.to_string_lossy().into_owned());
        args
    };

    let mut ran = Vec::new();
    let code = windows::perform_with(
        &args(
            &["add-service", "add-firewall-tcp", "add-firewall-udp"],
            &host,
        ),
        &mut |program, args| {
            ran.push(format!("{program} {}", args.join(" ")));
            true
        },
    );
    assert_eq!(code, 0);
    assert_eq!(ran.len(), 4, "{ran:#?}");
    let svc = host.join("tools").join("sunshinesvc.exe");
    assert!(ran[0].starts_with(&format!(
        "sc.exe create CaskadeStreaming binPath= \"{}\" start= demand",
        svc.display()
    )));
    assert!(ran[1].starts_with("sc.exe sdset CaskadeStreaming D:"));
    assert!(
        ran[1].contains(";;;IU)"),
        "the interactive user may start and stop it"
    );
    assert!(
        ran[2].contains("name=Caskade streaming (TCP)")
            && ran[2].contains("localport=47984,47989,48010")
    );
    assert!(ran[3].contains("protocol=UDP") && ran[3].contains("localport=47998-48000"));
    let exe = host.join("sunshine.exe");
    assert!(ran[2].contains(&format!("program={}", exe.display())));

    // The UDP rule fails: the service and TCP rule are undone.
    let mut ran = Vec::new();
    let code = windows::perform_with(
        &args(
            &["add-service", "add-firewall-tcp", "add-firewall-udp"],
            &host,
        ),
        &mut |program, args| {
            let line = format!("{program} {}", args.join(" "));
            let ok = !line.contains("(UDP)");
            ran.push(line);
            ok
        },
    );
    assert_eq!(code, 1);
    let undone: Vec<&String> = ran.iter().skip(4).collect();
    assert!(
        undone[0].contains("delete rule name=Caskade streaming (TCP)"),
        "{ran:#?}"
    );
    assert!(undone[1].ends_with("stop CaskadeStreaming"));
    assert!(undone[2].ends_with("delete CaskadeStreaming"));

    // Unknown steps, and hosts that aren't Caskade's, run nothing.
    for wrong in [
        args(&["add-service", "cmd /c evil"], &host),
        args(&["add-service"], &temp.path().join("elsewhere")),
        args(&["add-service"], &host.join("..").join("host")),
        vec!["caskade.exe".into(), windows::ELEVATED_ARG.into()],
    ] {
        let code = windows::perform_with(&wrong, &mut |_, _| panic!("ran {wrong:?}"));
        assert_eq!(code, 64);
    }
}

#[test]
fn the_windows_launcher_passes_names_and_caskades_own_paths() {
    let request = Request {
        steps: vec![Step::Add(Change::Service), Step::Add(Change::FirewallTcp)],
        host_dir: r"C:\Users\Ada O'Neil\AppData\Local\caskade\data\streaming\host".into(),
    };
    let argv =
        windows::launcher(Path::new(r"C:\Program Files\Caskade\caskade.exe"), &request).unwrap();
    assert_eq!(argv[0], "powershell.exe");
    let script = argv.last().unwrap();
    assert!(script.contains("-FilePath 'C:\\Program Files\\Caskade\\caskade.exe'"));
    assert!(script.contains("-Verb RunAs -Wait -PassThru"));
    assert!(script.contains(
        r#"-ArgumentList '--elevated add-service add-firewall-tcp --host "C:\Users\Ada O''Neil\AppData\Local\caskade\data\streaming\host"'"#
    ));
    assert!(script.contains("catch { exit 1223 }"));
    assert_eq!(windows::prompt(Some(1223)).unwrap(), Prompt::Cancelled);
    assert_eq!(windows::prompt(Some(0)).unwrap(), Prompt::Done);
    assert!(windows::prompt(Some(1)).is_err());
    assert_eq!(windows::quote_arg(r#"a "b"\"#), r#""a \"b\"\\""#);
    assert_eq!(windows::quote_arg("plain"), "plain");
}

use std::path::Path;
