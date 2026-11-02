//! Windows: Caskade itself, started with `runas` (one UAC prompt), runs
//! `caskade --elevated <steps…> --host <folder>` ([`perform`]). The steps
//! are names from [`token`]; the folder is the host program's, which
//! Caskade works out itself and the elevated side checks again. The
//! service runs `tools\sunshinesvc.exe` on demand, never at boot, and its
//! security descriptor lets the interactive user start and stop it. The
//! firewall rules let the host program take the stream's ports.

use super::{Change, Prompt, Request, Runner, Step};
use crate::{Error, Result};
use std::path::{Component, Path, PathBuf};

/// The subcommand an elevated Caskade runs.
pub const ELEVATED_ARG: &str = "--elevated";
const HOST_ARG: &str = "--host";
pub const SERVICE: &str = "CaskadeStreaming";
const SERVICE_NAME: &str = "Caskade streaming";
pub const RULE_TCP: &str = "Caskade streaming (TCP)";
pub const RULE_UDP: &str = "Caskade streaming (UDP)";
/// The host program, and the service launcher beside it (§4.2).
const HOST_EXE: &str = "sunshine.exe";
const SERVICE_EXE: [&str; 2] = ["tools", "sunshinesvc.exe"];
/// SYSTEM and administrators as Windows gives services; authenticated
/// users may query; the interactive user may also start (RP) and stop
/// (WP) it.
const SERVICE_SDDL: &str = "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCSWLOCRRC;;;AU)(A;;CCLCSWRPWPLOCRRC;;;IU)";
/// ERROR_CANCELLED: the UAC prompt was dismissed.
const CANCELLED: i32 = 1223;
const USAGE: i32 = 64;

/// The subcommand's name for a step.
pub fn token(step: Step) -> Option<&'static str> {
    Some(match step {
        Step::Add(Change::Service) => "add-service",
        Step::Remove(Change::Service) => "remove-service",
        Step::Add(Change::FirewallTcp) => "add-firewall-tcp",
        Step::Remove(Change::FirewallTcp) => "remove-firewall-tcp",
        Step::Add(Change::FirewallUdp) => "add-firewall-udp",
        Step::Remove(Change::FirewallUdp) => "remove-firewall-udp",
        _ => return None,
    })
}

fn step(token: &str) -> Option<Step> {
    [Change::Service, Change::FirewallTcp, Change::FirewallUdp]
        .into_iter()
        .flat_map(|change| [Step::Add(change), Step::Remove(change)])
        .find(|step| self::token(*step) == Some(token))
}

/// The host folder, as the elevated side accepts it: an absolute
/// `…\streaming\host` with the host program and its service launcher.
pub fn checked_host(dir: &Path) -> Result<PathBuf> {
    let parts: Vec<_> = dir.components().collect();
    let shaped = dir.is_absolute()
        && parts
            .iter()
            .all(|part| !matches!(part, Component::ParentDir | Component::CurDir))
        && parts.ends_with(&[
            Component::Normal("streaming".as_ref()),
            Component::Normal("host".as_ref()),
        ]);
    if !shaped
        || !dir.join(HOST_EXE).is_file()
        || !dir.join(SERVICE_EXE[0]).join(SERVICE_EXE[1]).is_file()
    {
        return Err(Error::Invalid(crate::tr!(
            "the streaming host isn't installed where Caskade keeps it"
        )));
    }
    Ok(dir.to_path_buf())
}

/// The commands a step runs, in order: program and arguments.
pub fn commands(step: Step, host: &Path) -> Vec<(&'static str, Vec<String>)> {
    let exe = host.join(HOST_EXE).to_string_lossy().into_owned();
    let rule = |name: &str, protocol: &str, ports: &str| {
        vec![
            "advfirewall".into(),
            "firewall".into(),
            "add".into(),
            "rule".into(),
            format!("name={name}"),
            "dir=in".into(),
            "action=allow".into(),
            format!("protocol={protocol}"),
            format!("localport={ports}"),
            format!("program={exe}"),
            // The stream stays on the local network (§1).
            "profile=private,domain".into(),
            "enable=yes".into(),
        ]
    };
    let delete = |name: &str| {
        vec![
            "advfirewall".into(),
            "firewall".into(),
            "delete".into(),
            "rule".into(),
            format!("name={name}"),
        ]
    };
    match step {
        Step::Add(Change::Service) => vec![
            (
                "sc.exe",
                vec![
                    "create".into(),
                    SERVICE.into(),
                    "binPath=".into(),
                    format!(
                        "\"{}\"",
                        host.join(SERVICE_EXE[0]).join(SERVICE_EXE[1]).display()
                    ),
                    "start=".into(),
                    "demand".into(),
                    "DisplayName=".into(),
                    SERVICE_NAME.into(),
                ],
            ),
            (
                "sc.exe",
                vec!["sdset".into(), SERVICE.into(), SERVICE_SDDL.into()],
            ),
        ],
        Step::Remove(Change::Service) => vec![
            ("sc.exe", vec!["stop".into(), SERVICE.into()]),
            ("sc.exe", vec!["delete".into(), SERVICE.into()]),
        ],
        Step::Add(Change::FirewallTcp) => {
            vec![("netsh.exe", rule(RULE_TCP, "TCP", "47984,47989,48010"))]
        }
        Step::Remove(Change::FirewallTcp) => vec![("netsh.exe", delete(RULE_TCP))],
        Step::Add(Change::FirewallUdp) => vec![("netsh.exe", rule(RULE_UDP, "UDP", "47998-48000"))],
        Step::Remove(Change::FirewallUdp) => vec![("netsh.exe", delete(RULE_UDP))],
        _ => Vec::new(),
    }
}

/// The elevated side's arguments: steps, then `--host <folder>`.
pub fn parse(args: &[String]) -> Result<(Vec<Step>, PathBuf)> {
    let usage = || Error::Invalid("usage: caskade --elevated <step>… --host <folder>".into());
    let at = args
        .iter()
        .position(|arg| arg == ELEVATED_ARG)
        .ok_or_else(usage)?;
    let mut steps = Vec::new();
    let mut host = None;
    let mut rest = args[at + 1..].iter();
    while let Some(arg) = rest.next() {
        if arg == HOST_ARG {
            host = Some(PathBuf::from(rest.next().ok_or_else(usage)?));
        } else {
            steps.push(step(arg).ok_or_else(usage)?);
        }
    }
    match (steps.is_empty(), host) {
        (false, Some(host)) => Ok((steps, host)),
        _ => Err(usage()),
    }
}

/// Runs the steps with `run`, undoing the ones that worked when one fails.
/// The process's exit code: 0, 1 (failed and undone) or 64 (refused).
pub fn perform_with(args: &[String], run: &mut dyn FnMut(&str, &[String]) -> bool) -> i32 {
    let Ok((steps, host)) = parse(args) else {
        return USAGE;
    };
    let adds = steps.iter().any(|step| matches!(step, Step::Add(_)));
    let host = match checked_host(&host) {
        Ok(host) => host,
        // Removing needs no program to point at.
        Err(_) if !adds => host,
        Err(_) => return USAGE,
    };
    let mut done: Vec<Step> = Vec::new();
    for step in steps {
        let ok = commands(step, &host)
            .iter()
            .enumerate()
            .all(|(at, (program, args))| {
                // Stopping a service that isn't running is fine.
                run(program, args) || (matches!(step, Step::Remove(Change::Service)) && at == 0)
            });
        if !ok {
            for done in done.into_iter().rev() {
                if let Step::Add(change) = done {
                    for (program, args) in commands(Step::Remove(change), &host) {
                        run(program, &args);
                    }
                }
            }
            return 1;
        }
        done.push(step);
    }
    0
}

/// `caskade --elevated …`: what the elevated process does.
pub fn perform(args: &[String]) -> i32 {
    perform_with(args, &mut |program, args| {
        std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// One argument quoted for a Windows command line (CommandLineToArgvW).
pub fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    let mut quoted = String::from('"');
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
                quoted.push('"');
                slashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', slashes));
                quoted.push(c);
                slashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', slashes * 2));
    quoted.push('"');
    quoted
}

/// PowerShell starting `exe` elevated with the request's arguments, waiting
/// for it and passing on its exit code; 1223 when the prompt is dismissed.
pub fn launcher(exe: &Path, request: &Request) -> Result<Vec<String>> {
    let mut args = vec![ELEVATED_ARG.to_owned()];
    for step in &request.steps {
        args.push(
            token(*step)
                .ok_or_else(|| {
                    Error::Invalid(format!("{step:?} isn't an administrator step on Windows"))
                })?
                .to_owned(),
        );
    }
    args.push(HOST_ARG.to_owned());
    args.push(request.host_dir.to_string_lossy().into_owned());
    let line = args
        .iter()
        .map(|arg| quote_arg(arg))
        .collect::<Vec<_>>()
        .join(" ");
    // Single-quoted PowerShell strings: only ' needs doubling.
    let literal = |text: &str| format!("'{}'", text.replace('\'', "''"));
    let script = format!(
        "try {{ $p = Start-Process -FilePath {} -ArgumentList {} -Verb RunAs -Wait -PassThru -WindowStyle Hidden; exit $p.ExitCode }} catch {{ exit {CANCELLED} }}",
        literal(&exe.to_string_lossy()),
        literal(&line),
    );
    Ok(vec![
        "powershell.exe".into(),
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-Command".into(),
        script,
    ])
}

/// How the elevated Caskade ended.
pub fn prompt(code: Option<i32>) -> Result<Prompt> {
    match code {
        Some(0) => Ok(Prompt::Done),
        Some(CANCELLED) => Ok(Prompt::Cancelled),
        Some(USAGE) => Err(Error::Invalid(crate::tr!(
            "the streaming host isn't installed where Caskade keeps it"
        ))),
        _ => Err(Error::Invalid(crate::tr!(
            "the administrator step failed and was undone"
        ))),
    }
}

/// The real thing: UAC's prompt, and `sc.exe` / `netsh` to look.
pub struct WindowsRunner {
    exe: Option<PathBuf>,
}

impl WindowsRunner {
    pub fn new() -> Self {
        Self {
            exe: std::env::current_exe().ok(),
        }
    }
}

impl Default for WindowsRunner {
    fn default() -> Self {
        Self::new()
    }
}

fn quietly(program: &str, args: &[&str]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

impl Runner for WindowsRunner {
    fn present(&self, change: Change) -> bool {
        let rule = |name: &str| {
            quietly(
                "netsh.exe",
                &[
                    "advfirewall",
                    "firewall",
                    "show",
                    "rule",
                    &format!("name={name}"),
                ],
            )
        };
        match change {
            Change::Service => quietly("sc.exe", &["query", SERVICE]),
            Change::FirewallTcp => rule(RULE_TCP),
            Change::FirewallUdp => rule(RULE_UDP),
            Change::UdevRules | Change::ModulesLoad | Change::UhidLoaded => false,
        }
    }

    fn run(&self, request: &Request) -> Result<Prompt> {
        let exe = self
            .exe
            .as_deref()
            .ok_or_else(|| Error::Invalid(crate::tr!("Caskade can't find its own program")))?;
        let argv = launcher(exe, request)?;
        let status = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(std::process::Stdio::null())
            .status()?;
        prompt(status.code())
    }
}
