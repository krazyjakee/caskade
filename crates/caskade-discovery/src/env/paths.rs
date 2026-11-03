use super::{HostEnv, KnownFolder};
use crate::OperatingSystem;
use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Component, Path, PathBuf},
};

fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

impl HostEnv {
    /// Expand captured variables once. Unknown/malformed variables remain
    /// literal; no shell is involved, and non-Unicode values are preserved.
    pub fn expand_vars(&self, input: &str) -> OsString {
        let mut out = OsString::new();
        let mut offset = 0;
        while offset < input.len() {
            let rest = &input[offset..];
            let parsed = if let Some(rest) = rest.strip_prefix('%') {
                rest.find('%').map(|end| (&rest[..end], end + 2))
            } else if let Some(rest) = rest.strip_prefix("${") {
                rest.find('}').map(|end| (&rest[..end], end + 3))
            } else if let Some(rest) = rest.strip_prefix('$') {
                let end = rest
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .unwrap_or(rest.len());
                (end > 0).then_some((&rest[..end], end + 1))
            } else {
                None
            };
            if let Some((name, length)) = parsed
                && let Some(value) = self.var(name)
            {
                out.push(value);
                offset += length;
                continue;
            }
            let ch = rest.chars().next().expect("offset is inside input");
            out.push(ch.to_string());
            offset += ch.len_utf8();
        }
        out
    }
    pub fn expand_path(&self, input: &str) -> io::Result<PathBuf> {
        let expanded = if input == "~" || input.starts_with("~/") || input.starts_with("~\\") {
            let home = self
                .folders
                .get(&KnownFolder::Home)
                .map(|path| path.as_os_str())
                .map(OsStr::to_os_string)
                .or_else(|| {
                    self.var(if self.os == OperatingSystem::Windows {
                        "USERPROFILE"
                    } else {
                        "HOME"
                    })
                })
                .unwrap_or_default();
            let mut path = home;
            if input.len() > 1 {
                path.push(&input[1..]);
            }
            path
        } else {
            self.expand_vars(input)
        };
        self.resolve_path(Path::new(&expanded))
    }
    /// Turn an OS-reported path into a path this process can read. Fixture
    /// absolute paths are rebased; mapped paths are idempotent. Existing links
    /// must resolve inside the fixture root, including intermediate links.
    pub fn resolve_path(&self, path: &Path) -> io::Result<PathBuf> {
        if self.wine_drives.is_some() {
            if let Some(text) = path.to_str()
                && windows_path(text)
            {
                return self.map_windows_path(text);
            }
            // Physical paths this env handed out (known folders, drives).
            if !self.is_fixture() || path.starts_with(&self.fs_root) {
                return Ok(path.into());
            }
            return Err(denied("path escapes fixture root"));
        }
        if !self.is_fixture() {
            return Ok(path.into());
        }
        if path.starts_with(&self.fs_root) {
            return self.lookup(path.to_path_buf());
        }
        if let Some(text) = path.to_str() {
            if text.contains('\0') {
                return Err(invalid("NUL in fixture path"));
            }
            if self.os == OperatingSystem::Windows || windows_path(text) {
                return self.map_windows_path(text);
            }
        }
        let mut relative = PathBuf::new();
        for part in path.components() {
            match part {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(value) => relative.push(value),
                Component::ParentDir => {
                    if !relative.pop() {
                        return Err(denied("path escapes fixture root"));
                    }
                }
                Component::Prefix(_) => return Err(invalid("unrecognised fixture path prefix")),
            }
        }
        self.lookup(self.fs_root.join(relative))
    }
    pub fn map_windows_path(&self, input: &str) -> io::Result<PathBuf> {
        if input.contains('\0') {
            return Err(invalid("NUL in Windows path"));
        }
        let text = input.replace('/', "\\");
        let text = if let Some(tail) = text.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{tail}")
        } else {
            text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
        };
        if text.starts_with(r"\\.\") {
            return Err(invalid("device paths are not game paths"));
        }
        if let Some(drives) = &self.wine_drives {
            return self.map_prefix_path(drives, &text);
        }
        if !self.is_fixture() {
            if self.os == OperatingSystem::Windows {
                return Ok(text.into());
            }
            if let Some(tail) = text
                .strip_prefix("Z:\\")
                .or_else(|| text.strip_prefix("z:\\"))
            {
                return Ok(PathBuf::from("/").join(tail.replace('\\', "/")));
            }
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Windows drive needs an explicit prefix mapping",
            ));
        }
        let (base, tail) = if let Some(unc) = text.strip_prefix(r"\\") {
            let parts: Vec<_> = unc.splitn(3, '\\').collect();
            if parts.len() < 2
                || parts[..2].iter().any(|part| {
                    part.is_empty() || part.contains(':') || matches!(*part, "." | "..")
                })
            {
                return Err(invalid("UNC path requires server and share"));
            }
            (
                self.fs_root.join("unc").join(parts[0]).join(parts[1]),
                parts.get(2).copied().unwrap_or_default().to_owned(),
            )
        } else if text.as_bytes().get(1) == Some(&b':') && text.as_bytes()[0].is_ascii_alphabetic()
        {
            if text.len() > 2 && text.as_bytes().get(2) != Some(&b'\\') {
                return Err(invalid("drive-relative paths are ambiguous"));
            }
            (
                self.fs_root
                    .join("drives")
                    .join(text[..1].to_ascii_lowercase()),
                text[2..].trim_start_matches('\\').to_owned(),
            )
        } else if let Some(tail) = text.strip_prefix('\\') {
            let drive = self
                .var("SystemDrive")
                .and_then(|v| v.to_str().map(str::to_owned))
                .unwrap_or_else(|| "C:".into());
            let letter = drive
                .as_bytes()
                .first()
                .filter(|b| b.is_ascii_alphabetic())
                .copied()
                .ok_or_else(|| invalid("invalid SystemDrive"))?;
            (
                self.fs_root
                    .join("drives")
                    .join((letter as char).to_ascii_lowercase().to_string()),
                tail.to_owned(),
            )
        } else {
            (self.fs_root.clone(), text)
        };
        let mut relative = PathBuf::new();
        for part in tail.split('\\') {
            match part {
                "" | "." => {}
                ".." => {
                    if !relative.pop() {
                        return Err(denied("path escapes fixture volume"));
                    }
                }
                value if value.contains(':') => {
                    return Err(invalid("alternate streams are not fixture paths"));
                }
                value => relative.push(value),
            }
        }
        self.lookup(base.join(relative))
    }
    fn lookup(&self, candidate: PathBuf) -> io::Result<PathBuf> {
        let relative = candidate
            .strip_prefix(&self.fs_root)
            .map_err(|_| denied("mapped path escapes fixture root"))?;
        let case_sensitive = self
            .volumes()?
            .into_iter()
            .filter(|v| candidate.starts_with(&v.root))
            .max_by_key(|v| v.root.components().count())
            .map_or(self.os == OperatingSystem::Linux, |v| v.case_sensitive);
        let mut current = self.fs_root.clone();
        for part in relative.components() {
            let Component::Normal(name) = part else {
                return Err(denied("mapped path is not normalized"));
            };
            let mut next = current.join(name);
            // An insensitive native filesystem must not hide a wrong-case
            // path on an explicitly case-sensitive fixture volume.
            if case_sensitive
                && cfg!(any(windows, target_os = "macos"))
                && fs::symlink_metadata(&next).is_ok()
                && current.is_dir()
            {
                let mut exact = false;
                for entry in fs::read_dir(&current)? {
                    if entry?.file_name() == name {
                        exact = true;
                        break;
                    }
                }
                if !exact {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "case-sensitive fixture path does not exist",
                    ));
                }
            }
            // Use the stored name. A case-insensitive native filesystem
            // finds `next` in any case, so there the folder is always read.
            if !case_sensitive
                && (cfg!(any(windows, target_os = "macos")) || fs::symlink_metadata(&next).is_err())
                && current.is_dir()
            {
                let wanted = name.to_string_lossy().to_lowercase();
                let mut found = None;
                let mut exact = None;
                for (count, entry) in fs::read_dir(&current)?.enumerate() {
                    if count >= 100_000 {
                        return Err(invalid("fixture directory exceeds lookup limit"));
                    }
                    let entry = entry?;
                    if entry.file_name() == name {
                        exact = Some(entry.path());
                    } else if entry.file_name().to_string_lossy().to_lowercase() == wanted {
                        if found.is_some() {
                            return Err(invalid("ambiguous case-insensitive fixture path"));
                        }
                        found = Some(entry.path());
                    }
                }
                if let Some(stored) = exact.or(found) {
                    next = stored;
                }
            }
            match fs::symlink_metadata(&next) {
                Ok(_) => {
                    let canonical = dunce::canonicalize(&next)?;
                    if !canonical.starts_with(&self.fs_root) {
                        return Err(denied("fixture symlink leaves its filesystem root"));
                    }
                    current = canonical;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => current = next,
                Err(error) => return Err(error),
            }
        }
        Ok(current)
    }
}
fn windows_path(text: &str) -> bool {
    text.starts_with(r"\\")
        || (text.as_bytes().get(1) == Some(&b':') && text.as_bytes()[0].is_ascii_alphabetic())
}
