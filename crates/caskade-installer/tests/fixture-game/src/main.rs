//! Small, dependency-free executable used to prove installs and moves work.
use std::{env, fs, path::Path, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fixture-game: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut token = None;
    let mut self_check = false;
    let mut check_z_drive = false;
    let mut name = String::from("FixtureGame");
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--token") => token = Some(args.next().ok_or("--token needs a path")?),
            Some("--self-check") => self_check = true,
            Some("--check-z-drive") => check_z_drive = true,
            Some("--name") => {
                name = args
                    .next()
                    .ok_or("--name needs a value")?
                    .into_string()
                    .map_err(|_| "--name must be UTF-8")?;
                if name.is_empty() || name.contains(['\\', '/']) {
                    return Err("--name must be a non-empty registry key component".into());
                }
            }
            _ => return Err(format!("unknown argument: {}", arg.to_string_lossy())),
        }
    }
    let token = token.ok_or("usage: caskade-fixture-game --token <path> [--self-check] [--name <name>] [--check-z-drive]")?;
    let executable = env::current_exe().map_err(|e| e.to_string())?;
    let directory = executable.parent().ok_or("executable has no parent")?;
    let result = (|| {
        if self_check {
            check_config(directory)?;
            check_registry(directory, &name)?;
        }
        if check_z_drive {
            #[cfg(windows)]
            if fs::read(r"Z:\etc\hostname").is_ok() {
                return Err("sandbox leak: Z:\\etc\\hostname is readable".into());
            }
            #[cfg(not(windows))]
            return Err("--check-z-drive requires Windows/Wine".into());
        }
        Ok(())
    })();
    let contents = match &result {
        Ok(()) if self_check => "ok\n".to_owned(),
        Ok(()) => format!("{}\n", executable.display()),
        Err(error) => format!("mismatch: {error}\n"),
    };
    fs::write(token, contents).map_err(|e| format!("write token: {e}"))?;
    result
}

fn check_config(directory: &Path) -> Result<(), String> {
    let config = fs::read_to_string(directory.join("config.ini"))
        .map_err(|e| format!("read config.ini: {e}"))?;
    let mut paths = config.lines().filter_map(|line| {
        let (key, value) = line.trim().split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("InstallPath")
            .then_some(value.trim())
    });
    let path = paths.next().ok_or("config.ini has no InstallPath")?;
    if paths.next().is_some() {
        return Err("config.ini has duplicate InstallPath values".into());
    }
    check_path(directory, Path::new(path), "config.ini InstallPath")
}

fn check_path(directory: &Path, configured: &Path, label: &str) -> Result<(), String> {
    if !configured.is_absolute() {
        return Err(format!(
            "{label} must be absolute: {}",
            configured.display()
        ));
    }
    // Canonicalization accepts either the actual install path or its stable alias.
    let actual = fs::canonicalize(directory).map_err(|e| e.to_string())?;
    let configured = fs::canonicalize(configured).map_err(|e| format!("{label}: {e}"))?;
    if actual == configured {
        Ok(())
    } else {
        Err(format!(
            "{label} points at {}, expected {}",
            configured.display(),
            actual.display()
        ))
    }
}

#[cfg(not(windows))]
fn check_registry(_directory: &Path, _name: &str) -> Result<(), String> {
    // Native Linux/macOS fixtures do not have a Windows registry.
    Ok(())
}

#[cfg(windows)]
fn check_registry(directory: &Path, name: &str) -> Result<(), String> {
    use std::{
        ffi::{OsStr, OsString, c_void},
        os::windows::ffi::{OsStrExt, OsStringExt},
    };
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: *mut c_void,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            value_type: *mut u32,
            data: *mut c_void,
            data_size: *mut u32,
        ) -> i32;
    }
    let key: Vec<u16> = OsStr::new(&format!(r"Software\CaskadeFixture\{name}"))
        .encode_wide()
        .chain(Some(0))
        .collect();
    let value: Vec<u16> = OsStr::new("InstallPath")
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut size = 0_u32;
    // Sign extension is required for predefined Windows HKEY handles on Win64.
    let hklm = (0x8000_0002_u32 as i32 as isize) as *mut c_void;
    let flags = 0x0000_0002 | 0x0001_0000; // REG_SZ, 64-bit view
    // SAFETY: NUL-terminated UTF-16 inputs are alive; null data queries the size.
    let status = unsafe {
        RegGetValueW(
            hklm,
            key.as_ptr(),
            value.as_ptr(),
            flags,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status != 0 {
        return Err(format!("registry InstallPath query failed: {status}"));
    }
    let mut buffer = vec![0_u16; (size as usize).div_ceil(2)];
    // SAFETY: the buffer has at least `size` writable bytes; inputs remain alive.
    let status = unsafe {
        RegGetValueW(
            hklm,
            key.as_ptr(),
            value.as_ptr(),
            flags,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return Err(format!("registry InstallPath read failed: {status}"));
    }
    buffer.truncate((size as usize) / 2);
    while buffer.last() == Some(&0) {
        buffer.pop();
    }
    check_path(
        directory,
        Path::new(&OsString::from_wide(&buffer)),
        "registry InstallPath",
    )
}
