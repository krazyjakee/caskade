//! On macOS a Windows game installs only when Wine is there to launch it
//! (`pipeline::record` refuses it otherwise). Tests that send Windows payloads
//! through the pipeline give macOS a stand-in `wine`: extraction never runs it,
//! and it fails if anything does.
use crate::host::HostCaps;
#[cfg(target_os = "macos")]
use crate::host::{RunnerInfo, RunnerKind};
use std::ffi::OsString;
#[cfg(target_os = "macos")]
use std::path::PathBuf;

#[cfg(target_os = "macos")]
fn stand_in() -> &'static std::path::Path {
    use std::{os::unix::fs::PermissionsExt, sync::OnceLock};
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("caskade-test-wine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("stand-in wine folder");
        let wine = dir.join("wine");
        std::fs::write(
            &wine,
            "#!/bin/sh\necho 'stand-in wine: tests never launch' >&2\nexit 1\n",
        )
        .expect("stand-in wine");
        std::fs::set_permissions(&wine, std::fs::Permissions::from_mode(0o755))
            .expect("stand-in wine mode");
        dir
    })
}

/// `PATH` with the stand-in `wine` first, for running the CLI binary; `None`
/// where the host needs no Wine for Windows payloads.
pub fn windows_runner_path() -> Option<OsString> {
    #[cfg(target_os = "macos")]
    {
        let mut paths = vec![stand_in().to_path_buf()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::join_paths(paths).ok()
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// For `pipeline::Context::host`: on macOS the pipeline's own default host
/// with the stand-in Wine runner; `None` (the default) elsewhere.
pub fn host_for_windows_payloads() -> Option<HostCaps> {
    #[cfg(target_os = "macos")]
    {
        let mut host = crate::pipeline::input::host(None).expect("detect the host");
        if !host.has_runner(RunnerKind::Wine) {
            host.runners.push(RunnerInfo {
                kind: RunnerKind::Wine,
                program: stand_in().join("wine"),
            });
        }
        Some(host)
    }
    #[cfg(not(target_os = "macos"))]
    None
}
