//! Helper programs that show nothing.

use std::ffi::OsStr;
use std::process::Command;

/// `program`, with no console window on Windows. Caskade has no console of its
/// own, so Windows opens one for each console program it starts.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}
