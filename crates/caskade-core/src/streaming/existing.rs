//! Someone else's Sunshine on this device (STREAMING-PLAN §3.4, #258). Its
//! ports and mDNS name would collide with Caskade's, so hosting offers to
//! use it instead ("Streaming is already set up on this device. Use it?"):
//! Caskade then installs nothing, asks once for its user name and password
//! (kept in the keychain), and adds and removes only its own apps there.

use crate::emulation::OperatingSystem;
use std::path::PathBuf;

/// The MSI's UpgradeCode.
pub const UPGRADE_CODE: &str = "{512A3D1B-BE16-401B-A0D1-59BBA3942FB8}";
/// Its Windows service.
pub const SERVICE: &str = "SunshineService";
const FLATPAK_APP: &str = "dev.lizardbyte.app.Sunshine";

/// How it was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// Windows: installed from the MSI.
    Msi,
    /// Windows: its service.
    Service,
    /// Linux: a package's `/usr/bin/sunshine`.
    Package,
    Flatpak,
    /// Linux: the AppImage's or package's systemd user unit.
    UserUnit,
    /// macOS: `Sunshine.app`.
    App,
    /// macOS: Homebrew.
    Homebrew,
}

/// A user's own Sunshine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Existing {
    pub found: Vec<Found>,
    /// Its certificate, to pin its API to, when it is where its install
    /// keeps it.
    pub cert: Option<PathBuf>,
}

/// The MSI's UpgradeCode as Windows files it under
/// `Installer\UpgradeCodes`: each GUID group reversed, the last two by byte.
pub fn packed_guid(guid: &str) -> Option<String> {
    let parts: Vec<&str> = guid.trim_matches(['{', '}']).split('-').collect();
    let [a, b, c, d, e] = parts.as_slice() else {
        return None;
    };
    let reversed = |part: &str| part.chars().rev().collect::<String>();
    let swapped = |part: &str| {
        part.as_bytes()
            .chunks(2)
            .map(|pair| pair.iter().rev().map(|&b| b as char).collect::<String>())
            .collect::<String>()
    };
    Some(format!(
        "{}{}{}{}{}",
        reversed(a),
        reversed(b),
        reversed(c),
        swapped(d),
        swapped(e)
    ))
}

/// Where to look: the file system's root and the user's home (both temp
/// folders in tests), and a way to run the system's own tools.
pub struct Probe<'a> {
    pub os: OperatingSystem,
    pub root: PathBuf,
    pub home: PathBuf,
    /// Whether a command succeeds (`sc query`, `reg query`).
    pub runs: &'a dyn Fn(&str, &[&str]) -> bool,
}

impl Probe<'_> {
    fn at(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }

    fn home(&self, path: &str) -> PathBuf {
        self.home.join(path)
    }
}

fn first_file(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|path| path.is_file())
}

/// A user's own Sunshine, if there is one. Caskade's own host is in
/// Caskade's data folder (or its Program Files copy), never at these
/// places.
pub fn detect(probe: &Probe<'_>) -> Option<Existing> {
    let mut found = Vec::new();
    let mut certs = Vec::new();
    let user_config = probe.home(".config/sunshine/credentials/cacert.pem");
    match probe.os {
        OperatingSystem::Windows => {
            let key = format!(
                r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UpgradeCodes\{}",
                packed_guid(UPGRADE_CODE)?
            );
            if (probe.runs)("reg.exe", &["query", &key]) {
                found.push(Found::Msi);
            }
            if (probe.runs)("sc.exe", &["query", SERVICE]) {
                found.push(Found::Service);
            }
            certs.push(probe.at("Program Files/Sunshine/config/credentials/cacert.pem"));
        }
        OperatingSystem::Linux => {
            if probe.at("/usr/bin/sunshine").is_file() {
                found.push(Found::Package);
            }
            let flatpak = [
                probe.at(&format!("/var/lib/flatpak/app/{FLATPAK_APP}")),
                probe.home(&format!(".local/share/flatpak/app/{FLATPAK_APP}")),
            ];
            if flatpak.iter().any(|dir| dir.is_dir()) {
                found.push(Found::Flatpak);
                certs.push(probe.home(&format!(
                    ".var/app/{FLATPAK_APP}/config/sunshine/credentials/cacert.pem"
                )));
            }
            if probe
                .home(&format!(".config/systemd/user/app-{FLATPAK_APP}.service"))
                .is_file()
            {
                found.push(Found::UserUnit);
            }
            certs.push(user_config);
        }
        OperatingSystem::Macos => {
            if probe.at("/Applications/Sunshine.app").is_dir()
                || probe.home("Applications/Sunshine.app").is_dir()
            {
                found.push(Found::App);
            }
            if probe.at("/opt/homebrew/bin/sunshine").is_file()
                || probe.at("/usr/local/bin/sunshine").is_file()
            {
                found.push(Found::Homebrew);
            }
            certs.push(user_config);
        }
    }
    (!found.is_empty()).then(|| Existing {
        found,
        cert: first_file(certs),
    })
}

/// This system's probe.
pub fn detect_here() -> Option<Existing> {
    let runs = |program: &str, args: &[&str]| {
        crate::quiet::command(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    let os = crate::emulation::host_os();
    let root = match os {
        OperatingSystem::Windows => PathBuf::from(r"C:\"),
        _ => PathBuf::from("/"),
    };
    detect(&Probe {
        os,
        root,
        home: std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_default(),
        runs: &runs,
    })
}

#[cfg(test)]
mod tests;
