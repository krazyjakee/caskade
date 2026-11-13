// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Installed shortcuts and AppImages enter here. Native is the default;
    // `--legacy` keeps the Tauri UI available while it is retained.
    // The launch caller must keep this process alive (including its hidden
    // WM_CLOSE window on Windows), and elevation runs only its requested steps.
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(args) = native_args(args) {
        std::process::exit(start_native(args));
    }
    // Before any thread exists, so none keeps an arena of its own.
    caskade_malloc::limit_arenas();
    caskade_malloc::trim_every(std::time::Duration::from_secs(60));
    caskade_desktop::run();
}

/// The native app's arguments, or a request handled by this executable.
/// Shell selectors are recognised only as the first argument, so paths and
/// values belonging to another option are passed through unchanged.
fn native_args(mut args: Vec<std::ffi::OsString>) -> Option<Vec<std::ffi::OsString>> {
    if args.first().is_some_and(|arg| arg == "--legacy")
        || args
            .iter()
            .any(|arg| arg == "--launch" || arg == "--elevated")
    {
        return None;
    }
    // Retain the old opt-in as an alias for the default.
    if args.first().is_some_and(|arg| arg == "--native") {
        args.remove(0);
    }
    Some(args)
}

/// Starts the `caskade-native` beside this executable: in its place on Unix,
/// as a process of its own on Windows. A missing native app is an installation
/// error; it never silently selects another UI.
fn start_native(args: Vec<std::ffi::OsString>) -> i32 {
    let Some(native) = std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name(format!("caskade-native{}", std::env::consts::EXE_SUFFIX)))
    else {
        return 1;
    };
    let mut command = std::process::Command::new(&native);
    command.args(args);
    #[cfg(unix)]
    let error = std::os::unix::process::CommandExt::exec(&mut command);
    #[cfg(not(unix))]
    let error = match command.spawn() {
        Ok(_) => return 0,
        Err(error) => error,
    };
    eprintln!("Caskade couldn't start {}: {error}", native.display());
    1
}

#[cfg(test)]
mod tests {
    use super::native_args;
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn normal_starts_forward_every_argument_to_native() {
        for values in [
            vec![],
            vec!["--console"],
            vec!["--login"],
            vec!["caskade://install?url=https%3A%2F%2Fexample.test%2Fgame.zip"],
            vec!["/a folder/game.torrent"],
            vec!["--restore", "/a folder/backup.zip"],
            vec!["--web-game", "https://example.test/"],
            vec!["--video", "aaaaaaaaaaa"],
        ] {
            assert_eq!(native_args(args(&values)), Some(args(&values)));
        }
    }

    #[test]
    fn legacy_is_explicit_and_native_remains_an_alias() {
        assert_eq!(native_args(args(&["--legacy", "--console"])), None);
        assert_eq!(native_args(args(&["--native"])), Some(vec![]));
        assert_eq!(
            native_args(args(&["--native", "--console"])),
            Some(args(&["--console"]))
        );
    }

    #[test]
    fn callers_and_administrator_steps_keep_the_original_process() {
        for values in [
            vec!["--launch", "game", "--entry", "release"],
            vec!["--elevated", "steps.json"],
        ] {
            assert_eq!(native_args(args(&values)), None);
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_file_names_are_forwarded_losslessly() {
        use std::os::unix::ffi::OsStringExt;
        let file = OsString::from_vec(b"/games/\xff.torrent".to_vec());
        assert_eq!(native_args(vec![file.clone()]), Some(vec![file]));
    }
}
