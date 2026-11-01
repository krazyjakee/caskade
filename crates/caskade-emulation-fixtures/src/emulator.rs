#[cfg(unix)]
use crate::executable;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

/// The recorder only captures these deliberate test values, never arbitrary
/// host environment variables or secrets. The native binary additionally
/// accepts CASKADE_FAKE_ENV_KEYS (comma-separated keys).
pub const ENV_KEYS: &[&str] = &[
    "CASKADE_FAKE_MARKER",
    "CASKADE_FAKE_PHASE",
    "CASKADE_FAKE_CORE",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invocation {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub pid: u32,
}
impl Invocation {
    pub fn read(prefix: &Path) -> io::Result<Self> {
        let strings = |suffix| -> io::Result<Vec<String>> {
            let bytes = fs::read(append(prefix, suffix))?;
            if !bytes.is_empty() && bytes.last() != Some(&0) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "incomplete fake-emulator record",
                ));
            }
            if bytes.is_empty() {
                return Ok(vec![]);
            }
            bytes[..bytes.len() - 1]
                .split(|b| *b == 0)
                .map(|s| {
                    String::from_utf8(s.to_vec())
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
                })
                .collect()
        };
        let cwd = strings(".cwd")?;
        let environment = strings(".env")?;
        if cwd.len() != 1 || environment.len() % 2 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid fake-emulator record",
            ));
        }
        let pid = fs::read_to_string(append(prefix, ".pid"))?
            .trim()
            .parse()
            .map_err(io::Error::other)?;
        Ok(Self {
            argv: strings(".argv")?,
            cwd: PathBuf::from(&cwd[0]),
            env: environment
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| (p[0].clone(), p[1].clone()))
                .collect(),
            pid,
        })
    }
}
pub fn append(prefix: &Path, suffix: &str) -> PathBuf {
    let mut path = prefix.as_os_str().to_os_string();
    path.push(suffix);
    path.into()
}

/// Uses the once-built Cargo binary when CASKADE_TEST_EMULATOR is set.
/// Focused Unix tests can use a generated shell recorder without a compiler,
/// download or real emulator. Windows tests require the Cargo-built binary.
pub struct FakeEmulator {
    path: PathBuf,
    script: bool,
}
impl FakeEmulator {
    pub fn new(directory: &Path) -> io::Result<Self> {
        if let Some(path) = std::env::var_os("CASKADE_TEST_EMULATOR") {
            return Self::from_binary(Path::new(&path));
        }
        #[cfg(unix)]
        {
            let path = directory.join("fake emulator");
            executable(&path, include_bytes!("recorder.sh"))?;
            Ok(Self { path, script: true })
        }
        #[cfg(not(unix))]
        {
            let _ = directory;
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "set CASKADE_TEST_EMULATOR to the Cargo-built fake-emulator binary",
            ))
        }
    }
    pub fn from_binary(binary: &Path) -> io::Result<Self> {
        let path = binary.canonicalize()?;
        if !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fake emulator is not a file",
            ));
        }
        let mut magic = [0u8; 2];
        use std::io::Read;
        let script = fs::File::open(&path)?.read(&mut magic)? == 2 && magic == *b"#!";
        Ok(Self { path, script })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Copy the native recorder to a differently named launcher/game path.
    /// Useful for ProcessName/Folder tracking; never compiles at runtime.
    pub fn copy_to(&self, destination: &Path) -> io::Result<Self> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&self.path, destination)?;
        Self::from_binary(destination)
    }
    pub fn command(&self, record: &Path) -> Command {
        let mut command = if self.script {
            let mut shell = Command::new("/bin/sh");
            shell.arg(&self.path);
            shell
        } else {
            Command::new(&self.path)
        };
        command.env("CASKADE_FAKE_RECORD", record);
        command
    }
}

/// Entry point of the tiny native fake-emulator binary. Configuration comes
/// exclusively from CASKADE_FAKE_* variables; argv remains untouched.
pub fn run() -> io::Result<i32> {
    use std::{env, io::Write, time::Duration};
    let record = env::var_os("CASKADE_FAKE_RECORD").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "CASKADE_FAKE_RECORD is required",
        )
    })?;
    let record = PathBuf::from(record);
    if let Some(parent) = record.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut argv = fs::File::create(append(&record, ".argv"))?;
    for value in env::args().skip(1) {
        argv.write_all(value.as_bytes())?;
        argv.write_all(&[0])?;
    }
    let cwd = env::current_dir()?;
    fs::write(
        append(&record, ".cwd"),
        format!("{}\0", cwd.to_string_lossy()),
    )?;
    let mut keys: Vec<String> = ENV_KEYS.iter().map(|k| (*k).to_owned()).collect();
    if let Ok(extra) = env::var("CASKADE_FAKE_ENV_KEYS") {
        if extra.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "environment allowlist too long",
            ));
        }
        keys.extend(
            extra
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        );
    }
    keys.sort();
    keys.dedup();
    let mut output = fs::File::create(append(&record, ".env"))?;
    for key in keys {
        if let Ok(value) = env::var(&key) {
            output.write_all(key.as_bytes())?;
            output.write_all(&[0])?;
            output.write_all(value.as_bytes())?;
            output.write_all(&[0])?;
        }
    }
    // .pid is published last: a reader seeing it can read the complete record.
    fs::write(append(&record, ".pid"), std::process::id().to_string())?;
    let duration = |name: &str| -> io::Result<Duration> {
        let value = env::var(name)
            .unwrap_or_else(|_| "0".into())
            .parse::<u64>()
            .map_err(io::Error::other)?;
        if value > 30_000 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fake emulator delay exceeds 30 seconds",
            ));
        }
        Ok(Duration::from_millis(value))
    };
    if let Ok(mode) = env::var("CASKADE_FAKE_FORK") {
        if !matches!(mode.as_str(), "wait" | "detach") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "CASKADE_FAKE_FORK must be wait or detach",
            ));
        }
        let program = env::var_os("CASKADE_FAKE_CHILD_PROGRAM")
            .map(PathBuf::from)
            .unwrap_or(env::current_exe()?);
        let mut child = Command::new(program)
            .args(env::args_os().skip(1))
            .env_remove("CASKADE_FAKE_FORK")
            .env_remove("CASKADE_FAKE_CHILD_PROGRAM")
            .env("CASKADE_FAKE_RECORD", append(&record, ".child"))
            .env(
                "CASKADE_FAKE_SLEEP_MS",
                duration("CASKADE_FAKE_CHILD_SLEEP_MS")?
                    .as_millis()
                    .to_string(),
            )
            .env("CASKADE_FAKE_PHASE", "child")
            .spawn()?;
        if mode == "wait" {
            child.wait()?;
        }
        // detach deliberately lets the launcher exit while its child is alive.
    }
    std::thread::sleep(duration("CASKADE_FAKE_SLEEP_MS")?);
    let code = env::var("CASKADE_FAKE_EXIT_CODE")
        .unwrap_or_else(|_| "0".into())
        .parse::<u8>()
        .map_err(io::Error::other)?;
    Ok(i32::from(code))
}
