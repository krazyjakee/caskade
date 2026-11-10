//! Programs that tests write and then run (#118).
//!
//! A test that writes a program with `fs::write` or `fs::copy` holds a write
//! fd on it while it writes. A child that another test thread forks in that
//! moment inherits the fd until it execs, and exec'ing the program then fails
//! with ETXTBSY ("Text file busy"): the kernel refuses to run a file someone
//! has open for writing. Here the only write fd is in a `cp` process of its
//! own, which has exited before the program runs, so no fork in this process
//! can carry one.

use std::path::Path;
use std::process::Command;

/// Copies the program at `from` to `to`, keeping its mode, without this
/// process ever opening `to` for writing.
pub fn copy_program(from: impl AsRef<Path>, to: impl AsRef<Path>) {
    let (from, to) = (from.as_ref(), to.as_ref());
    let status = Command::new("cp")
        .arg(from)
        .arg(to)
        .status()
        .expect("cp runs");
    assert!(
        status.success(),
        "cp {} {}: {status}",
        from.display(),
        to.display()
    );
}

/// Writes `contents` to `to` as a program (mode 0755), the same way: into a
/// scratch file beside it, which never runs, then [`copy_program`].
pub fn write_program(to: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    use std::os::unix::fs::PermissionsExt;
    let to = to.as_ref();
    let name = to.file_name().expect("a file name").to_string_lossy();
    let scratch = to.with_file_name(format!(".{name}.writing"));
    std::fs::write(&scratch, contents).expect("the scratch copy is written");
    copy_program(&scratch, to);
    std::fs::remove_file(&scratch).expect("the scratch copy is removed");
    // chmod opens nothing.
    std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o755))
        .expect("the program is executable");
}
