//! Small helpers the streaming modules share.

use caskade_core::Result;
use std::path::{Path, PathBuf};

/// Writes `path` through a `.part` file beside it, so a reader never sees
/// half of it.
pub(crate) fn write_atomically(path: &Path, bytes: impl AsRef<[u8]>) -> Result<()> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    std::fs::write(&part, bytes)?;
    std::fs::rename(&part, path)?;
    Ok(())
}

/// Runs `program` with no input or output; whether it ran and succeeded.
pub(crate) fn runs_quietly<S: AsRef<std::ffi::OsStr>>(program: &str, args: &[S]) -> bool {
    caskade_core::quiet::command(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// A device name as a pairing request and `sunshine.conf` carry it:
/// printable, at most 64 characters, never empty.
pub fn device_name(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect::<String>()
        .trim()
        .to_owned();
    if name.is_empty() {
        "Caskade".to_owned()
    } else {
        name
    }
}

#[cfg(feature = "net")]
pub(crate) fn invalid(error: impl std::fmt::Display) -> caskade_core::Error {
    caskade_core::Error::Invalid(error.to_string())
}

/// The trimmed text of the first `<name>` element in `xml`.
#[cfg(feature = "net")]
pub(crate) fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(xml[start..end].trim())
}

/// `address` as the host part of a URL: an IPv6 address in brackets.
#[cfg(feature = "net")]
pub(crate) fn url_host(address: &str) -> String {
    match address.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(ip)) => format!("[{ip}]"),
        _ => address.to_owned(),
    }
}
