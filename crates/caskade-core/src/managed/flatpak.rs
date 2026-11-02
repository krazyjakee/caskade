//! Programs from Flathub, installed for the user (`flatpak --user`).

use crate::{Error, Result};
use std::path::Path;
use tokio_util::sync::CancellationToken;

/// Runs `flatpak <args>`, ended by `cancel`; the error names `what` failed.
async fn run(
    program: &Path,
    args: &[&str],
    cancel: &CancellationToken,
    failed: impl FnOnce(String) -> String,
) -> Result<()> {
    let mut command = tokio::process::Command::new(program);
    super::outside_own_appimage(command.as_std_mut());
    command
        .args(args)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let child = command.spawn().map_err(|error| {
        Error::Invalid(crate::tr!("couldn't run flatpak: {error}", error = error))
    })?;
    let output = tokio::select! {
        () = cancel.cancelled() => return Err(Error::Invalid(crate::tr!("cancelled"))),
        output = child.wait_with_output() => output?,
    };
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Invalid(failed(message.trim().to_owned())));
    }
    Ok(())
}

/// `flatpak install --user` from Flathub.
pub(crate) async fn install(program: &Path, app: &str, cancel: &CancellationToken) -> Result<()> {
    run(
        program,
        &[
            "install",
            "--user",
            "--noninteractive",
            "-y",
            "flathub",
            app,
        ],
        cancel,
        |error| {
            crate::tr!(
                "flatpak couldn't install {app}: {error}",
                app = app,
                error = error
            )
        },
    )
    .await
}

/// `flatpak uninstall --user`: the app goes, its data in `~/.var/app` stays.
pub(crate) async fn uninstall(program: &Path, app: &str) -> Result<()> {
    run(
        program,
        &["uninstall", "--user", "--noninteractive", "-y", app],
        &CancellationToken::new(),
        |error| {
            crate::tr!(
                "flatpak couldn't remove {app}: {error}",
                app = app,
                error = error
            )
        },
    )
    .await
}

/// Whether the user has `app` installed (`flatpak info --user`).
pub(crate) async fn installed(program: &Path, app: &str) -> bool {
    run(
        program,
        &["info", "--user", app],
        &CancellationToken::new(),
        |error| error,
    )
    .await
    .is_ok()
}
