//! Generated synthetic emulation fixtures (#114), shared by detection, scanner
//! and launch tests. Never reads live user data or invokes a real emulator.
mod emulator;
mod images;
mod machine;
mod roms;

#[doc(hidden)]
pub use emulator::run as run_fake_emulator;
pub use emulator::{FakeEmulator, Invocation};
pub use machine::{FixtureOs, MachineFixture};
pub use roms::{ExpectedGame, RomFixture, RomTree, SCANNER_EXCLUSIONS};

use std::{fs, io, path::Path};

#[derive(Debug, serde::Serialize)]
pub struct ValidationReport {
    pub machines: usize,
    pub rom_files: usize,
    pub rom_groups: usize,
    pub rom_paths: usize,
    pub claimed_tracks: usize,
    pub excluded_files: usize,
    pub generated_bytes: u64,
}

/// Read-only with respect to the checkout: generate, validate and discard all
/// payloads in TempDirs. No downloads, emulator tools or copyrighted data.
pub fn validate() -> io::Result<ValidationReport> {
    let roms = RomFixture::thousand()?;
    roms.tree.validate()?;
    let mut bytes = inventory_bytes(&roms.tree.root)?;
    for os in [FixtureOs::Linux, FixtureOs::Windows, FixtureOs::Macos] {
        let fixture = MachineFixture::new(os)?;
        bytes += inventory_bytes(fixture.path())?;
    }
    if bytes > 16 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "generated emulation fixtures exceed 16 MiB",
        ));
    }
    Ok(ValidationReport {
        machines: 3,
        rom_files: roms.tree.files.len(),
        rom_groups: roms.tree.expected_games.len(),
        rom_paths: roms.tree.expected_discs(),
        claimed_tracks: roms.tree.claimed.len(),
        excluded_files: roms.tree.excluded.len(),
        generated_bytes: bytes,
    })
}

fn inventory_bytes(root: &Path) -> io::Result<u64> {
    let mut pending = vec![root.to_path_buf()];
    let mut bytes = 0;
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if entry.file_type()?.is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "fixture symlinks are not allowed",
                ));
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                if metadata.len() > 3 * 1024 * 1024 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "generated file exceeds 3 MiB",
                    ));
                }
                bytes += metadata.len();
            }
        }
    }
    Ok(bytes)
}

pub(crate) fn write(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)
}

pub(crate) fn executable(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}
