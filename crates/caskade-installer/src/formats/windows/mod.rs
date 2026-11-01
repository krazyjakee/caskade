//! Windows installer detection and strategies, available on every host.

pub mod inno;

pub mod msi;

mod uninstall;
pub use uninstall::uninstall_steps;

pub mod nsis;

pub mod burn;
pub mod generic;
pub mod installshield;
pub mod wise;

use crate::{OperatingSystem, RuntimeSpec, host::HostCaps, host::RunnerKind};

/// Where a Windows installer runs on this host: natively on Windows, else in
/// the default Proton (Linux only) or Wine prefix.
fn installer_runtime(host: &HostCaps) -> Option<RuntimeSpec> {
    match host.os {
        OperatingSystem::Windows => Some(RuntimeSpec::Native),
        OperatingSystem::Linux if host.has_runner(RunnerKind::Proton) => {
            Some(RuntimeSpec::ProtonPrefix {
                id: "default".into(),
            })
        }
        _ if host.has_runner(RunnerKind::Wine) => Some(RuntimeSpec::WinePrefix {
            id: "default".into(),
        }),
        _ => None,
    }
}
