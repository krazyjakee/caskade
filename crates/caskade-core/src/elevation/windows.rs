//! Windows: Caskade itself, started with `runas` (one UAC prompt), runs
//! `caskade --elevated <steps…> --host <staging> [--manifest <sha256>]`
//! ([`perform`]). The steps are names from [`token`]; the staging folder
//! is where Caskade downloaded and checked the host program, which it
//! works out itself and the elevated side checks again.
//!
//! The host runs from an administrator-only folder (decided 2026-11-02,
//! STREAMING-PLAN §8.4): `add-host` copies the staged program and its
//! configuration into `%ProgramFiles%\Caskade\Streaming\host` ([`target`],
//! found in HKLM, never in the environment) and checks every copied file
//! against the manifest Caskade wrote in staging, whose own SHA-256 is the
//! `--manifest` argument. The service runs `tools\sunshinesvc.exe` from
//! there on demand, never at boot, and its security descriptor lets the
//! interactive user start and stop it. The firewall rules let that copy of
//! the host program take the stream's ports.

use super::{Change, Prompt, Request, Runner, Step};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

/// The subcommand an elevated Caskade runs.
pub const ELEVATED_ARG: &str = "--elevated";
const HOST_ARG: &str = "--host";
const MANIFEST_ARG: &str = "--manifest";
/// The manifest of the staged host, relative to the staging folder.
pub const MANIFEST: [&str; 2] = [".caskade", "host-manifest.json"];
pub const SERVICE: &str = "CaskadeStreaming";
const SERVICE_NAME: &str = "Caskade streaming";
pub const RULE_TCP: &str = "Caskade streaming (TCP)";
pub const RULE_UDP: &str = "Caskade streaming (UDP)";
/// The host's own files, which it changes as it runs: paired devices and
/// its identity (`sunshine_state.json`'s `root`), its certificate and key,
/// and the games Caskade published. A new copy of the program keeps them
/// (an update, a repair), and switching hosting off sets them aside in
/// [`KEPT`] beside the host folder, so pairings survive (§7).
pub const RUNTIME: [&str; 3] = [
    "config/sunshine_state.json",
    "config/apps.json",
    "config/credentials",
];
/// Where switching hosting off keeps [`RUNTIME`], beside `host`.
pub const KEPT: &str = "kept";
/// ViGEmBus (§4.5): the last release of the only open-source gamepad
/// driver unmodified Sunshine speaks to, production-signed, pinned here.
pub const GAMEPAD_DRIVER_URL: &str = "https://github.com/nefarius/ViGEmBus/releases/download/v1.22.0/ViGEmBus_1.22.0_x64_x86_arm64.exe";
pub const GAMEPAD_DRIVER_FILE: &str = "ViGEmBus_1.22.0_x64_x86_arm64.exe";
pub const GAMEPAD_DRIVER_SHA256: &str =
    "89220a7865076b342892f98865f3499fb7c4cfd673159e89d352c360fd014c6a";
/// Its service, which is how it is found installed.
pub const GAMEPAD_DRIVER_SERVICE: &str = "ViGEmBus";
/// Silent: no window, no reboot.
pub const GAMEPAD_DRIVER_ARGS: [&str; 3] = ["/exenoui", "/qn", "/norestart"];
/// Where Caskade downloads it, beside the staging folder:
/// `<data>\streaming\drivers`.
pub fn gamepad_driver_download(staging: &Path) -> PathBuf {
    staging
        .parent()
        .unwrap_or(staging)
        .join("drivers")
        .join(GAMEPAD_DRIVER_FILE)
}

/// Installs ViGEmBus: copies the downloaded installer into the
/// administrator-only folder beside `target`, checks it against the pinned
/// SHA-256 there (nobody else can change it after), and runs it silently.
/// `true` when it ran and finished.
pub fn install_gamepad_driver(
    staging: &Path,
    target: &Path,
    run: &mut dyn FnMut(&str, &[String]) -> bool,
) -> bool {
    let drivers = target.parent().map_or_else(
        || target.with_extension("drivers"),
        |parent| parent.join("drivers"),
    );
    let copy = drivers.join(GAMEPAD_DRIVER_FILE);
    let checked = (|| -> Result<bool> {
        std::fs::create_dir_all(&drivers)?;
        std::fs::copy(gamepad_driver_download(staging), &copy)?;
        Ok(sha256_file(&copy)? == GAMEPAD_DRIVER_SHA256)
    })();
    let ran = matches!(checked, Ok(true))
        && run(
            &copy.to_string_lossy(),
            &GAMEPAD_DRIVER_ARGS.map(str::to_owned),
        );
    let _ = std::fs::remove_dir_all(&drivers);
    ran
}

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
        Step::Add(Change::HostProgram) => "add-host",
        Step::Remove(Change::HostProgram) => "remove-host",
        Step::Add(Change::Service) => "add-service",
        Step::Remove(Change::Service) => "remove-service",
        Step::Add(Change::FirewallTcp) => "add-firewall-tcp",
        Step::Remove(Change::FirewallTcp) => "remove-firewall-tcp",
        Step::Add(Change::FirewallUdp) => "add-firewall-udp",
        Step::Remove(Change::FirewallUdp) => "remove-firewall-udp",
        Step::Add(Change::GamepadDriver) => "add-gamepad-driver",
        Step::Remove(Change::GamepadDriver) => "remove-gamepad-driver",
        _ => return None,
    })
}

fn step(token: &str) -> Option<Step> {
    [
        Change::HostProgram,
        Change::Service,
        Change::FirewallTcp,
        Change::FirewallUdp,
        Change::GamepadDriver,
    ]
    .into_iter()
    .flat_map(|change| [Step::Add(change), Step::Remove(change)])
    .find(|step| self::token(*step) == Some(token))
}

/// The staging folder, as the elevated side accepts it: an absolute
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

/// One file of the staged host, as the manifest lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFile {
    /// Relative, with `/` between folders.
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub files: Vec<ManifestFile>,
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Lists every file in the staged host (Caskade's own `.caskade` folder
/// aside) with its SHA-256, writes the list into staging, and returns the
/// list's own SHA-256 for the elevated side. Runs unelevated.
pub fn write_manifest(staging: &Path) -> Result<String> {
    let mut files = Vec::new();
    for relative in crate::managed::unpack::files(staging)? {
        let path = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        files.push(ManifestFile {
            sha256: sha256_file(&staging.join(&relative))?,
            path,
        });
    }
    let bytes = serde_json::to_vec_pretty(&Manifest { files })?;
    let at = staging.join(MANIFEST[0]).join(MANIFEST[1]);
    std::fs::create_dir_all(staging.join(MANIFEST[0]))?;
    std::fs::write(at, &bytes)?;
    Ok(hex(&Sha256::digest(&bytes)))
}

/// Copies the staged host into `target` and checks every copied file
/// against the manifest, whose SHA-256 must be `expected`. The copy goes to
/// a sibling folder first and replaces `target` only once it all checks.
pub fn install_host(staging: &Path, expected: &str, target: &Path) -> Result<()> {
    let refuse = |why: &str| Error::Invalid(format!("the staged host was refused: {why}"));
    let bytes = std::fs::read(staging.join(MANIFEST[0]).join(MANIFEST[1]))?;
    if hex(&Sha256::digest(&bytes)) != expected.to_ascii_lowercase() {
        return Err(refuse("the manifest changed"));
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    let parent = target.parent().ok_or_else(|| refuse("no target folder"))?;
    std::fs::create_dir_all(parent)?;
    let fresh = target.with_extension("new");
    let _ = std::fs::remove_dir_all(&fresh);
    let copied = (|| {
        for file in &manifest.files {
            let relative: PathBuf = file.path.split('/').collect();
            if file.path.is_empty()
                || !relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
            {
                return Err(refuse("a path leaves the folder"));
            }
            let to = fresh.join(&relative);
            std::fs::create_dir_all(to.parent().unwrap_or(&fresh))?;
            std::fs::copy(staging.join(&relative), &to)?;
            // Checked where it now is: nobody but administrators can
            // change it there.
            if sha256_file(&to)? != file.sha256.to_ascii_lowercase() {
                return Err(refuse("a file changed after Caskade checked it"));
            }
        }
        let has = |parts: &[&str]| {
            parts
                .iter()
                .fold(fresh.clone(), |at, part| at.join(part))
                .is_file()
        };
        if !has(&[HOST_EXE]) || !has(&SERVICE_EXE) {
            return Err(refuse("the host program is missing"));
        }
        Ok(())
    })();
    if let Err(error) = copied {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(error);
    }
    // The host's own state comes along: from the copy being replaced, or
    // from where switching hosting off kept it.
    let kept = kept_dir(target);
    let from = if target.join(RUNTIME[0]).is_file() {
        Some(target.to_path_buf())
    } else if kept.is_dir() {
        Some(kept.clone())
    } else {
        None
    };
    if let Some(from) = &from
        && let Err(error) = carry_runtime(from, &fresh)
    {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(error);
    }
    let old = target.with_extension("old");
    let _ = std::fs::remove_dir_all(&old);
    if target.exists() {
        std::fs::rename(target, &old)?;
    }
    if let Err(error) = std::fs::rename(&fresh, target) {
        if old.exists() {
            let _ = std::fs::rename(&old, target);
        }
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(error.into());
    }
    let _ = std::fs::remove_dir_all(&old);
    // In use again: nothing is kept aside any more.
    let _ = std::fs::remove_dir_all(&kept);
    Ok(())
}

/// `<Streaming>\kept`, beside the host folder.
pub fn kept_dir(target: &Path) -> PathBuf {
    target
        .parent()
        .map_or_else(|| target.with_extension(KEPT), |parent| parent.join(KEPT))
}

/// Copies the host's own files ([`RUNTIME`]) from `from` into `to`. The
/// state keeps `from`'s paired devices and identity (`root`) and `to`'s
/// web sign-in, which a repair may have made new.
pub fn carry_runtime(from: &Path, to: &Path) -> Result<()> {
    for relative in RUNTIME {
        let source = from.join(relative);
        let target = to.join(relative);
        if source.is_dir() {
            let _ = std::fs::remove_dir_all(&target);
            copy_tree(&source, &target)?;
        } else if source.is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if relative == RUNTIME[0] && target.is_file() {
                let merged = merge_state(&std::fs::read(&target)?, &std::fs::read(&source)?)?;
                std::fs::write(&target, merged)?;
            } else {
                std::fs::copy(&source, &target)?;
            }
        }
    }
    Ok(())
}

/// The new copy's state (`fresh`: its sign-in) with the old one's paired
/// devices and identity (`old`'s `root`).
pub fn merge_state(fresh: &[u8], old: &[u8]) -> Result<Vec<u8>> {
    let mut fresh: serde_json::Value = serde_json::from_slice(fresh)?;
    let old: serde_json::Value = serde_json::from_slice(old)?;
    if let (Some(fresh), Some(root)) = (fresh.as_object_mut(), old.get("root")) {
        fresh.insert("root".into(), root.clone());
    }
    Ok(serde_json::to_vec_pretty(&fresh)?)
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Whether the installed copy in `target` differs from the staged host in
/// `staging`: a file missing or changed, the host's own files aside. Read
/// without elevation (users may read Program Files).
pub fn host_stale(staging: &Path, target: &Path) -> Result<bool> {
    let runtime = |relative: &Path| {
        let text = relative.to_string_lossy().replace('\\', "/");
        text.starts_with(".caskade/")
            || text.ends_with(".log")
            || text.contains(".log.")
            || RUNTIME
                .iter()
                .any(|own| text == *own || text.starts_with(&format!("{own}/")))
    };
    for relative in crate::managed::unpack::files(staging)? {
        if runtime(&relative) {
            continue;
        }
        let installed = target.join(&relative);
        if !installed.is_file()
            || sha256_file(&installed)? != sha256_file(&staging.join(&relative))?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Removes `target` and the `Streaming` and `Caskade` folders above it when
/// that leaves them empty. The host's own files ([`RUNTIME`]) are kept
/// aside first in [`kept_dir`], still administrator-only, so switching
/// hosting on again needs no new pairing.
pub fn remove_host(target: &Path) -> Result<()> {
    if target.join(RUNTIME[0]).is_file() {
        let kept = kept_dir(target);
        let _ = std::fs::remove_dir_all(&kept);
        carry_runtime(target, &kept)?;
    }
    match std::fs::remove_dir_all(target) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let mut above = target.parent();
    for _ in 0..2 {
        let Some(dir) = above else { break };
        if std::fs::remove_dir(dir).is_err() {
            break;
        }
        above = dir.parent();
    }
    Ok(())
}

/// `%ProgramFiles%\Caskade\Streaming\host`, with Program Files read from
/// HKLM's 64-bit view (an environment variable the caller set could point
/// anywhere).
pub fn target() -> Option<PathBuf> {
    let output = crate::quiet::command("reg.exe")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion",
            "/v",
            "ProgramFilesDir",
            "/reg:64",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    program_files(&String::from_utf8_lossy(&output.stdout))
        .map(|root| root.join("Caskade").join("Streaming").join("host"))
}

/// The value of `ProgramFilesDir` in `reg query` output.
pub fn program_files(output: &str) -> Option<PathBuf> {
    output.lines().find_map(|line| {
        let (name, rest) = line.trim().split_once("REG_SZ")?;
        let path = rest.trim();
        (name.trim() == "ProgramFilesDir" && Path::new(path).is_absolute() && !path.is_empty())
            .then(|| PathBuf::from(path))
    })
}

/// What the elevated side was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elevated {
    pub steps: Vec<Step>,
    /// The staging folder.
    pub host: PathBuf,
    pub manifest: Option<String>,
}

/// The elevated side's arguments: steps, then `--host <staging>` and, to
/// add the host, `--manifest <sha256>`.
pub fn parse(args: &[String]) -> Result<Elevated> {
    let usage = || {
        Error::Invalid(
            "usage: caskade --elevated <step>… --host <folder> [--manifest <sha256>]".into(),
        )
    };
    let at = args
        .iter()
        .position(|arg| arg == ELEVATED_ARG)
        .ok_or_else(usage)?;
    let mut steps = Vec::new();
    let mut host = None;
    let mut manifest = None;
    let mut rest = args[at + 1..].iter();
    while let Some(arg) = rest.next() {
        if arg == HOST_ARG {
            host = Some(PathBuf::from(rest.next().ok_or_else(usage)?));
        } else if arg == MANIFEST_ARG {
            let hash = rest.next().ok_or_else(usage)?;
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(usage());
            }
            manifest = Some(hash.clone());
        } else {
            steps.push(step(arg).ok_or_else(usage)?);
        }
    }
    let adds_host = steps.contains(&Step::Add(Change::HostProgram));
    match (steps.is_empty(), host) {
        (false, Some(host)) if !adds_host || manifest.is_some() => Ok(Elevated {
            steps,
            host,
            manifest,
        }),
        _ => Err(usage()),
    }
}

/// Runs the steps against the administrator-only `target`, with `run` for
/// the system's own commands, undoing the ones that worked when one fails.
/// The process's exit code: 0, 1 (failed and undone) or 64 (refused).
pub fn perform_with(
    args: &[String],
    target: &Path,
    run: &mut dyn FnMut(&str, &[String]) -> bool,
) -> i32 {
    let Ok(request) = parse(args) else {
        return USAGE;
    };
    // Only a staging folder shaped like Caskade's is copied from.
    if request.steps.contains(&Step::Add(Change::HostProgram))
        && checked_host(&request.host).is_err()
    {
        return USAGE;
    }
    let installed = |target: &Path| {
        target.join(HOST_EXE).is_file()
            && target.join(SERVICE_EXE[0]).join(SERVICE_EXE[1]).is_file()
    };
    let apply = |step: Step, run: &mut dyn FnMut(&str, &[String]) -> bool| -> bool {
        match step {
            Step::Add(Change::HostProgram) => {
                let expected = request.manifest.as_deref().unwrap_or_default();
                install_host(&request.host, expected, target).is_ok()
            }
            Step::Remove(Change::HostProgram) => remove_host(target).is_ok(),
            // Best effort: hosting works without pads, and says so.
            Step::Add(Change::GamepadDriver) => {
                if !install_gamepad_driver(&request.host, target, run) {
                    tracing::warn!("the gamepad driver wasn't installed");
                }
                true
            }
            // Never removed: other programs use it.
            Step::Remove(Change::GamepadDriver) => true,
            // The service and rules point only at the administrator-only copy.
            Step::Add(_) if !installed(target) => false,
            _ => commands(step, target)
                .iter()
                .enumerate()
                .all(|(at, (program, args))| {
                    // Stopping a service that isn't running is fine.
                    run(program, args) || (matches!(step, Step::Remove(Change::Service)) && at == 0)
                }),
        }
    };
    let mut done: Vec<Step> = Vec::new();
    for step in request.steps.iter().copied() {
        if !apply(step, run) {
            for done in done.into_iter().rev() {
                if let Step::Add(change) = done {
                    apply(Step::Remove(change), run);
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
    let Some(target) = target() else {
        return USAGE;
    };
    perform_with(args, &target, &mut |program, args| {
        crate::quiet::command(program)
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
    if let Some(manifest) = &request.manifest {
        args.push(MANIFEST_ARG.to_owned());
        args.push(manifest.clone());
    }
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
    crate::quiet::command(program)
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
            Change::HostProgram => target().is_some_and(|target| target.join(HOST_EXE).is_file()),
            Change::Service => quietly("sc.exe", &["query", SERVICE]),
            Change::FirewallTcp => rule(RULE_TCP),
            Change::FirewallUdp => rule(RULE_UDP),
            Change::GamepadDriver => quietly("sc.exe", &["query", GAMEPAD_DRIVER_SERVICE]),
            Change::UdevRules | Change::ModulesLoad | Change::UhidLoaded => false,
        }
    }

    fn run(&self, request: &Request) -> Result<Prompt> {
        let exe = self
            .exe
            .as_deref()
            .ok_or_else(|| Error::Invalid(crate::tr!("Caskade can't find its own program")))?;
        let argv = launcher(exe, request)?;
        let status = crate::quiet::command(&argv[0])
            .args(&argv[1..])
            .stdin(std::process::Stdio::null())
            .status()?;
        prompt(status.code())
    }
}
