//! What this machine lets a host do (§4.4–4.5).

use caskade_core::emulation::OperatingSystem;

/// The categories Windows gives the networks this machine is on, one a
/// line; `None` when it can't say.
fn windows_network_category() -> Option<String> {
    let mut command = caskade_core::quiet::command("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-NetConnectionProfile).NetworkCategory",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let output = command.output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    (output.status.success() && !text.trim().is_empty()).then_some(text)
}

/// Whether any of these networks is one the firewall rules cover.
pub(super) fn network_lets_others_in(categories: &str) -> bool {
    categories
        .lines()
        .any(|category| matches!(category.trim(), "Private" | "DomainAuthenticated"))
}

/// What this machine lets a host do (§4.4–4.5), looked at when it is ready
/// and every [`Timing::look_every`](super::Timing::look_every) while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostChecks {
    /// A screen to capture: a desktop session (Linux), always on Windows.
    pub screen: bool,
    /// Virtual pads: `/dev/uinput` writable (Linux), ViGEmBus running
    /// (Windows).
    pub pads: bool,
    /// Windows lets other devices in: the firewall rules cover private and
    /// domain networks, so a machine only on a public one can't be reached.
    pub reachable: bool,
}

impl HostChecks {
    /// This machine, looked at now.
    pub fn here(os: OperatingSystem) -> Self {
        match os {
            OperatingSystem::Windows => Self {
                screen: true,
                pads: caskade_core::quiet::command("sc.exe")
                    .args(["query", crate::elevation::windows::GAMEPAD_DRIVER_SERVICE])
                    .stdin(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output()
                    .is_ok_and(|out| {
                        out.status.success()
                            && String::from_utf8_lossy(&out.stdout).contains("RUNNING")
                    }),
                reachable: windows_network_category()
                    .is_none_or(|categories| network_lets_others_in(&categories)),
            },
            _ => Self {
                screen: ["DISPLAY", "WAYLAND_DISPLAY"]
                    .iter()
                    .any(|var| std::env::var_os(var).is_some_and(|value| !value.is_empty())),
                pads: std::fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/uinput")
                    .is_ok(),
                reachable: true,
            },
        }
    }

    /// Everything works (tests).
    pub fn fine(_: OperatingSystem) -> Self {
        Self {
            screen: true,
            pads: true,
            reachable: true,
        }
    }

    /// What people are told, when anything is missing.
    pub fn warning(self, os: OperatingSystem) -> Option<String> {
        if !self.reachable {
            return Some(caskade_core::tr!(
                "Other devices can't reach this PC while Windows treats its network as public. Set the network to private in Windows Settings → Network & internet."
            ));
        }
        if !self.screen {
            return Some(caskade_core::tr!(
                "This device has no screen for other devices to see. Connect a monitor or a display dummy plug."
            ));
        }
        if !self.pads {
            return Some(match os {
                OperatingSystem::Windows => caskade_core::tr!(
                    "Controllers on other devices won't work here: the controller driver isn't answering. Repair may fix it."
                ),
                _ => caskade_core::tr!(
                    "Controllers on other devices won't work here until Repair sets them up."
                ),
            });
        }
        None
    }
}
