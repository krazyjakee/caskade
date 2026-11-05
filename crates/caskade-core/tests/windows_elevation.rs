//! The Windows administrator step for real (#21, #31): `windows::perform`
//! with stand-in files makes the real service, firewall rules and Program
//! Files folder, and its undo removes those and nothing else. CI's
//! `windows-elevation` job runs it on a throwaway runner, which is an
//! administrator with no UAC, so it proves the commands, not the prompt.

use caskade_core::elevation::{Change, Runner, Step, windows};
use std::{path::Path, process::Command};

const ADD: [&str; 4] = [
    "add-host",
    "add-service",
    "add-firewall-tcp",
    "add-firewall-udp",
];
const REMOVE: [&str; 4] = [
    "remove-firewall-udp",
    "remove-firewall-tcp",
    "remove-service",
    "remove-host",
];
const CHANGES: [Change; 4] = [
    Change::HostProgram,
    Change::Service,
    Change::FirewallTcp,
    Change::FirewallUdp,
];
/// Someone else's, named to catch an undo that matches too much.
const OTHER_SERVICE: &str = "CaskadeStreamingOther";
const OTHER_RULE: &str = "Caskade streaming (TCP) other";

/// Whether a system command worked, and what it said.
fn system(program: &str, args: &[&str]) -> (bool, String) {
    let output = Command::new(program).args(args).output().unwrap();
    let mut said = String::from_utf8_lossy(&output.stdout).into_owned();
    said.push_str(&String::from_utf8_lossy(&output.stderr));
    println!("> {program} {args:?}\n{said}");
    (output.status.success(), said)
}

fn firewall(verb: &str, name: &str, rest: &[&str]) -> (bool, String) {
    let name = format!("name={name}");
    let mut args = vec!["advfirewall", "firewall", verb, "rule", &name];
    args.extend(rest);
    system("netsh.exe", &args)
}

fn perform(steps: &[&str], staging: &Path, manifest: Option<&str>) -> i32 {
    let mut args = vec!["caskade.exe".to_owned(), windows::ELEVATED_ARG.to_owned()];
    args.extend(steps.iter().map(|step| (*step).to_owned()));
    args.extend(["--host".to_owned(), staging.display().to_string()]);
    if let Some(manifest) = manifest {
        args.extend(["--manifest".to_owned(), manifest.to_owned()]);
    }
    windows::perform(&args)
}

/// `perform` hides what the system said, and has undone its steps: runs them
/// again in the open, for the log of a failed run.
fn explain(staging: &Path, manifest: &str, target: &Path) {
    println!(
        "copying the host: {:?}",
        windows::install_host(staging, manifest, target)
    );
    for change in &CHANGES[1..] {
        for (program, args) in windows::commands(Step::Add(*change), target) {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            system(program, &args);
        }
    }
}

#[test]
#[ignore = "changes this machine's services, firewall and Program Files; CASKADE_TEST_NATIVE=1 on a throwaway administrator runner"]
fn the_administrator_step_adds_and_removes_the_real_things_and_nothing_else() {
    // Asserted, not skipped: a skip would read as a pass in the job.
    assert_eq!(
        std::env::var("CASKADE_TEST_NATIVE").as_deref(),
        Ok("1"),
        "this changes the real machine"
    );
    let runner = windows::WindowsRunner::new();
    let present = || CHANGES.map(|change| runner.present(change));
    let target = windows::target().expect("Program Files, from HKLM");
    let caskade = target.ancestors().nth(2).unwrap().to_path_buf();
    assert_eq!(present(), [false; 4], "this machine already hosts");
    assert!(!caskade.exists(), "{} is in use", caskade.display());

    let temp = tempfile::tempdir().unwrap();
    let staging = temp.path().join("data").join("streaming").join("host");
    for (file, text) in [
        ("sunshine.exe", "host"),
        ("tools/sunshinesvc.exe", "svc"),
        ("config/sunshine.conf", "sunshine_name = Den"),
    ] {
        let file = staging.join(file);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let manifest = windows::write_manifest(&staging).unwrap();

    let cmd = r"C:\Windows\System32\cmd.exe";
    let service = ["create", OTHER_SERVICE, "binPath=", cmd, "start=", "demand"];
    assert!(system("sc.exe", &service).0);
    let rule = [
        "dir=in",
        "action=allow",
        "protocol=TCP",
        "localport=48999",
        "enable=no",
    ];
    assert!(firewall("add", OTHER_RULE, &rule).0);

    // On: one elevated process, as behind the prompt.
    let code = perform(&ADD, &staging, Some(&manifest));
    if code != 0 {
        explain(&staging, &manifest, &target);
    }
    assert_eq!(code, 0);
    assert_eq!(present(), [true; 4], "the unelevated look finds them all");

    // The copy is the staged files, in the administrator-only folder.
    let text = |path: &Path| std::fs::read_to_string(path).unwrap();
    assert_eq!(text(&target.join("sunshine.exe")), "host");
    assert_eq!(text(&target.join("tools").join("sunshinesvc.exe")), "svc");
    assert!(target.join("config").join("sunshine.conf").is_file());
    assert!(!target.join(".caskade").exists(), "not Caskade's records");

    // The service: on demand, from that copy, for the interactive user.
    let (_, config) = system("sc.exe", &["qc", windows::SERVICE]);
    let program = target.join("tools").join("sunshinesvc.exe");
    assert!(config.contains("DEMAND_START"), "{config}");
    assert!(config.contains(&*program.to_string_lossy()), "{config}");
    let (_, rights) = system("sc.exe", &["sdshow", windows::SERVICE]);
    assert!(rights.contains(";;;IU)"), "{rights}");

    // The rules: inbound, this program's ports, never a public network.
    let program = target.join("sunshine.exe");
    for (name, protocol, ports) in [
        (windows::RULE_TCP, "TCP", "47984,47989,48010"),
        (windows::RULE_UDP, "UDP", "47998-48000"),
    ] {
        let (_, said) = firewall("show", name, &["verbose"]);
        let all = said.to_lowercase();
        let path = program.to_string_lossy().to_lowercase();
        for fact in [&*path, "domain,private", "allow", ports] {
            assert!(all.contains(fact), "{fact} in {said}");
        }
        assert!(said.contains(protocol), "{protocol} in {said}");
    }

    // Off: every one goes, Caskade's now-empty folders too.
    assert_eq!(perform(&REMOVE, &staging, None), 0);
    assert_eq!(present(), [false; 4], "the undo left something");
    assert!(!caskade.exists(), "{} stayed", caskade.display());

    // And nothing else: not the neighbours, not staging.
    assert!(system("sc.exe", &["query", OTHER_SERVICE]).0);
    assert!(firewall("show", OTHER_RULE, &[]).0);
    assert!(staging.join("sunshine.exe").is_file(), "staging stays");

    // Off after a host that ran: no program file, only its pairings in
    // `kept`. An MSI install of the app, in the `Caskade` folder, stays.
    assert_eq!(perform(&ADD[..1], &staging, Some(&manifest)), 0);
    let state = Path::new("config").join("sunshine_state.json");
    std::fs::write(target.join(&state), "{}").unwrap();
    let app = caskade.join("caskade.exe");
    std::fs::write(&app, "the app").unwrap();
    assert_eq!(perform(&REMOVE[3..], &staging, None), 0);
    let streaming = std::fs::read_dir(target.parent().unwrap()).unwrap();
    let left: Vec<_> = streaming.map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(left, [windows::KEPT], "no program file");
    assert_eq!(text(&windows::kept_dir(&target).join(&state)), "{}");
    assert_eq!(text(&app), "the app");

    std::fs::remove_dir_all(&caskade).unwrap();
    assert!(system("sc.exe", &["delete", OTHER_SERVICE]).0);
    assert!(firewall("delete", OTHER_RULE, &[]).0);
}
