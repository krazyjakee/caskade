//! Folder listings for the in-app path browser: console mode's stand-in for
//! the native file and folder dialogs, which a pad can't drive (and which
//! Steam Deck's game mode doesn't have).

use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// One folder's contents and the places to jump to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    /// The folder listed, absolute.
    pub path: String,
    /// The folder above it; none at a filesystem root.
    pub parent: Option<String>,
    /// Folders first, then files, each by name ignoring case.
    pub entries: Vec<DirEntry>,
    pub places: Vec<Place>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    /// A folder, or a link to one.
    pub is_dir: bool,
}

/// How long a folder, or the list of places, gets to answer. A network drive
/// whose server has gone blocks every call into it, in the kernel where
/// nothing can cancel it, so the browser mustn't wait on one.
const FOLDER_WAIT: Duration = Duration::from_secs(5);
/// How long one listing's links get to answer, together.
const LINKS_WAIT: Duration = Duration::from_secs(1);

/// What `work` answers within `wait`. Past that it carries on alone, in a
/// thread that ends when the call it's stuck in does.
fn within<T: Send + 'static>(
    wait: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (sent, answer) = mpsc::channel();
    std::thread::Builder::new()
        .name("browse".into())
        .spawn(move || {
            let _ = sent.send(work());
        })
        .ok()?;
    answer.recv_timeout(wait).ok()
}

/// Whether `path` is a folder, following links, by `deadline`. `None` for
/// something broken or that doesn't answer in time.
fn follow(path: PathBuf, deadline: Instant) -> Option<bool> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return None;
    }
    within(left, move || {
        std::fs::metadata(path).map(|metadata| metadata.is_dir())
    })?
    .ok()
}

/// Whether an entry is a folder, asking as little as it can. A mount point
/// is a plain folder in the folder above it, so only a link needs following,
/// and a drive that has gone doesn't hold up the listing it shows in.
fn is_folder(entry: &std::fs::DirEntry, follow: &dyn Fn(PathBuf) -> Option<bool>) -> Option<bool> {
    let kind = entry.file_type().ok()?;
    if kind.is_symlink() {
        follow(entry.path())
    } else {
        Some(kind.is_dir())
    }
}

/// Home, the filesystem roots and mounted drives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub label: String,
    pub path: String,
}

/// Lists `path` (home without one). Entries that can't be read, or whose
/// names aren't UTF-8, are left out; hidden ones too unless `show_hidden`.
/// A missing or unreadable folder is an error.
pub fn browse_dir(path: Option<&Path>, show_hidden: bool) -> Result<DirListing, String> {
    let dir = match path {
        Some(path) if !path.is_absolute() => {
            return Err(crate::tr!(
                "{path} is not an absolute path",
                path = path.display()
            ));
        }
        Some(path) => normalize(path),
        None => home().ok_or_else(|| crate::tr!("there is no home folder"))?,
    };
    let entries = answered(&dir, FOLDER_WAIT, {
        let dir = dir.clone();
        move || {
            let links = Instant::now() + LINKS_WAIT;
            list(&dir, show_hidden, &|path| follow(path, links))
        }
    })?;
    Ok(DirListing {
        path: text(&dir)?,
        parent: dir
            .parent()
            .and_then(|parent| parent.to_str())
            .map(Into::into),
        entries,
        places: within(FOLDER_WAIT, places).unwrap_or_else(home_place),
    })
}

/// `work`'s result, or that `dir` isn't answering.
fn answered<T: Send + 'static>(
    dir: &Path,
    wait: Duration,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    within(wait, work).unwrap_or_else(|| {
        Err(crate::tr!(
            "{path} isn't answering. If it's a network drive, check that it's reachable.",
            path = dir.display()
        ))
    })
}

fn text(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(Into::into)
        .ok_or_else(|| crate::tr!("{path} is not a UTF-8 path", path = path.display()))
}

fn home() -> Option<PathBuf> {
    directories::UserDirs::new().map(|user| user.home_dir().to_path_buf())
}

/// `path` without `.` and `..`, without touching the disk: the browser goes
/// up by name, not through links.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn list(
    dir: &Path,
    show_hidden: bool,
    follow: &dyn Fn(PathBuf) -> Option<bool>,
) -> Result<Vec<DirEntry>, String> {
    let read = std::fs::read_dir(dir).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => crate::tr!("{path} does not exist", path = dir.display()),
        std::io::ErrorKind::NotADirectory => {
            crate::tr!("{path} is not a folder", path = dir.display())
        }
        _ => crate::tr!(
            "{path} can't be read: {error}",
            path = dir.display(),
            error = error
        ),
    })?;
    let mut entries: Vec<DirEntry> = read
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            if !show_hidden && hidden(&name, &entry) {
                return None;
            }
            // A broken link, or one that doesn't answer, is left out.
            let is_dir = is_folder(&entry, follow)?;
            Some(DirEntry {
                path: entry.path().into_os_string().into_string().ok()?,
                name,
                is_dir,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}

#[cfg(windows)]
fn hidden(name: &str, entry: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    name.starts_with('.')
        || entry
            .metadata()
            .is_ok_and(|metadata| metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
}

#[cfg(not(windows))]
fn hidden(name: &str, _entry: &std::fs::DirEntry) -> bool {
    name.starts_with('.')
}

/// Home, the filesystem root(s), then mounted drives.
pub fn places() -> Vec<Place> {
    let mut places = home_place();
    places.extend(roots());
    for place in mounts() {
        if !places.iter().any(|known| known.path == place.path) {
            places.push(place);
        }
    }
    places
}

/// Home alone: all there is to offer when the drives don't answer.
fn home_place() -> Vec<Place> {
    home()
        .as_deref()
        .and_then(Path::to_str)
        .map(|home| Place {
            label: crate::trp!("folder browser place", "Home"),
            path: home.into(),
        })
        .into_iter()
        .collect()
}

#[cfg(windows)]
fn roots() -> Vec<Place> {
    let links = Instant::now() + LINKS_WAIT;
    (b'A'..=b'Z')
        .map(|letter| format!("{}:\\", letter as char))
        // A mapped drive whose server has gone doesn't answer.
        .filter(|root| follow(root.into(), links) == Some(true))
        .map(|root| Place {
            label: root.trim_end_matches('\\').into(),
            path: root,
        })
        .collect()
}

#[cfg(not(windows))]
fn roots() -> Vec<Place> {
    vec![Place {
        label: crate::tr!("File system"),
        path: "/".into(),
    }]
}

#[cfg(windows)]
fn mounts() -> Vec<Place> {
    Vec::new()
}

#[cfg(target_os = "macos")]
fn mounts() -> Vec<Place> {
    folders_in(Path::new("/Volumes"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn mounts() -> Vec<Place> {
    let user = std::env::var("USER").ok().filter(|user| !user.is_empty());
    let mut found = Vec::new();
    if let Some(user) = &user {
        found.extend(folders_in(&Path::new("/run/media").join(user)));
        found.extend(folders_in(&Path::new("/media").join(user)));
    }
    // `/media/<user>` is listed above for what it holds, not as a drive.
    found.extend(
        folders_in(Path::new("/media"))
            .into_iter()
            .filter(|place| Some(&place.label) != user.as_ref()),
    );
    found.extend(folders_in(Path::new("/mnt")));
    found
}

/// The folders directly in `dir`, by name, as places named after them.
#[cfg(unix)]
fn folders_in(dir: &Path) -> Vec<Place> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let links = Instant::now() + LINKS_WAIT;
    let mut found: Vec<Place> = read
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let label = entry.file_name().into_string().ok()?;
            if label.starts_with('.') || !is_folder(&entry, &|path| follow(path, links))? {
                return None;
            }
            Some(Place {
                label,
                path: entry.path().into_os_string().into_string().ok()?,
            })
        })
        .collect();
    found.sort_by_key(|place| place.label.to_lowercase());
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("beta")).unwrap();
        fs::create_dir(dir.path().join("Alpha")).unwrap();
        fs::create_dir(dir.path().join(".config")).unwrap();
        fs::write(dir.path().join("zeta.iso"), b"").unwrap();
        fs::write(dir.path().join("Setup.exe"), b"").unwrap();
        fs::write(dir.path().join(".hidden"), b"").unwrap();
        dir
    }

    fn names(listing: &DirListing) -> Vec<&str> {
        listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect()
    }

    #[test]
    fn folders_come_first_then_names_ignoring_case() {
        let dir = tree();
        let listing = browse_dir(Some(dir.path()), false).unwrap();
        assert_eq!(names(&listing), ["Alpha", "beta", "Setup.exe", "zeta.iso"]);
        assert_eq!(listing.path, dir.path().to_str().unwrap());
        assert_eq!(
            listing.parent.as_deref(),
            dir.path().parent().and_then(Path::to_str)
        );
        let alpha = &listing.entries[0];
        assert!(alpha.is_dir);
        assert_eq!(Path::new(&alpha.path), dir.path().join("Alpha"));
        assert!(!listing.entries[2].is_dir);
    }

    #[test]
    fn hidden_entries_show_only_when_asked() {
        let dir = tree();
        let listing = browse_dir(Some(dir.path()), true).unwrap();
        assert_eq!(
            names(&listing),
            [
                ".config",
                "Alpha",
                "beta",
                ".hidden",
                "Setup.exe",
                "zeta.iso"
            ]
        );
    }

    #[test]
    fn only_links_are_followed_so_a_gone_drive_holds_nothing_up() {
        let dir = tree();
        let asked = std::sync::Mutex::new(Vec::new());
        let follow = |path: PathBuf| {
            asked.lock().unwrap().push(path);
            None
        };
        #[cfg(unix)]
        std::os::unix::fs::symlink("/nowhere", dir.path().join("away")).unwrap();
        let entries = list(dir.path(), false, &follow).unwrap();
        // The link that didn't answer is left out and the rest are there.
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "beta", "Setup.exe", "zeta.iso"]);
        #[cfg(unix)]
        assert_eq!(*asked.lock().unwrap(), [dir.path().join("away")]);
    }

    /// The call that never comes back, as into a drive whose server has gone.
    fn stuck() -> (std::sync::mpsc::Sender<()>, impl FnOnce() + Send + 'static) {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        (release, move || {
            let _ = wait.recv();
        })
    }

    #[test]
    fn a_folder_that_doesnt_answer_says_so_and_nothing_waits_for_it() {
        let (_release, block) = stuck();
        let started = Instant::now();
        let error = answered::<()>(
            Path::new("/mnt/nas"),
            Duration::from_millis(50),
            move || {
                block();
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.contains("/mnt/nas"), "{error}");
        assert!(error.contains("isn't answering"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(4), "it waited");
    }

    #[test]
    fn a_link_gets_only_what_is_left_of_the_time() {
        let dir = tree();
        let started = Instant::now();
        assert_eq!(follow(dir.path().join("Alpha"), started), None);
        let soon = Instant::now() + Duration::from_secs(4);
        assert_eq!(follow(dir.path().join("Alpha"), soon), Some(true));
        assert_eq!(follow(dir.path().join("zeta.iso"), soon), Some(false));
        assert_eq!(follow(dir.path().join("nothing"), soon), None);
    }

    #[test]
    fn dot_dot_goes_up_by_name() {
        let dir = tree();
        let listing = browse_dir(Some(&dir.path().join("Alpha/../beta/.")), false).unwrap();
        assert_eq!(Path::new(&listing.path), dir.path().join("beta"));
        assert!(listing.entries.is_empty());
    }

    #[test]
    fn missing_files_and_relative_paths_are_errors() {
        let dir = tree();
        let missing = browse_dir(Some(&dir.path().join("gone")), false).unwrap_err();
        assert!(missing.contains("does not exist"), "{missing}");
        assert!(browse_dir(Some(&dir.path().join("zeta.iso")), false).is_err());
        let relative = browse_dir(Some(Path::new("games")), false).unwrap_err();
        assert!(relative.contains("not an absolute path"), "{relative}");
    }

    #[cfg(unix)]
    #[test]
    fn broken_links_are_left_out_and_folder_links_are_folders() {
        let dir = tree();
        std::os::unix::fs::symlink(dir.path().join("nowhere"), dir.path().join("broken")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("Alpha"), dir.path().join("link")).unwrap();
        let listing = browse_dir(Some(dir.path()), false).unwrap();
        assert_eq!(
            names(&listing),
            ["Alpha", "beta", "link", "Setup.exe", "zeta.iso"]
        );
        assert!(listing.entries[2].is_dir);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_folders_are_errors() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tree();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let result = browse_dir(Some(&locked), false);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        // Root reads anything.
        if let Err(error) = result {
            assert!(error.contains("can't be read"), "{error}");
        }
    }

    #[test]
    fn places_start_with_home_and_a_root() {
        let places = places();
        assert_eq!(
            places.first().map(|place| place.label.as_str()),
            Some("Home")
        );
        assert!(
            places
                .iter()
                .any(|place| Path::new(&place.path).parent().is_none())
        );
    }

    #[cfg(unix)]
    #[test]
    fn mount_folders_are_listed_by_name() {
        let dir = tree();
        let found = folders_in(dir.path());
        let labels: Vec<_> = found.iter().map(|place| place.label.as_str()).collect();
        assert_eq!(labels, ["Alpha", "beta"]);
        assert!(folders_in(&dir.path().join("gone")).is_empty());
    }

    #[test]
    fn no_path_lists_home() {
        let Some(home) = home() else { return };
        let listing = browse_dir(None, false).unwrap();
        assert_eq!(Path::new(&listing.path), home);
    }
}
