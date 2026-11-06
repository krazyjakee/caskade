//! A very small Windows game with saves, for the tests that run a real one
//! (`tests/proton_real.rs` under Proton, `tests/windows_saves.rs` on Windows).
//! It's a program of its own, built by `rustc`, never by Cargo.
//!
//! Each run asks Windows where its folders are, as a game does, reads the
//! saves there, writes the next ones, and appends what it saw to `ran.log`
//! beside itself. `CASKADE_PROOF_FOLDER` names its save folders.

use std::ffi::{c_void, OsString};
use std::fs;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::{env, thread, time::Duration};

const APP_DATA: i32 = 0x1a;
const LOCAL_APP_DATA: i32 = 0x1c;
const DOCUMENTS: i32 = 0x05;
const PROFILE: i32 = 0x28;

#[link(name = "shell32")]
extern "system" {
    fn SHGetFolderPathW(
        owner: *mut c_void,
        folder: i32,
        token: *mut c_void,
        flags: u32,
        path: *mut u16,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

fn folder(id: i32) -> PathBuf {
    let mut path = [0u16; 260];
    let result = unsafe {
        SHGetFolderPathW(
            std::ptr::null_mut(),
            id,
            std::ptr::null_mut(),
            0,
            path.as_mut_ptr(),
        )
    };
    assert_eq!(result, 0, "SHGetFolderPathW({id})");
    let length = path.iter().position(|unit| *unit == 0).unwrap_or(260);
    OsString::from_wide(&path[..length]).into()
}

/// Wine's version; `None` on Windows, whose ntdll has no such function.
fn wine() -> Option<String> {
    unsafe {
        let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr().cast());
        let function = GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast());
        if function.is_null() {
            return None;
        }
        let function: extern "C" fn() -> *const std::ffi::c_char = std::mem::transmute(function);
        Some(
            std::ffi::CStr::from_ptr(function())
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn main() {
    let name = env::var("CASKADE_PROOF_FOLDER").unwrap_or_else(|_| "CaskadeProof".into());
    let app_data = folder(APP_DATA).join(&name);
    let local = folder(LOCAL_APP_DATA).join(&name);
    let documents = folder(DOCUMENTS).join(&name);
    let saved_games = folder(PROFILE).join("Saved Games").join(&name);

    let progress = fs::read_to_string(app_data.join("progress.txt")).unwrap_or_default();
    let slot = fs::read_to_string(documents.join("slot1.sav")).unwrap_or_default();
    let settings = fs::read_to_string(local.join("settings.ini")).unwrap_or_default();
    let trophies = fs::read_to_string(saved_games.join("trophies.dat")).unwrap_or_default();

    let level: u32 = progress
        .split_whitespace()
        .find_map(|word| word.strip_prefix("level="))
        .and_then(|level| level.parse().ok())
        .unwrap_or(0)
        + 1;
    let wine = wine();
    let by = if wine.is_some() { "proton" } else { "windows" };
    for (dir, file, text) in [
        (&app_data, "progress.txt", format!("level={level} by={by}")),
        (&documents, "slot1.sav", format!("slot level={level} by={by}")),
        (&local, "settings.ini", format!("[game]\nby={by}\n")),
        (&saved_games, "trophies.dat", format!("trophies={level} by={by}")),
    ] {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(file), text).unwrap();
    }

    // Games open their data relative to where they were started.
    let data = match fs::metadata(r"Content\level1.pak") {
        Ok(metadata) => metadata.len().to_string(),
        Err(error) => format!("{error}"),
    };
    let exe = env::current_exe().unwrap();
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(exe.with_file_name("ran.log"))
        .unwrap();
    writeln!(
        log,
        "run wine={} exe={} cwd={} data={data} appdata={} documents={}\nread progress=[{progress}] slot=[{slot}] settings=[{settings}] trophies=[{trophies}]\nwrote level={level} by={by}",
        wine.as_deref().unwrap_or("none"),
        exe.display(),
        env::current_dir().unwrap().display(),
        app_data.display(),
        documents.display(),
    )
    .unwrap();

    // Long enough for process tracking to see it, as it would a game.
    thread::sleep(Duration::from_secs(3));
}
