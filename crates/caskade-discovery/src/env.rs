//! Captured host facts and injected read-only capabilities (#65).
//! Only `live` inspects the current process environment. Fixture paths, drives
//! and time never inherit facts from the machine running the test.
mod clock;
mod folders;
mod paths;
mod prefix;

pub use clock::{Clock, LiveClock};
pub use folders::KnownFolder;

use crate::{
    OperatingSystem,
    registry::{EmptyRegistry, Registry},
    testing::{FakeClock, FakeDrives},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum VolumeKind {
    Fixed,
    Removable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Volume {
    /// Resolved host path; in fixtures this always lies below `fs_root()`.
    pub root: PathBuf,
    pub kind: VolumeKind,
    pub case_sensitive: bool,
    pub filesystem: Option<String>,
}
pub trait DriveProvider: Send + Sync {
    fn volumes(&self) -> io::Result<Vec<Volume>>;
}

/// Safe OS volume APIs through sysinfo: mount tables on Linux, mounted volumes
/// on macOS, and drive roots on Windows. Never starts a process, and never
/// asks a volume for its free space: `statvfs` on a dead network mount
/// blocks. Network filesystems are left out, as sysinfo already leaves out
/// NFS and CIFS, so nothing that walks the volumes touches them.
#[derive(Debug, Default)]
pub struct LiveDrives;
impl DriveProvider for LiveDrives {
    fn volumes(&self) -> io::Result<Vec<Volume>> {
        let disks =
            sysinfo::Disks::new_with_refreshed_list_specifics(sysinfo::DiskRefreshKind::nothing());
        let mut volumes: Vec<_> = disks
            .list()
            .iter()
            .filter(|disk| !network_filesystem(&disk.file_system().to_string_lossy()))
            .map(|disk| {
                let filesystem = disk.file_system().to_string_lossy().into_owned();
                let case_sensitive = !cfg!(any(windows, target_os = "macos"))
                    && !matches!(
                        filesystem.to_ascii_lowercase().as_str(),
                        "vfat" | "exfat" | "ntfs" | "ntfs3" | "msdos"
                    );
                Volume {
                    root: disk.mount_point().into(),
                    kind: if disk.is_removable() {
                        VolumeKind::Removable
                    } else {
                        VolumeKind::Fixed
                    },
                    case_sensitive,
                    filesystem: (!filesystem.is_empty()).then_some(filesystem),
                }
            })
            .collect();
        volumes.sort_by(|a, b| a.root.cmp(&b.root));
        volumes.dedup_by(|a, b| a.root == b.root);
        Ok(volumes)
    }
}

/// Filesystems served over the network, whose mounts can block every
/// access while the server is gone: FUSE clients (sshfs, rclone, s3fs, …)
/// and kernel network filesystems.
pub fn network_filesystem(filesystem: &str) -> bool {
    let filesystem = filesystem.to_ascii_lowercase();
    let name = filesystem.strip_prefix("fuse.").unwrap_or(&filesystem);
    matches!(
        name,
        "sshfs"
            | "rclone"
            | "s3fs"
            | "gcsfuse"
            | "goofys"
            | "blobfuse"
            | "blobfuse2"
            | "curlftpfs"
            | "davfs"
            | "webdav"
            | "glusterfs"
            | "ceph"
            | "cephfs"
            | "juicefs"
            | "nfs"
            | "nfs4"
            | "cifs"
            | "smb3"
            | "smbfs"
            | "afpfs"
    )
}

/// A volume list read once; see [`HostEnv::with_volume_snapshot`].
struct SnapshotDrives(Result<Vec<Volume>, (io::ErrorKind, String)>);
impl DriveProvider for SnapshotDrives {
    fn volumes(&self) -> io::Result<Vec<Volume>> {
        self.0
            .clone()
            .map_err(|(kind, message)| io::Error::new(kind, message))
    }
}

#[derive(Clone)]
pub struct HostEnv {
    os: OperatingSystem,
    fixture_root: Option<PathBuf>,
    fs_root: PathBuf,
    vars: BTreeMap<OsString, OsString>,
    folders: BTreeMap<KnownFolder, PathBuf>,
    xdg_data_dirs: Vec<PathBuf>,
    clock: Arc<dyn Clock>,
    drives: Arc<dyn DriveProvider>,
    registry: Arc<dyn Registry>,
    /// Set for the Windows view of a Wine prefix: drive letter → physical
    /// directory (`c` → `drive_c`, others from `dosdevices`).
    wine_drives: Option<Arc<BTreeMap<char, PathBuf>>>,
}
impl std::fmt::Debug for HostEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Captured environment values may contain credentials; never format them.
        f.debug_struct("HostEnv")
            .field("os", &self.os)
            .field("fixture_root", &self.fixture_root)
            .finish_non_exhaustive()
    }
}
impl HostEnv {
    pub fn live() -> Self {
        let os = if cfg!(windows) {
            OperatingSystem::Windows
        } else if cfg!(target_os = "macos") {
            OperatingSystem::Macos
        } else {
            OperatingSystem::Linux
        };
        let mut vars: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
        if let Some(base) = directories::BaseDirs::new() {
            vars.entry(if cfg!(windows) { "USERPROFILE" } else { "HOME" }.into())
                .or_insert_with(|| base.home_dir().as_os_str().to_owned());
        }
        let fs_root = if cfg!(windows) {
            let mut drive = vars
                .iter()
                .find(|(name, _)| {
                    name.to_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case("SystemDrive"))
                })
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| "C:".into());
            drive.push("\\");
            PathBuf::from(drive)
        } else {
            PathBuf::from("/")
        };
        let mut env = Self::base(os, None, fs_root, vars);
        env.clock = Arc::new(LiveClock::default());
        env.drives = Arc::new(LiveDrives);
        #[cfg(windows)]
        {
            env.registry = Arc::new(crate::registry::NativeRegistry::default());
        }
        env.populate_folders(&BTreeMap::new())
            .expect("live paths do not require fixture mapping");
        if let Some(base) = directories::BaseDirs::new() {
            env.folders
                .insert(KnownFolder::Home, base.home_dir().into());
            for (folder, path) in [
                (KnownFolder::ConfigHome, base.config_dir()),
                (KnownFolder::DataHome, base.data_dir()),
                (KnownFolder::CacheHome, base.cache_dir()),
            ] {
                env.folders.insert(folder, path.into());
            }
        }
        if let Some(user) = directories::UserDirs::new() {
            for (folder, path) in [
                (KnownFolder::Desktop, user.desktop_dir()),
                (KnownFolder::Documents, user.document_dir()),
                (KnownFolder::Downloads, user.download_dir()),
            ] {
                if let Some(path) = path {
                    env.folders.insert(folder, path.into());
                }
            }
        }
        env
    }

    /// Read `machine.toml` and its `root/` tree. Without metadata the stable
    /// default is Linux, not the OS running this test. No directories are made.
    pub fn fixture(scenario: impl AsRef<Path>) -> io::Result<Self> {
        Self::load_fixture(scenario.as_ref(), None)
    }
    pub fn fixture_for(scenario: impl AsRef<Path>, os: OperatingSystem) -> io::Result<Self> {
        Self::load_fixture(scenario.as_ref(), Some(os))
    }
    fn load_fixture(scenario: &Path, os_override: Option<OperatingSystem>) -> io::Result<Self> {
        let scenario = scenario.canonicalize()?;
        let root = scenario.join("root").canonicalize()?;
        if !root.starts_with(&scenario) || !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture root must be a directory within the scenario",
            ));
        }
        let metadata_path = scenario.join("machine.toml");
        if let Ok(path) = metadata_path.canonicalize()
            && !path.starts_with(&scenario)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture metadata leaves its scenario",
            ));
        }
        let metadata = match fs::File::open(&metadata_path) {
            Ok(file) => {
                let mut bytes = vec![];
                file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
                if bytes.len() > 1024 * 1024 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "machine.toml exceeds 1 MiB",
                    ));
                }
                toml::from_str::<FixtureMetadata>(std::str::from_utf8(&bytes).map_err(invalid)?)
                    .map_err(invalid)?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => FixtureMetadata::default(),
            Err(error) => return Err(error),
        };
        if metadata.schema_version != 1 {
            return Err(invalid("unsupported machine fixture schema version"));
        }
        let os = os_override.unwrap_or(metadata.os);
        let mut env = Self::base(
            os,
            Some(scenario),
            root,
            metadata
                .env
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        );
        env.populate_folders(&metadata.known_folders)?;
        let now = UNIX_EPOCH
            .checked_add(Duration::from_secs(metadata.now_unix_secs))
            .ok_or_else(|| invalid("fixture clock is out of range"))?;
        env.clock = Arc::new(FakeClock::new(now));
        let volumes = if metadata.volumes.is_empty() {
            vec![Volume {
                root: if os == OperatingSystem::Windows {
                    env.map_windows_path("C:\\")?
                } else {
                    env.fs_root.clone()
                },
                kind: VolumeKind::Fixed,
                case_sensitive: os == OperatingSystem::Linux,
                filesystem: None,
            }]
        } else {
            metadata
                .volumes
                .into_iter()
                .map(|volume| {
                    Ok(Volume {
                        root: env.resolve_path(Path::new(&volume.path))?,
                        kind: volume.kind,
                        case_sensitive: volume
                            .case_sensitive
                            .unwrap_or(os == OperatingSystem::Linux),
                        filesystem: volume.filesystem,
                    })
                })
                .collect::<io::Result<Vec<_>>>()?
        };
        env.drives = Arc::new(FakeDrives::new(volumes));
        env.registry = Arc::new(crate::registry::WineRegistry::from_fixture(
            env.fixture_root.as_deref().expect("fixture scenario"),
            crate::registry::Limits::default(),
        )?);
        Ok(env)
    }
    fn base(
        os: OperatingSystem,
        fixture_root: Option<PathBuf>,
        fs_root: PathBuf,
        vars: BTreeMap<OsString, OsString>,
    ) -> Self {
        Self {
            os,
            fixture_root,
            fs_root,
            vars,
            folders: BTreeMap::new(),
            xdg_data_dirs: vec![],
            clock: Arc::new(LiveClock::default()),
            drives: Arc::new(FakeDrives::default()),
            registry: Arc::new(EmptyRegistry),
            wine_drives: None,
        }
    }
    pub fn os(&self) -> OperatingSystem {
        self.os
    }
    pub fn fixture_root(&self) -> Option<&Path> {
        self.fixture_root.as_deref()
    }
    pub fn fs_root(&self) -> &Path {
        &self.fs_root
    }
    pub fn is_fixture(&self) -> bool {
        self.fixture_root.is_some()
    }
    pub fn home(&self) -> &Path {
        self.folders
            .get(&KnownFolder::Home)
            .expect("home is always populated")
    }
    pub fn known_folder(&self, folder: KnownFolder) -> Option<&Path> {
        self.folders.get(&folder).map(PathBuf::as_path)
    }
    pub fn vars(&self) -> &BTreeMap<OsString, OsString> {
        &self.vars
    }
    pub fn var(&self, name: &str) -> Option<OsString> {
        self.vars
            .get(OsStr::new(name))
            .or_else(|| {
                (self.os == OperatingSystem::Windows)
                    .then(|| {
                        self.vars
                            .iter()
                            .find(|(key, _)| {
                                key.to_str()
                                    .is_some_and(|key| key.eq_ignore_ascii_case(name))
                            })
                            .map(|(_, value)| value)
                    })
                    .flatten()
            })
            .cloned()
    }
    pub fn xdg_data_dirs(&self) -> &[PathBuf] {
        &self.xdg_data_dirs
    }
    pub fn now(&self) -> SystemTime {
        self.clock.now()
    }
    pub fn monotonic(&self) -> Duration {
        self.clock.monotonic()
    }
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }
    pub fn volumes(&self) -> io::Result<Vec<Volume>> {
        self.drives.volumes()
    }
    /// This environment with its volume list read now and reused from then
    /// on. A scan or merge asks for the volumes once per game path; the
    /// mount table doesn't change under it in a way that matters.
    pub fn with_volume_snapshot(&self) -> Self {
        let volumes = self
            .drives
            .volumes()
            .map_err(|error| (error.kind(), error.to_string()));
        self.clone().with_drives(Arc::new(SnapshotDrives(volumes)))
    }
    pub fn registry(&self) -> &dyn Registry {
        &*self.registry
    }
    pub fn with_registry(mut self, registry: Arc<dyn Registry>) -> Self {
        self.registry = registry;
        self
    }
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }
    pub fn with_drives(mut self, drives: Arc<dyn DriveProvider>) -> Self {
        self.drives = drives;
        self
    }
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
#[derive(Deserialize)]
#[serde(default)]
struct FixtureMetadata {
    schema_version: u32,
    os: OperatingSystem,
    now_unix_secs: u64,
    env: BTreeMap<String, String>,
    known_folders: BTreeMap<KnownFolder, String>,
    volumes: Vec<FixtureVolume>,
}
impl Default for FixtureMetadata {
    fn default() -> Self {
        Self {
            schema_version: 1,
            os: OperatingSystem::Linux,
            now_unix_secs: 1_700_000_000,
            env: BTreeMap::new(),
            known_folders: BTreeMap::new(),
            volumes: vec![],
        }
    }
}
#[derive(Deserialize)]
struct FixtureVolume {
    path: String,
    #[serde(default = "unknown_volume")]
    kind: VolumeKind,
    #[serde(default)]
    case_sensitive: Option<bool>,
    #[serde(default)]
    filesystem: Option<String>,
}
fn unknown_volume() -> VolumeKind {
    VolumeKind::Unknown
}
