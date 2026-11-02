use std::{
    collections::BTreeMap,
    io::Cursor,
    path::{Path, PathBuf},
};

use super::{DiscoveryConfig, DiscoveryHints, tree::Tree, warning};
use crate::Warning;

pub(super) struct ExactTarget {
    pub path: PathBuf,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub score: f32,
    pub reason: String,
}
pub(super) struct Found {
    pub titles: Vec<(u8, String)>,
    pub ids: BTreeMap<String, String>,
    pub targets: Vec<ExactTarget>,
    pub icons: Vec<PathBuf>,
    /// Icons read from inside a container (AppImage squashfs), preferred
    /// over loose files. The path labels warnings only.
    pub embedded_icons: Vec<(PathBuf, Vec<u8>)>,
    pub warnings: Vec<Warning>,
}
impl Found {
    pub fn best_title(&self) -> Option<String> {
        self.titles
            .iter()
            .filter(|(_, title)| !title.trim().is_empty())
            .max_by_key(|(rank, _)| rank)
            .map(|(_, title)| title.trim().into())
    }
}

/// A hint that is only a file name, when the tree has no such file at its
/// top: the shallowest file of that name, ignoring case, first by path at
/// that depth. Archives often wrap a release in a folder of its own, and a
/// caller that knows the program's name doesn't know the folder's.
fn by_name(tree: &Tree, hint: &Path) -> Option<PathBuf> {
    let mut components = hint.components();
    let (Some(std::path::Component::Normal(name)), None) = (components.next(), components.next())
    else {
        return None;
    };
    if tree.resolve(hint).is_some() {
        return None;
    }
    let name = name.to_string_lossy().to_lowercase();
    tree.entries
        .iter()
        .filter(|entry| !entry.directory)
        .filter(|entry| {
            entry
                .path
                .file_name()
                .is_some_and(|file| file.to_string_lossy().to_lowercase() == name)
        })
        .min_by(|a, b| {
            a.path
                .components()
                .count()
                .cmp(&b.path.components().count())
                .then_with(|| a.path.cmp(&b.path))
        })
        .map(|entry| entry.path.clone())
}

pub(super) fn scan(tree: &Tree, hints: &DiscoveryHints, config: DiscoveryConfig) -> Found {
    let mut found = Found {
        titles: vec![],
        ids: hints.ids.clone(),
        targets: vec![],
        icons: vec![],
        embedded_icons: vec![],
        warnings: vec![],
    };
    for (rank, title) in [
        (90, &hints.installer_title),
        (80, &hints.package_name),
        (50, &hints.resolver_title),
    ] {
        if let Some(title) = title {
            found.titles.push((rank, title.clone()));
        }
    }
    if let Some(url) = &hints.source_url {
        found.ids.insert("source_url".into(), url.to_string());
    }
    for target in &hints.launch_targets {
        let path = by_name(tree, &target.path).unwrap_or_else(|| target.path.clone());
        let working_dir = target
            .working_dir
            .as_ref()
            .and_then(|path| tree.resolve(path))
            .or_else(|| {
                // A target found by name runs from its own folder.
                (path != target.path)
                    .then(|| path.parent().map(Path::to_path_buf))
                    .flatten()
                    .filter(|parent| !parent.as_os_str().is_empty())
            });
        found.targets.push(ExactTarget {
            path,
            args: target.args.clone(),
            working_dir,
            score: 1100.0,
            reason: target.reason.clone(),
        });
    }
    for entry in &tree.entries {
        if entry.directory {
            continue;
        }
        let name = entry.path.file_name().unwrap_or_default().to_string_lossy();
        let lower = name.to_lowercase();
        #[cfg(feature = "linux-formats")]
        if appimage_candidate(tree, &entry.path, &lower) {
            if let Err(error) = appimage(tree, &entry.path, config, &mut found) {
                found.warnings.push(warning(
                    "invalid_appimage_metadata",
                    &error.to_string(),
                    Some(entry.path.clone()),
                ));
            }
            continue;
        }
        let kind = if lower.starts_with("goggame-") && lower.ends_with(".info") {
            0
        } else if lower == ".itch.toml" {
            1
        } else if lower.ends_with(".desktop") {
            2
        } else if lower == "info.plist"
            && entry
                .path
                .parent()
                .is_some_and(|parent| parent.ends_with("Contents"))
        {
            3
        } else if lower == "steam_appid.txt" {
            4
        } else {
            continue;
        };
        let result =
            tree.read(&entry.path, config.max_metadata_bytes)
                .and_then(|bytes| match kind {
                    0 => gog(tree, &entry.path, &bytes, &mut found),
                    1 => itch(tree, &entry.path, &bytes, &mut found),
                    2 => desktop(tree, &entry.path, &bytes, &mut found),
                    3 => app(tree, &entry.path, &bytes, &mut found),
                    _ => {
                        let id = std::str::from_utf8(&bytes)
                            .map_err(std::io::Error::other)?
                            .trim();
                        if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) {
                            found.ids.insert("steam".into(), id.into());
                        }
                        Ok(())
                    }
                });
        if let Err(error) = result {
            found.warnings.push(warning(
                "invalid_launch_metadata",
                &error.to_string(),
                Some(entry.path.clone()),
            ));
        }
    }
    found
}

fn gog(tree: &Tree, path: &Path, bytes: &[u8], found: &mut Found) -> std::io::Result<()> {
    let data: serde_json::Value = serde_json::from_slice(bytes).map_err(std::io::Error::other)?;
    if let Some(name) = data["name"].as_str() {
        found.titles.push((100, name.into()));
    }
    if let Some(id) = data["gameId"]
        .as_str()
        .or_else(|| data["rootGameId"].as_str())
    {
        found.ids.insert("gog".into(), id.into());
    } else if let Some(name) = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.strip_prefix("goggame-"))
    {
        found.ids.insert("gog".into(), name.into());
    }
    if let Some(tasks) = data["playTasks"].as_array() {
        for task in tasks {
            if task["type"].as_str().is_some_and(|kind| kind != "FileTask")
                || task["category"]
                    .as_str()
                    .is_some_and(|category| category != "game")
            {
                continue;
            }
            let Some(target) = task["path"].as_str() else {
                continue;
            };
            let target = relative_to(path, target);
            let args = task["arguments"]
                .as_str()
                .and_then(|text| split_args(text, true))
                .unwrap_or_default();
            let working_dir = task["workingDir"]
                .as_str()
                .map(|dir| relative_to(path, dir))
                .and_then(|dir| tree.resolve(&dir));
            found.targets.push(ExactTarget {
                path: target,
                args,
                working_dir,
                score: if task["isPrimary"].as_bool() == Some(true) {
                    1050.0
                } else {
                    1000.0
                },
                reason: "GOG playTasks".into(),
            });
        }
    }
    Ok(())
}

fn itch(tree: &Tree, path: &Path, bytes: &[u8], found: &mut Found) -> std::io::Result<()> {
    let text = std::str::from_utf8(bytes).map_err(std::io::Error::other)?;
    let data: toml::Value = toml::from_str(text).map_err(std::io::Error::other)?;
    if let Some(name) = data.get("name").and_then(toml::Value::as_str) {
        found.titles.push((85, name.into()));
    }
    if let Some(actions) = data.get("actions").and_then(toml::Value::as_array) {
        for action in actions {
            let Some(target) = action.get("path").and_then(toml::Value::as_str) else {
                continue;
            };
            let args = match action.get("args") {
                Some(toml::Value::Array(args)) => args
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .map(String::from)
                    .collect(),
                Some(toml::Value::String(args)) => split_args(args, false).unwrap_or_default(),
                _ => vec![],
            };
            let target = relative_to(path, target);
            let working_dir = action
                .get("cwd")
                .and_then(toml::Value::as_str)
                .map(|dir| relative_to(path, dir))
                .and_then(|dir| tree.resolve(&dir));
            found.targets.push(ExactTarget {
                path: target,
                args,
                working_dir,
                score: 1000.0,
                reason: "itch.io manifest action".into(),
            });
        }
    }
    Ok(())
}

fn desktop(tree: &Tree, path: &Path, bytes: &[u8], found: &mut Found) -> std::io::Result<()> {
    let text = std::str::from_utf8(bytes).map_err(std::io::Error::other)?;
    let mut in_entry = false;
    let mut values = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry
            && !line.starts_with('#')
            && let Some((key, value)) = line.split_once('=')
        {
            values.insert(key.trim(), value.trim());
        }
    }
    if values
        .get("Type")
        .is_some_and(|value| *value != "Application")
    {
        return Ok(());
    }
    if let Some(name) = values.get("Name") {
        found.titles.push((75, name.replace("\\s", " ")));
    }
    if let Some(icon) = values.get("Icon") {
        for candidate in icon_paths(path, icon) {
            if tree.resolve(&candidate).is_some() {
                found.icons.push(candidate);
            }
        }
    }
    let Some(command) = values.get("Exec").and_then(|text| split_args(text, false)) else {
        return Ok(());
    };
    let Some(program) = command.first() else {
        return Ok(());
    };
    let mut candidate = relative_to(path, program);
    if tree.resolve(&candidate).is_none() {
        let packaged = PathBuf::from(program.trim_start_matches('/'));
        if tree.resolve(&packaged).is_some() {
            candidate = packaged;
        } else if let Some(entry) = tree.entries.iter().find(|entry| {
            !entry.directory
                && entry
                    .path
                    .file_name()
                    .is_some_and(|name| name == program.as_str())
        }) {
            candidate = entry.path.clone();
        }
    }
    let args = command
        .into_iter()
        .skip(1)
        .filter(|arg| !["%f", "%F", "%u", "%U", "%i", "%c", "%k"].contains(&arg.as_str()))
        .map(|arg| arg.replace("%%", "%"))
        .collect();
    let working_dir = values
        .get("Path")
        .map(|dir| relative_to(path, dir))
        .and_then(|dir| tree.resolve(&dir));
    found.targets.push(ExactTarget {
        path: candidate,
        args,
        working_dir,
        score: 1000.0,
        reason: "Desktop Entry Exec".into(),
    });
    Ok(())
}

fn app(tree: &Tree, path: &Path, bytes: &[u8], found: &mut Found) -> std::io::Result<()> {
    let data = plist::Value::from_reader(Cursor::new(bytes)).map_err(std::io::Error::other)?;
    let Some(dict) = data.as_dictionary() else {
        return Ok(());
    };
    let string = |key| dict.get(key).and_then(plist::Value::as_string);
    if let Some(name) = string("CFBundleDisplayName").or_else(|| string("CFBundleName")) {
        found.titles.push((70, name.into()));
    }
    if let Some(id) = string("CFBundleIdentifier") {
        found.ids.insert("bundle".into(), id.into());
    }
    let Some(contents) = path.parent() else {
        return Ok(());
    };
    let Some(bundle) = contents.parent() else {
        return Ok(());
    };
    if let Some(executable) = string("CFBundleExecutable") {
        let target = contents.join("MacOS").join(executable);
        if tree.resolve(&target).is_some() {
            found.targets.push(ExactTarget {
                path: target,
                args: vec![],
                working_dir: tree.resolve(bundle),
                score: 1000.0,
                reason: "Info.plist CFBundleExecutable".into(),
            });
        }
    }
    if let Some(icon) = string("CFBundleIconFile") {
        let mut icon = PathBuf::from(icon);
        if icon.extension().is_none() {
            icon.set_extension("icns");
        }
        found.icons.push(contents.join("Resources").join(icon));
    }
    Ok(())
}

/// `.AppImage` names anywhere, and AppImage headers on top-level files.
#[cfg(feature = "linux-formats")]
fn appimage_candidate(tree: &Tree, path: &Path, lower: &str) -> bool {
    use std::io::Read;
    if lower.ends_with(".appimage") {
        return true;
    }
    let mut header = [0u8; 11];
    path.components().count() == 1
        && tree
            .open(path)
            .and_then(|mut file| file.read_exact(&mut header))
            .is_ok()
        && crate::formats::linux::appimage::is_appimage(&header)
}

/// The image is the launch target; title, ID and icon come from its
/// embedded desktop entry and AppStream data, read without running it.
#[cfg(feature = "linux-formats")]
fn appimage(
    tree: &Tree,
    path: &Path,
    config: DiscoveryConfig,
    found: &mut Found,
) -> std::io::Result<()> {
    use crate::formats::linux::appimage::{AppImageLimits, read_metadata};
    let file = tree.open(path)?;
    let metadata = read_metadata(
        std::io::BufReader::new(file),
        AppImageLimits {
            max_metadata_bytes: config.max_metadata_bytes,
            max_icon_bytes: config.max_resource_bytes,
            ..AppImageLimits::default()
        },
    )
    .map_err(std::io::Error::other)?;
    if let Some(name) = metadata.appstream_name {
        found.titles.push((77, name));
    }
    if let Some(name) = metadata.name {
        found.titles.push((75, name));
    }
    if let Some(id) = metadata.appstream_id {
        found.ids.insert("appstream".into(), id);
    }
    let args = metadata
        .exec
        .as_deref()
        .and_then(|text| split_args(text, false))
        .map(|command| {
            command
                .into_iter()
                .skip(1)
                .filter(|arg| !arg.starts_with('%') || arg == "%%")
                .map(|arg| arg.replace("%%", "%"))
                .collect()
        })
        .unwrap_or_default();
    found.targets.push(ExactTarget {
        path: path.to_path_buf(),
        args,
        working_dir: None,
        score: if metadata.desktop_file.is_some() {
            1000.0
        } else {
            900.0
        },
        reason: "AppImage desktop entry".into(),
    });
    for icon in metadata.icons {
        found
            .embedded_icons
            .push((path.join(icon.path), icon.bytes));
    }
    Ok(())
}

fn relative_to(metadata: &Path, target: &str) -> PathBuf {
    let target = target.replace('\\', "/");
    metadata.parent().unwrap_or(Path::new("")).join(target)
}

fn icon_paths(metadata: &Path, icon: &str) -> Vec<PathBuf> {
    let mut paths = vec![relative_to(metadata, icon), PathBuf::from(icon)];
    if Path::new(icon).extension().is_none() {
        for ext in ["png", "ico", "icns"] {
            for directory in [
                "",
                "icons",
                "usr/share/pixmaps",
                "usr/share/icons/hicolor/256x256/apps",
                "usr/share/icons/hicolor/128x128/apps",
                "usr/share/icons/hicolor/48x48/apps",
            ] {
                paths.push(Path::new(directory).join(format!("{icon}.{ext}")));
            }
        }
    }
    paths
}

/// Tokenize command metadata into argv without invoking or expanding a shell.
/// The Windows mode preserves ordinary path backslashes and handles runs of
/// backslashes before quotes according to CommandLineToArgvW conventions.
pub(crate) fn split_args(text: &str, windows: bool) -> Option<Vec<String>> {
    let mut args = vec![];
    let mut current = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '"' | '\'' if character == '"' || !windows => {
                if quote == Some(character) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(character);
                } else {
                    current.push(character);
                }
                started = true;
            }
            '\\' if windows => {
                let mut count = 1;
                while chars.peek() == Some(&'\\') {
                    chars.next();
                    count += 1;
                }
                if chars.peek() == Some(&'"') {
                    current.extend(std::iter::repeat_n('\\', count / 2));
                    chars.next();
                    if count % 2 == 0 {
                        quote = if quote.is_some() { None } else { Some('"') };
                    } else {
                        current.push('"');
                    }
                } else {
                    current.extend(std::iter::repeat_n('\\', count));
                }
                started = true;
            }
            '\\' if quote != Some('\'') => {
                current.push(chars.next()?);
                started = true;
            }
            c if c.is_whitespace() && quote.is_none() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        args.push(current);
    }
    Some(args)
}
