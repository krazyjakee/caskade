//! Read-only path identity in the captured host/fixture namespace.
//! Canonical keys are comparison evidence, never execution capabilities.
use crate::{GameRuntime, HostEnv, OperatingSystem};
use caskade_installer::{
    relocate::plain_windows,
    scan::{self as variants, Encoding},
};
use percent_encoding::percent_decode_str;
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};
use url::Url;

#[derive(Debug, Clone, Default)]
pub struct PathContext {
    /// Explicit prefix drive mapping; absent drives never fall back to the host C:.
    pub prefix: Option<PathBuf>,
    /// Flatpak sandbox identity, for virtual XDG paths beneath the captured home.
    pub flatpak_id: Option<String>,
}
impl PathContext {
    pub fn from_runtime(runtime: &GameRuntime) -> Self {
        Self {
            prefix: match runtime {
                GameRuntime::Windows {
                    prefix: Some(prefix),
                    ..
                } => Some(prefix.path.clone()),
                _ => None,
            },
            flatpak_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalPath {
    pub path: PathBuf,
    pub case_sensitive: bool,
}
impl CanonicalPath {
    pub fn same(&self, other: &Self) -> bool {
        self.components(other) == other.components(self)
    }
    pub(crate) fn key(&self) -> Vec<Vec<u8>> {
        self.components(self)
    }
    /// Component containment, never a textual prefix such as Foo/Foobar.
    pub fn contains(&self, other: &Self) -> bool {
        let parent = self.components(other);
        let child = other.components(self);
        if !child.starts_with(&parent) {
            return false;
        }
        // Share the installer's boundary-safe path matcher for Unicode forms.
        // Non-Unicode paths retain their lossless component comparison above.
        let sensitive = self.case_sensitive || other.case_sensitive;
        if let (Some(a), Some(b)) = (self.path.to_str(), other.path.to_str()) {
            let a = if sensitive {
                a.to_owned()
            } else {
                a.to_lowercase()
            };
            let b = if sensitive {
                b.to_owned()
            } else {
                b.to_lowercase()
            };
            if a.trim_end_matches(['/', '\\']).is_empty() {
                return true;
            }
            return variants::needles(&a, &[])
                .iter()
                .filter(|n| {
                    n.encoding == Encoding::Utf8
                        && n.bytes == a.trim_end_matches(['/', '\\']).as_bytes()
                })
                .any(|n| variants::find(b.as_bytes(), n) == Some(0));
        }
        true
    }
    fn components(&self, other: &Self) -> Vec<Vec<u8>> {
        let insensitive = !self.case_sensitive && !other.case_sensitive;
        self.path
            .components()
            .map(|component| {
                let value = component.as_os_str();
                if insensitive {
                    if let Some(text) = value.to_str() {
                        return text.to_lowercase().into_bytes();
                    }
                    return value
                        .as_encoded_bytes()
                        .iter()
                        .map(u8::to_ascii_lowercase)
                        .collect();
                }
                value.as_encoded_bytes().to_vec()
            })
            .collect()
    }
}

pub fn canonical_path(
    env: &HostEnv,
    path: &Path,
    runtime: &GameRuntime,
) -> io::Result<CanonicalPath> {
    canonical_path_with(env, path, &PathContext::from_runtime(runtime))
}

pub fn canonical_path_with(
    env: &HostEnv,
    path: &Path,
    context: &PathContext,
) -> io::Result<CanonicalPath> {
    let mut path = path.to_path_buf();
    let host_reference = path.to_str().is_some_and(|v| v.starts_with("/run/host/"));
    if let Some(text) = path.to_str() {
        if text.contains('\0') {
            return Err(invalid("NUL in path"));
        }
        let text = file_path(text)?;
        let text = plain_windows(&text);
        let windows = is_windows(&text);
        path = if windows {
            map_windows(env, &text, context)?
        } else if let Some(rest) = text.strip_prefix("/run/host/") {
            env.resolve_path(&Path::new("/").join(rest))?
        } else if text.starts_with('~') || text.contains(['$', '%']) {
            env.expand_path(&text)?
        } else {
            env.resolve_path(Path::new(&text))?
        };
    } else {
        path = env.resolve_path(&path)?;
    }
    let flatpak = context
        .flatpak_id
        .clone()
        .or_else(|| env.var("FLATPAK_ID").and_then(|v| v.into_string().ok()));
    if let Some(app) = flatpak.filter(|_| !host_reference) {
        if app.is_empty() || app.contains(['/', '\\']) || app == "." || app == ".." {
            return Err(invalid("invalid Flatpak identity"));
        }
        for (virtual_dir, actual) in [
            (env.home().join(".config"), "config"),
            (env.home().join(".local/share"), "data"),
            (env.home().join(".cache"), "cache"),
        ] {
            if let Ok(tail) = path.strip_prefix(virtual_dir) {
                path = env.resolve_path(
                    &env.home()
                        .join(".var/app")
                        .join(&app)
                        .join(actual)
                        .join(tail),
                )?;
                break;
            }
        }
    }
    if !path.is_absolute() {
        return Err(invalid("identity requires an absolute path"));
    }
    path = lexical(&path)?;
    // HostEnv confines fixtures, resolves links/junctions, and performs their
    // per-volume case lookup. Live paths additionally canonicalize existing
    // ancestors so missing leaves do not hide a junction/symlink identity.
    path = env.resolve_path(&path)?;
    path = canonical_existing(&path)?;
    if env.is_fixture() && !path.starts_with(env.fs_root()) {
        return Err(invalid("path leaves fixture namespace"));
    }
    let case_sensitive = env
        .volumes()?
        .into_iter()
        .filter(|v| path.starts_with(&v.root))
        .max_by_key(|v| v.root.components().count())
        .map_or(env.os() == OperatingSystem::Linux, |v| v.case_sensitive);
    Ok(CanonicalPath {
        path,
        case_sensitive,
    })
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn is_windows(text: &str) -> bool {
    text.starts_with(r"\\")
        || (text.as_bytes().get(1) == Some(&b':') && text.as_bytes()[0].is_ascii_alphabetic())
}
fn file_path(text: &str) -> io::Result<String> {
    if !text
        .get(..5)
        .is_some_and(|v| v.eq_ignore_ascii_case("file:"))
    {
        return Ok(text.into());
    }
    let uri = Url::parse(text).map_err(|_| invalid("invalid file URI"))?;
    if !uri.username().is_empty()
        || uri.password().is_some()
        || uri.port().is_some()
        || uri.query().is_some()
        || uri.fragment().is_some()
    {
        return Err(invalid("invalid file URI"));
    }
    let path = percent_decode_str(uri.path())
        .decode_utf8()
        .map_err(|_| invalid("file URI is not UTF-8"))?;
    if let Some(host) = uri
        .host_str()
        .filter(|host| !host.eq_ignore_ascii_case("localhost"))
    {
        return Ok(format!(r"\\{}{}", host, path.replace('/', r"\")));
    }
    if path.as_bytes().get(2) == Some(&b':') && path.starts_with('/') {
        return Ok(path[1..].into());
    }
    Ok(path.into_owned())
}
fn map_windows(env: &HostEnv, text: &str, context: &PathContext) -> io::Result<PathBuf> {
    let text = text.replace('/', r"\");
    if !text.starts_with(r"\\") && text.as_bytes().get(1..3) != Some(b":\\") {
        return Err(invalid("drive-relative path"));
    }
    if env.os() != OperatingSystem::Windows
        && (text.starts_with(r"Z:\") || text.starts_with(r"z:\"))
    {
        return env.resolve_path(&Path::new("/").join(text[3..].replace('\\', "/")));
    }
    if let Some(prefix) = &context.prefix {
        let prefix = env.resolve_path(prefix)?;
        let drive = text
            .as_bytes()
            .first()
            .copied()
            .filter(u8::is_ascii_alphabetic)
            .ok_or_else(|| invalid("prefix path requires a drive letter"))?;
        if text.as_bytes().get(1..3) != Some(b":\\") {
            return Err(invalid("drive-relative path"));
        }
        let name = format!("{}:", (drive as char).to_ascii_lowercase());
        let link = prefix.join("dosdevices").join(&name);
        // On a Windows host `c:` parses as a drive, and the join would be the
        // drive itself; NTFS can't hold such a link anyway.
        let ordinary = matches!(
            Path::new(&name).components().next(),
            Some(Component::Normal(_))
        );
        let base = if ordinary && fs::symlink_metadata(&link).is_ok() {
            env.resolve_path(&link)?
        } else if drive.eq_ignore_ascii_case(&b'c') {
            prefix.join("drive_c")
        } else {
            return Err(invalid("unmapped prefix drive"));
        };
        return env.resolve_path(&base.join(text[3..].replace('\\', "/")));
    }
    env.map_windows_path(&text)
}
fn lexical(path: &Path) -> io::Result<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return Err(invalid("path escapes root"));
                }
            }
            _ => result.push(component.as_os_str()),
        }
    }
    Ok(result)
}
fn canonical_existing(path: &Path) -> io::Result<PathBuf> {
    let mut current = path;
    let mut tail = Vec::new();
    loop {
        match dunce::canonicalize(current) {
            Ok(mut root) => {
                for part in tail.iter().rev() {
                    root.push(part);
                }
                return Ok(root);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tail.push(current.file_name().ok_or(error)?);
                current = current
                    .parent()
                    .ok_or_else(|| invalid("no absolute ancestor"))?;
            }
            Err(error) => return Err(error),
        }
    }
}
