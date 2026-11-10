// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `--native`: the native app (#110), which the bundles carry beside this
    // one, from inside the same install (an AppImage only starts this one).
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--native")
    {
        std::process::exit(start_native());
    }
    // Before any thread exists, so none keeps an arena of its own.
    caskade_malloc::limit_arenas();
    caskade_malloc::trim_every(std::time::Duration::from_secs(60));
    caskade_desktop::run();
}

/// Starts the `caskade-native` beside this executable with the arguments
/// after `--native`: in its place on Unix, as a process of its own on Windows.
fn start_native() -> i32 {
    let Some(native) = std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name(format!("caskade-native{}", std::env::consts::EXE_SUFFIX)))
    else {
        return 1;
    };
    let mut command = std::process::Command::new(&native);
    command.args(std::env::args_os().skip(2));
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
