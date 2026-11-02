//! Save paths stored relative to named folders (#239), so a save made on
//! one device restores into the same place on another: `<winAppData>` is
//! `C:/Users/me/AppData/Roaming` on Windows and the Proton prefix's
//! `drive_c/users/steamuser/AppData/Roaming` on Linux.

use std::path::PathBuf;

/// This device's named folders for one release: the manifest's placeholders
/// (`<home>`, `<winDocuments>`, …) or an emulator's variables
/// (`{save_data}`, `{RomDir}`, …), each with its path here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roots {
    roots: Vec<(String, String)>,
    case_sensitive: bool,
}

impl Roots {
    pub fn new(case_sensitive: bool) -> Self {
        Self {
            roots: Vec::new(),
            case_sensitive,
        }
    }

    /// Adds a folder; a relative or empty path is ignored.
    pub fn insert(&mut self, name: impl Into<String>, path: impl Into<String>) {
        let path = normal(&path.into());
        if !is_absolute(&path) {
            return;
        }
        let name = name.into();
        self.roots.retain(|(known, _)| *known != name);
        self.roots.push((name, path));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.roots
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, path)| path.as_str())
    }

    /// `path` as `(root, relative path)` under the deepest folder holding it;
    /// `("", path)` when none does, which only restores on this device.
    pub fn portable(&self, path: &str) -> (String, String) {
        let path = normal(path);
        let fold = |text: &str| {
            if self.case_sensitive {
                text.to_owned()
            } else {
                text.to_ascii_lowercase()
            }
        };
        let folded = fold(&path);
        let mut best: Option<(&str, &str)> = None;
        for (name, root) in &self.roots {
            let root_folded = fold(root.trim_end_matches('/'));
            let under = folded
                .strip_prefix(&root_folded)
                .is_some_and(|rest| rest.starts_with('/'));
            if under && best.is_none_or(|(_, known)| known.len() < root.len()) {
                best = Some((name, root));
            }
        }
        match best {
            Some((name, root)) => (
                name.to_owned(),
                path[root.trim_end_matches('/').len() + 1..].to_owned(),
            ),
            None => (String::new(), path),
        }
    }

    /// Where `(root, relative)` goes on this device; `None` when this
    /// device has no such folder or the path would leave it.
    pub fn place(&self, root: &str, relative: &str) -> Option<PathBuf> {
        let relative = normal(relative);
        if root.is_empty() {
            return is_absolute(&relative).then(|| PathBuf::from(relative));
        }
        if relative.is_empty()
            || is_absolute(&relative)
            || relative
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return None;
        }
        let base = self.get(root)?;
        Some(PathBuf::from(format!(
            "{}/{relative}",
            base.trim_end_matches('/')
        )))
    }
}

fn normal(path: &str) -> String {
    path.replace('\\', "/")
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || (path.as_bytes().get(1) == Some(&b':') && path.len() > 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows() -> Roots {
        let mut roots = Roots::new(false);
        roots.insert("<home>", "C:/Users/me");
        roots.insert("<winAppData>", "C:\\Users\\me\\AppData\\Roaming");
        roots
    }

    #[test]
    fn the_deepest_folder_names_a_path_and_places_it_elsewhere() {
        let (root, path) = windows().portable("C:/users/ME/AppData/Roaming/Game/save1.dat");
        assert_eq!(
            (root.as_str(), path.as_str()),
            ("<winAppData>", "Game/save1.dat")
        );
        let mut proton = Roots::new(true);
        proton.insert(
            "<winAppData>",
            "/home/deck/pfx/drive_c/users/steamuser/AppData/Roaming",
        );
        assert_eq!(
            proton.place(&root, &path).unwrap(),
            PathBuf::from("/home/deck/pfx/drive_c/users/steamuser/AppData/Roaming/Game/save1.dat")
        );
        // Back again, case kept as the file has it.
        let (root, path) = proton
            .portable("/home/deck/pfx/drive_c/users/steamuser/AppData/Roaming/Game/Save2.dat");
        assert_eq!(
            windows().place(&root, &path).unwrap(),
            PathBuf::from("C:/Users/me/AppData/Roaming/Game/Save2.dat")
        );
    }

    #[test]
    fn a_path_outside_every_folder_stays_absolute_and_escapes_are_refused() {
        let roots = windows();
        assert_eq!(
            roots.portable("D:/Saves/slot.sav"),
            (String::new(), "D:/Saves/slot.sav".to_owned())
        );
        // A sibling that only shares a prefix isn't under the folder.
        assert_eq!(roots.portable("C:/Users/meow/x.sav").0, "");
        assert!(roots.place("<home>", "../other/x.sav").is_none());
        assert!(roots.place("<home>", "/etc/passwd").is_none());
        assert!(roots.place("<xdgData>", "Game/x.sav").is_none());
    }
}
