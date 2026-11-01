use super::*;
use std::{
    fs,
    io::{self, Write},
    path::Path,
};
/// Injected for full/read-only/noexec/capability-limited volume tests.
pub trait RootProbe: Send + Sync {
    /// The full measurement: may create files and start a process.
    fn probe(&self, path: &Path) -> io::Result<RootCapabilities>;
    /// Brings a cached [`probe`](Self::probe) result up to date without
    /// writing to the volume (free space, mount changes). `None` asks for a
    /// full probe again. The default keeps the cached result.
    fn refresh(&self, _path: &Path, cached: &RootCapabilities) -> Option<RootCapabilities> {
        Some(cached.clone())
    }
}
#[derive(Default)]
pub struct FileSystemProbe;
impl RootProbe for FileSystemProbe {
    fn probe(&self, path: &Path) -> io::Result<RootCapabilities> {
        let mut caps = RootCapabilities {
            free_bytes: fs2::available_space(path).ok(),
            total_bytes: fs2::total_space(path).ok(),
            ..Default::default()
        };
        let mount = mount_info(path);
        caps.filesystem = mount.filesystem;
        caps.volume = mount.volume;
        let directory = path.join(format!(".caskade-probe-{}", crate::model::new_id()));
        if fs::create_dir(&directory).is_err() {
            caps.warnings.push(crate::tr!(
                "This location is read-only or cannot create files."
            ));
            return Ok(caps);
        }
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(directory.clone());
        let test = directory.join("CaseProbe");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&test)?;
        file.write_all(b"#!/bin/sh\nexit 0\n")?;
        file.sync_all()?;
        drop(file);
        caps.writable = true;
        caps.case_sensitive = !directory.join("caseprobe").exists();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&test, fs::Permissions::from_mode(0o700))?;
            // A concurrent fork can briefly inherit the just-closed write
            // descriptor before exec closes CLOEXEC: Linux returns ETXTBSY.
            // Retry only transient launch failures, never permission/noexec.
            for attempt in 0..4 {
                match std::process::Command::new(&test).status() {
                    Ok(status) => {
                        caps.executable = status.success();
                        break;
                    }
                    Err(error)
                        if attempt < 3
                            && (error.kind() == io::ErrorKind::WouldBlock
                                || error.raw_os_error() == Some(26)) =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(error) => {
                        caps.warnings.push(crate::tr!(
                            "Executable capability check failed: {error}",
                            error = error
                        ));
                        break;
                    }
                }
            }
            let link = directory.join("link");
            caps.symlinks =
                std::os::unix::fs::symlink("CaseProbe", &link).is_ok() && fs::read(&link).is_ok();
        }
        #[cfg(windows)]
        {
            caps.executable = true;
            caps.symlinks =
                std::os::windows::fs::symlink_file("CaseProbe", directory.join("link")).is_ok();
        }
        if !caps.executable {
            caps.warnings
                .push(crate::tr!("Executables cannot run from this location."));
        }
        if !caps.symlinks {
            caps.warnings.push(crate::tr!("Symbolic links are unavailable; games with local Proton/Wine prefixes cannot move here."));
        }
        Ok(caps)
    }
    /// Space is read live. The mount table (procfs, no disk access) catches a
    /// different volume at the path or a read-only remount; either one, or a
    /// rw mount where the probe could not write, measures again.
    fn refresh(&self, path: &Path, cached: &RootCapabilities) -> Option<RootCapabilities> {
        let mount = mount_info(path);
        if mount.filesystem != cached.filesystem
            || mount.volume != cached.volume
            || mount.read_only == Some(cached.writable)
        {
            return None;
        }
        let mut caps = cached.clone();
        caps.free_bytes = fs2::available_space(path).ok();
        caps.total_bytes = fs2::total_space(path).ok();
        Some(caps)
    }
}

#[derive(Default)]
struct MountInfo {
    filesystem: Option<String>,
    volume: Option<String>,
    /// `None` where the platform has no cheap answer.
    read_only: Option<bool>,
}
#[cfg(target_os = "linux")]
fn mount_info(path: &Path) -> MountInfo {
    use std::io::Read;
    let Ok(file) = fs::File::open("/proc/self/mountinfo") else {
        return MountInfo::default();
    };
    let mut text = String::new();
    if file
        .take(4 * 1024 * 1024)
        .read_to_string(&mut text)
        .is_err()
    {
        return MountInfo::default();
    }
    let mut best = None;
    for line in text.lines() {
        let Some((before, after)) = line.split_once(" - ") else {
            continue;
        };
        let mut mount_fields = before.split_whitespace().skip(4);
        let (Some(mount), options) = (mount_fields.next(), mount_fields.next()) else {
            continue;
        };
        let mount = mount
            .replace(r"\040", " ")
            .replace(r"\011", "\t")
            .replace(r"\012", "\n")
            .replace(r"\134", r"\");
        let mount = PathBuf::from(mount);
        if !path.starts_with(&mount) {
            continue;
        }
        let mut fields = after.split_whitespace();
        let Some(filesystem) = fields.next() else {
            continue;
        };
        let volume = fields.next().map(str::to_owned);
        let read_only = [options, fields.next()]
            .into_iter()
            .flatten()
            .any(|options| options.split(',').any(|option| option == "ro"));
        if best
            .as_ref()
            .is_none_or(|(length, _)| mount.as_os_str().len() > *length)
        {
            best = Some((
                mount.as_os_str().len(),
                MountInfo {
                    filesystem: Some(filesystem.to_owned()),
                    volume,
                    read_only: Some(read_only),
                },
            ));
        }
    }
    best.map(|(_, info)| info).unwrap_or_default()
}
#[cfg(not(target_os = "linux"))]
fn mount_info(_: &Path) -> MountInfo {
    MountInfo::default()
}
