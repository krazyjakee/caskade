//! Plans, records and undos with the runner injected; the Linux script run
//! for real against a temp root; the Windows elevated side with its
//! commands recorded.

use super::*;
use caskade_core::{AppPaths, emulation::OperatingSystem};
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
    let paths = AppPaths::under(temp.path());
    std::fs::create_dir_all(paths.data.join("streaming/host")).unwrap();
    let elevation = Elevation::with_runner(fake.clone(), os, &paths);
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
        Outcome::Done(set(&[
            Change::HostProgram,
            Change::Service,
            Change::FirewallUdp,
            Change::GamepadDriver
        ]))
    );
    // Copying the host program carries the staged manifest's hash.
    let first = fake.requests.lock().unwrap()[0].clone();
    assert_eq!(first.steps[0], Step::Add(Change::HostProgram));
    assert_eq!(first.manifest.as_deref().map(str::len), Some(64));
    // A cancelled undo keeps the record for next time.
    *fake.answer.lock().unwrap() = Some(Prompt::Cancelled);
    assert_eq!(
        elevation.undo(Plan::StreamingHost).await.unwrap(),
        Outcome::Cancelled
    );
    assert_eq!(
        elevation.record(Plan::StreamingHost).unwrap().added,
        set(&[
            Change::HostProgram,
            Change::Service,
            Change::FirewallUdp,
            Change::GamepadDriver
        ])
    );
    *fake.answer.lock().unwrap() = None;
    elevation.undo(Plan::StreamingHost).await.unwrap();
    let last = fake.requests.lock().unwrap().last().cloned().unwrap();
    assert_eq!(
        last.steps,
        [
            Step::Remove(Change::FirewallUdp),
            Step::Remove(Change::Service),
            Step::Remove(Change::HostProgram)
        ]
    );
    assert_eq!(last.manifest, None);
    // The gamepad driver stays: other programs use it.
    assert_eq!(
        *fake.present.lock().unwrap(),
        set(&[Change::FirewallTcp, Change::GamepadDriver])
    );
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
        manifest: None,
    };
    let argv = linux::command(&request).unwrap();
    assert_eq!(
        argv,
        [
            "pkexec",
            "/bin/sh",
            "-s",
            linux::NAME,
            "add-rules",
            "add-modules",
            "load-uhid"
        ]
    );
    assert!(!argv.iter().any(|arg| arg.contains("/home/u")));
    // The longest run there is, as the prompt shows it: pkexec prints the
    // start of the command line, so Caskade's name has to come first and
    // none of the script may be in it.
    let removal = Request {
        steps: vec![
            Step::Remove(Change::UdevRules),
            Step::Remove(Change::ModulesLoad),
            Step::Remove(Change::UhidLoaded),
        ],
        host_dir: "/x".into(),
        manifest: None,
    };
    let shown = linux::command(&removal).unwrap()[1..].join(" ");
    assert!(shown.starts_with("/bin/sh -s Caskade"), "{shown}");
    assert!(shown.len() <= 80, "pkexec cuts it short: {shown}");
    assert!(
        !shown.contains('\n') && !shown.contains("set -u"),
        "{shown}"
    );
    // A Windows change has no Linux step.
    let wrong = Request {
        steps: vec![Step::Add(Change::Service)],
        host_dir: "/x".into(),
        manifest: None,
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

        /// The script on its standard input, as pkexec hands it over.
        fn run(&self, steps: &[&str]) -> i32 {
            let script = linux::SCRIPT.replacen(
                "R=\"\"",
                &format!("R=\"{}\"", self.temp.path().join("root").display()),
                1,
            );
            self.feed(&script, steps)
        }

        fn feed(&self, script: &str, steps: &[&str]) -> i32 {
            let mut command = Command::new("/bin/sh");
            command
                .arg("-s")
                .arg(linux::NAME)
                .args(steps)
                .env(
                    "PATH",
                    format!("{}:/usr/bin:/bin", self.temp.path().join("bin").display()),
                )
                .stderr(std::process::Stdio::null());
            linux::with_script(command, script)
                .unwrap()
                .status
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

    /// A script cut off on its way in is a shell error, never half a run.
    #[test]
    fn half_a_script_changes_nothing() {
        let root = Root::new(false);
        let script = linux::SCRIPT.replacen(
            "R=\"\"",
            &format!("R=\"{}\"", root.temp.path().join("root").display()),
            1,
        );
        let cut = script.find("load-uhid) modprobe uhid").unwrap();
        assert_ne!(root.feed(&script[..cut], &["add-rules", "load-uhid"]), 0);
        assert!(!root.file(linux::RULES_FILE).exists());
        assert_eq!(root.log(), "");
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

/// A staged host (`<data>/streaming/host`) and the administrator-only
/// target it is copied to.
fn staged() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("data").join("streaming").join("host");
    std::fs::create_dir_all(host.join("tools")).unwrap();
    std::fs::create_dir_all(host.join("config")).unwrap();
    std::fs::write(host.join("sunshine.exe"), "host").unwrap();
    std::fs::write(host.join("tools").join("sunshinesvc.exe"), "svc").unwrap();
    std::fs::write(
        host.join("config").join("sunshine.conf"),
        "sunshine_name = Den",
    )
    .unwrap();
    let target = temp
        .path()
        .join("pf")
        .join("Caskade")
        .join("Streaming")
        .join("host");
    (temp, host, target)
}

fn elevated(steps: &[&str], host: &Path, manifest: Option<&str>) -> Vec<String> {
    let mut args = vec!["caskade.exe".to_owned(), windows::ELEVATED_ARG.to_owned()];
    args.extend(steps.iter().map(|step| (*step).to_owned()));
    args.push("--host".into());
    args.push(host.to_string_lossy().into_owned());
    if let Some(manifest) = manifest {
        args.push("--manifest".into());
        args.push(manifest.to_owned());
    }
    args
}

#[test]
fn the_windows_host_is_copied_checked_and_pointed_at_from_program_files() {
    let (_temp, host, target) = staged();
    let manifest = windows::write_manifest(&host).unwrap();
    let all = [
        "add-host",
        "add-service",
        "add-firewall-tcp",
        "add-firewall-udp",
    ];
    let mut ran = Vec::new();
    let code = windows::perform_with(
        &elevated(&all, &host, Some(&manifest)),
        &target,
        &mut |program, args| {
            ran.push(format!("{program} {}", args.join(" ")));
            true
        },
    );
    assert_eq!(code, 0);
    // The program and its configuration, as staged; not Caskade's records.
    assert_eq!(read(&target.join("sunshine.exe")), "host");
    assert_eq!(
        read(&target.join("config").join("sunshine.conf")),
        "sunshine_name = Den"
    );
    assert!(!target.join(".caskade").exists());
    assert_eq!(ran.len(), 4, "{ran:#?}");
    let svc = target.join("tools").join("sunshinesvc.exe");
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
    // Everything points at the administrator-only copy, nothing at staging.
    assert!(ran[2].contains(&format!(
        "program={}",
        target.join("sunshine.exe").display()
    )));
    assert!(
        !ran.iter()
            .any(|line| line.contains(&*host.to_string_lossy()))
    );

    // Undone: the rules, the service, then the folder (and its empty parents).
    let mut ran = Vec::new();
    let undo = [
        "remove-firewall-udp",
        "remove-firewall-tcp",
        "remove-service",
        "remove-host",
    ];
    assert_eq!(
        windows::perform_with(
            &elevated(&undo, &host, None),
            &target,
            &mut |program, args| {
                ran.push(format!("{program} {}", args.join(" ")));
                true
            }
        ),
        0
    );
    assert_eq!(ran.len(), 4);
    assert!(!target.exists() && !target.parent().unwrap().exists());
    assert!(host.join("sunshine.exe").exists(), "staging stays");
}

#[test]
fn a_changed_file_or_manifest_is_refused_and_a_failure_undoes_everything() {
    let (_temp, host, target) = staged();
    let manifest = windows::write_manifest(&host).unwrap();
    let never = &mut |_: &str, _: &[String]| -> bool { panic!("nothing runs") };
    // A staged file changed after Caskade checked it.
    std::fs::write(host.join("sunshine.exe"), "swapped").unwrap();
    assert_eq!(
        windows::perform_with(
            &elevated(&["add-host"], &host, Some(&manifest)),
            &target,
            never
        ),
        1
    );
    assert!(!target.exists() && !target.with_extension("new").exists());
    // A manifest that isn't the one Caskade wrote.
    let manifest = windows::write_manifest(&host).unwrap();
    let other = "0".repeat(64);
    assert_eq!(
        windows::perform_with(
            &elevated(&["add-host"], &host, Some(&other)),
            &target,
            never
        ),
        1
    );
    assert!(!target.exists());
    // The service and rules can't point at a missing copy.
    assert_eq!(
        windows::perform_with(&elevated(&["add-service"], &host, None), &target, never),
        1
    );

    // The UDP rule fails: the TCP rule, the service and the copy go.
    let mut ran = Vec::new();
    let all = [
        "add-host",
        "add-service",
        "add-firewall-tcp",
        "add-firewall-udp",
    ];
    let code = windows::perform_with(
        &elevated(&all, &host, Some(&manifest)),
        &target,
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
    assert!(!target.exists());

    // Unknown steps, foreign staging folders and missing manifests run nothing.
    for wrong in [
        elevated(&["add-service", "cmd /c evil"], &host, None),
        elevated(&["add-host"], &host, None),
        elevated(&["add-host"], &host, Some("not-a-hash")),
        elevated(
            &["add-host"],
            &host.parent().unwrap().join("elsewhere"),
            Some(&manifest),
        ),
        vec!["caskade.exe".into(), windows::ELEVATED_ARG.into()],
    ] {
        let code = windows::perform_with(&wrong, &target, &mut |_, _| panic!("ran {wrong:?}"));
        assert_eq!(code, 64, "{wrong:?}");
    }
}

#[test]
fn program_files_comes_from_the_registry_answer() {
    let answer = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\r\n    ProgramFilesDir    REG_SZ    C:\\Program Files\r\n\r\n";
    #[cfg(windows)]
    assert_eq!(
        windows::program_files(answer),
        Some(r"C:\Program Files".into())
    );
    // Elsewhere a Windows path isn't absolute; only the parsing is shared.
    #[cfg(not(windows))]
    assert_eq!(windows::program_files(answer), None);
    assert_eq!(
        windows::program_files("ProgramFilesDir REG_SZ relative"),
        None
    );
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn the_windows_launcher_passes_names_and_caskades_own_paths() {
    let request = Request {
        steps: vec![Step::Add(Change::HostProgram), Step::Add(Change::Service)],
        host_dir: r"C:\Users\Ada O'Neil\AppData\Local\caskade\data\streaming\host".into(),
        manifest: Some("ab".repeat(32)),
    };
    let argv =
        windows::launcher(Path::new(r"C:\Program Files\Caskade\caskade.exe"), &request).unwrap();
    assert_eq!(argv[0], "powershell.exe");
    let script = argv.last().unwrap();
    assert!(script.contains("-FilePath 'C:\\Program Files\\Caskade\\caskade.exe'"));
    assert!(script.contains("-Verb RunAs -Wait -PassThru"));
    assert!(script.contains(
        r#"-ArgumentList '--elevated add-host add-service --host "C:\Users\Ada O''Neil\AppData\Local\caskade\data\streaming\host" --manifest abababababababababababababababababababababababababababababababab'"#
    ));
    assert!(script.contains("catch { exit 1223 }"));
    assert_eq!(windows::prompt(Some(1223)).unwrap(), Prompt::Cancelled);
    assert_eq!(windows::prompt(Some(0)).unwrap(), Prompt::Done);
    assert!(windows::prompt(Some(1)).is_err());
    assert_eq!(windows::quote_arg(r#"a "b"\"#), r#""a \"b\"\\""#);
    assert_eq!(windows::quote_arg("plain"), "plain");
}

use std::path::Path;

/// The host's own state as it is after a device paired and Caskade
/// published a game, in the installed copy.
fn host_ran(target: &Path) {
    let config = target.join("config");
    std::fs::write(
        config.join("sunshine_state.json"),
        r#"{"username":"caskade-1","password":"H1","salt":"s1","root":{"uniqueid":"U1","named_devices":[{"name":"Deck","cert":"PEM"}]}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(config.join("credentials")).unwrap();
    std::fs::write(config.join("credentials").join("cacert.pem"), "CERT").unwrap();
    std::fs::write(config.join("credentials").join("cakey.pem"), "KEY").unwrap();
    std::fs::write(config.join("apps.json"), r#"{"apps":[{"name":"Celeste"}]}"#).unwrap();
}

fn add_host(host: &Path, target: &Path) -> i32 {
    let manifest = windows::write_manifest(host).unwrap();
    windows::perform_with(
        &elevated(&["add-host"], host, Some(&manifest)),
        target,
        &mut |_, _| true,
    )
}

#[test]
fn a_new_windows_copy_keeps_the_hosts_pairings_and_hosting_off_keeps_them_aside() {
    let (_temp, host, target) = staged();
    std::fs::write(
        host.join("config").join("sunshine_state.json"),
        r#"{"username":"caskade-1","password":"H1","salt":"s1"}"#,
    )
    .unwrap();
    assert_eq!(add_host(&host, &target), 0);
    host_ran(&target);

    // An update with new sign-in details (a repair after the keychain lost
    // them): the new program and sign-in, the old pairings and identity.
    std::fs::write(host.join("sunshine.exe"), "host 2").unwrap();
    std::fs::write(
        host.join("config").join("sunshine_state.json"),
        r#"{"username":"caskade-2","password":"H2","salt":"s2"}"#,
    )
    .unwrap();
    assert_eq!(add_host(&host, &target), 0);
    assert_eq!(read(&target.join("sunshine.exe")), "host 2");
    let state: serde_json::Value =
        serde_json::from_str(&read(&target.join("config/sunshine_state.json"))).unwrap();
    assert_eq!(state["username"], "caskade-2");
    assert_eq!(state["root"]["uniqueid"], "U1");
    assert_eq!(state["root"]["named_devices"][0]["name"], "Deck");
    assert_eq!(read(&target.join("config/credentials/cacert.pem")), "CERT");
    assert_eq!(read(&target.join("config/credentials/cakey.pem")), "KEY");
    assert!(read(&target.join("config/apps.json")).contains("Celeste"));

    // Hosting off: no program file left; the state kept aside, still in
    // the administrator-only folder.
    assert_eq!(
        windows::perform_with(
            &elevated(&["remove-host"], &host, None),
            &target,
            &mut |_, _| { true }
        ),
        0
    );
    assert!(!target.exists());
    let kept = windows::kept_dir(&target);
    assert_eq!(kept, target.parent().unwrap().join("kept"));
    assert!(read(&kept.join("config/sunshine_state.json")).contains("Deck"));
    assert_eq!(read(&kept.join("config/credentials/cakey.pem")), "KEY");
    let left: Vec<_> = walk(target.parent().unwrap());
    assert!(
        left.iter().all(|path| !path.ends_with(".exe")),
        "no program file: {left:?}"
    );

    // On again: the same identity and paired devices; nothing kept aside.
    assert_eq!(add_host(&host, &target), 0);
    let state = read(&target.join("config/sunshine_state.json"));
    assert!(state.contains("Deck") && state.contains("U1"), "{state}");
    assert_eq!(read(&target.join("config/credentials/cacert.pem")), "CERT");
    assert!(!kept.exists());
}

/// Every file under `dir`, as `/`-joined text.
fn walk(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path.to_string_lossy().replace('\\', "/"));
        }
    }
    found
}

#[tokio::test]
async fn a_changed_windows_copy_is_copied_again_behind_one_prompt() {
    let (elevation, fake, temp) = elevation(OperatingSystem::Windows);
    let host = temp.path().join("data").join("streaming").join("host");
    std::fs::create_dir_all(host.join("tools")).unwrap();
    std::fs::create_dir_all(host.join("config")).unwrap();
    std::fs::write(host.join("sunshine.exe"), "host").unwrap();
    std::fs::write(host.join("tools").join("sunshinesvc.exe"), "svc").unwrap();
    std::fs::write(host.join("config").join("sunshine.conf"), "a").unwrap();
    std::fs::write(host.join("config").join("sunshine_state.json"), "{}").unwrap();
    let target = temp
        .path()
        .join("pf")
        .join("Caskade")
        .join("Streaming")
        .join("host");
    assert_eq!(add_host(&host, &target), 0);
    let elevation = elevation.with_host_target(target.clone());
    fake.present
        .lock()
        .unwrap()
        .extend(Plan::StreamingHost.changes(OperatingSystem::Windows));

    // As installed: nothing to do, whatever the host itself changed.
    assert_eq!(
        elevation.apply(Plan::StreamingHost).await.unwrap(),
        Outcome::NothingToDo
    );
    host_ran(&target);
    assert_eq!(
        elevation.apply(Plan::StreamingHost).await.unwrap(),
        Outcome::NothingToDo
    );

    // An update, or configuration Caskade wrote again: one prompt copies
    // only the program, checked by its manifest.
    std::fs::write(host.join("config").join("sunshine.conf"), "b").unwrap();
    assert_eq!(
        elevation.apply(Plan::StreamingHost).await.unwrap(),
        Outcome::Done(set(&[Change::HostProgram]))
    );
    let last = fake.requests.lock().unwrap().last().unwrap().clone();
    assert_eq!(last.steps, [Step::Add(Change::HostProgram)]);
    assert!(last.manifest.is_some());

    // A program file deleted from the installed copy counts too.
    std::fs::write(target.join("config").join("sunshine.conf"), "b").unwrap();
    assert_eq!(
        elevation.apply(Plan::StreamingHost).await.unwrap(),
        Outcome::NothingToDo
    );
    std::fs::remove_file(target.join("tools").join("sunshinesvc.exe")).unwrap();
    assert!(matches!(
        elevation.apply(Plan::StreamingHost).await.unwrap(),
        Outcome::Done(_)
    ));
}

#[tokio::test]
async fn the_gamepad_driver_joins_the_windows_prompt_and_stays_after_the_undo() {
    let (elevation, fake, _temp) = elevation(OperatingSystem::Windows);
    assert!(
        Plan::StreamingHost
            .changes(OperatingSystem::Windows)
            .contains(&Change::GamepadDriver)
    );
    assert!(
        !Plan::StreamingHost
            .changes(OperatingSystem::Linux)
            .contains(&Change::GamepadDriver)
    );
    let done = elevation.apply(Plan::StreamingHost).await.unwrap();
    assert!(matches!(done, Outcome::Done(added) if added.contains(&Change::GamepadDriver)));
    assert_eq!(fake.requests.lock().unwrap().len(), 1, "one prompt");
    assert!(
        fake.requests.lock().unwrap()[0]
            .steps
            .contains(&Step::Add(Change::GamepadDriver))
    );
    // Off: everything else goes; the driver stays (other programs use it).
    elevation.undo(Plan::StreamingHost).await.unwrap();
    let undo = fake.requests.lock().unwrap().last().unwrap().clone();
    assert!(!undo.steps.contains(&Step::Remove(Change::GamepadDriver)));
    assert!(undo.steps.contains(&Step::Remove(Change::Service)));
    assert!(
        fake.present
            .lock()
            .unwrap()
            .contains(&Change::GamepadDriver)
    );
    // Already installed (by the user, or before): on again asks for nothing
    // about it.
    elevation.apply(Plan::StreamingHost).await.unwrap();
    let again = fake.requests.lock().unwrap().last().unwrap().clone();
    assert!(!again.steps.contains(&Step::Add(Change::GamepadDriver)));
}

#[test]
fn only_the_pinned_gamepad_installer_runs_and_never_from_staging() {
    let (_temp, host, target) = staged();
    let download = windows::gamepad_driver_download(&host);
    std::fs::create_dir_all(download.parent().unwrap()).unwrap();
    std::fs::write(&download, "not the driver").unwrap();
    assert_eq!(add_host(&host, &target), 0);
    let mut ran = Vec::new();
    let code = windows::perform_with(
        &elevated(&["add-gamepad-driver"], &host, None),
        &target,
        &mut |program, args| {
            ran.push(format!("{program} {}", args.join(" ")));
            true
        },
    );
    // Best effort: the step succeeds, nothing ran, nothing is left.
    assert_eq!(code, 0);
    assert!(ran.is_empty(), "{ran:?}");
    assert!(!target.parent().unwrap().join("drivers").exists());
    // Removing it is never asked of the system.
    let mut ran = Vec::new();
    assert_eq!(
        windows::perform_with(
            &elevated(&["remove-gamepad-driver"], &host, None),
            &target,
            &mut |program, args| {
                ran.push(format!("{program} {}", args.join(" ")));
                true
            }
        ),
        0
    );
    assert!(ran.is_empty());
    assert_eq!(
        windows::token(Step::Add(Change::GamepadDriver)),
        Some("add-gamepad-driver")
    );
}
