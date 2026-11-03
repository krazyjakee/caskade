//! Linux: `pkexec /bin/sh -c <SCRIPT> caskade <steps…>`. The script is a
//! constant of this binary, passed as an argument (no file anyone could
//! change between writing and running it), and the steps are names from
//! [`token`]. Anything else makes it exit 64 before changing anything.

use super::{Change, Prompt, Request, Runner, Step};
use crate::{Error, Result};
use std::path::Path;

/// The udev rules (§4.5): uinput and uhid, and the virtual pads Sunshine
/// creates, for the `input` group and the user at the seat.
pub const RULES_FILE: &str = "/etc/udev/rules.d/60-caskade-streaming.rules";
/// Loads uhid at boot.
pub const MODULES_FILE: &str = "/etc/modules-load.d/60-caskade-streaming.conf";

/// The elevated script. `$R` prefixes every path it writes; it is empty
/// here, and only tests set it (to a temp folder) by editing the text.
pub const SCRIPT: &str = r#"set -u
R=""
umask 022
rules="$R/etc/udev/rules.d/60-caskade-streaming.rules"
modules="$R/etc/modules-load.d/60-caskade-streaming.conf"
# Refuse the whole run before any change if a step is unknown.
for step in "$@"; do
  case "$step" in
    add-rules|remove-rules|add-modules|remove-modules|load-uhid|unload-uhid) ;;
    *) echo "unknown step $step" >&2; exit 64 ;;
  esac
done
done=""
rollback() {
  for step in $done; do
    case "$step" in
      add-rules) rm -f "$rules" ;;
      add-modules) rm -f "$modules" ;;
      load-uhid) modprobe -r uhid 2>/dev/null ;;
    esac
  done
}
fail() { echo "$1" >&2; rollback; exit 1; }
reload=0
for step in "$@"; do
  case "$step" in
    add-rules)
      mkdir -p "${rules%/*}" || fail "couldn't make the rules folder"
      cat > "$rules.tmp" <<'RULES' && mv -f "$rules.tmp" "$rules" || fail "couldn't write the udev rules"
# Caskade game streaming: lets the streaming host create virtual input
# devices (Sunshine's own rules as of v2026.914). Added and removed by Caskade.
KERNEL=="uinput", SUBSYSTEM=="misc", OPTIONS+="static_node=uinput", GROUP="input", MODE="0660", TAG+="uaccess"
KERNEL=="uhid", GROUP="input", MODE="0660", TAG+="uaccess"
SUBSYSTEM=="hidraw", KERNEL=="hidraw*", IMPORT{parent}="HID_*"
SUBSYSTEM=="hidraw", KERNEL=="hidraw*", ENV{HID_PHYS}=="libvirtualhid/uhid/*", GROUP="input", MODE="0660", TAG+="uaccess"
SUBSYSTEM=="input", KERNEL=="event*", ATTRS{phys}=="libvirtualhid/uhid/*", GROUP="input", MODE="0660", TAG+="uaccess"
SUBSYSTEM=="hidraw", KERNEL=="hidraw*", ENV{HID_NAME}=="Sunshine (libvirtualhid)*", GROUP="input", MODE="0660", TAG+="uaccess"
SUBSYSTEMS=="input", ATTRS{name}=="Sunshine (libvirtualhid)*", GROUP="input", MODE="0660", TAG+="uaccess"
RULES
      reload=1 ;;
    remove-rules) rm -f "$rules" || fail "couldn't remove the udev rules"; reload=1 ;;
    add-modules)
      mkdir -p "${modules%/*}" || fail "couldn't make the modules folder"
      printf 'uhid\n' > "$modules.tmp" && mv -f "$modules.tmp" "$modules" || fail "couldn't write the module list" ;;
    remove-modules) rm -f "$modules" || fail "couldn't remove the module list" ;;
    load-uhid) modprobe uhid || fail "couldn't load uhid" ;;
    # A module something else uses stays loaded.
    unload-uhid) modprobe -r uhid 2>/dev/null || true ;;
  esac
  done="$step $done"
done
if [ "$reload" = 1 ]; then
  udevadm control --reload-rules 2>/dev/null || true
  udevadm trigger --subsystem-match=misc 2>/dev/null || true
fi
exit 0
"#;

/// The script's name for a step.
pub fn token(step: Step) -> Option<&'static str> {
    Some(match step {
        Step::Add(Change::UdevRules) => "add-rules",
        Step::Remove(Change::UdevRules) => "remove-rules",
        Step::Add(Change::ModulesLoad) => "add-modules",
        Step::Remove(Change::ModulesLoad) => "remove-modules",
        Step::Add(Change::UhidLoaded) => "load-uhid",
        Step::Remove(Change::UhidLoaded) => "unload-uhid",
        _ => return None,
    })
}

/// The whole command line: `pkexec`, then what it runs. Only constants and
/// step names; the request's host folder isn't needed on Linux.
pub fn command(request: &Request) -> Result<Vec<String>> {
    let mut argv = vec![
        "pkexec".to_owned(),
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        SCRIPT.to_owned(),
        "caskade".to_owned(),
    ];
    for step in &request.steps {
        let token = token(*step).ok_or_else(|| {
            Error::Invalid(format!("{step:?} isn't an administrator step on Linux"))
        })?;
        argv.push(token.to_owned());
    }
    Ok(argv)
}

/// How pkexec ended: 126 when the prompt was dismissed, 127 when the
/// password was wrong or the account may not.
pub fn prompt(code: Option<i32>, stderr: &str) -> Result<Prompt> {
    match code {
        Some(0) => Ok(Prompt::Done),
        Some(126) => Ok(Prompt::Cancelled),
        Some(127) => Ok(Prompt::Refused),
        _ => Err(Error::Invalid(crate::tr!(
            "the administrator step failed and was undone: {error}",
            error = stderr.trim().lines().last().unwrap_or("").to_owned()
        ))),
    }
}

/// The real thing: pkexec's prompt, and the files and module it looks at.
pub struct LinuxRunner;

impl Runner for LinuxRunner {
    fn present(&self, change: Change) -> bool {
        match change {
            Change::UdevRules => Path::new(RULES_FILE).is_file(),
            Change::ModulesLoad => Path::new(MODULES_FILE).is_file(),
            // A module built into the kernel shows here too, and is never
            // loaded or unloaded.
            Change::UhidLoaded => Path::new("/sys/module/uhid").exists(),
            Change::HostProgram
            | Change::Service
            | Change::FirewallTcp
            | Change::FirewallUdp
            | Change::GamepadDriver => false,
        }
    }

    fn run(&self, request: &Request) -> Result<Prompt> {
        let argv = command(request)?;
        let output = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| {
                Error::Invalid(crate::tr!(
                    "couldn't ask for an administrator (pkexec): {error}",
                    error = error.to_string()
                ))
            })?;
        prompt(
            output.status.code(),
            &String::from_utf8_lossy(&output.stderr),
        )
    }
}
