use super::{
    CaskadeRoot, ID, Report, ScanKind,
    control::Control,
    executable::{self, Kind},
};
use crate::{
    classify::{self, Metadata},
    types::*,
};
use cap_std::fs::Dir;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    kind: Option<Kind>,
}

pub(super) fn root(
    root: &Path,
    control: &mut Control<'_>,
    report: &mut Report,
) -> Result<(), SourceError> {
    control.check()?;
    let metadata = match fs::symlink_metadata(root) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            control.ctx.warn(format!("{}: {e}", root.display()));
            return Ok(());
        }
    };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    let (anchor, requested) = if metadata.is_file() {
        (
            root.parent().unwrap_or(root),
            root.file_name().map(PathBuf::from),
        )
    } else {
        (root, None)
    };
    let dir = match Dir::open_ambient_dir(anchor, cap_std::ambient_authority()) {
        Ok(d) => d,
        Err(e) => {
            control.ctx.warn(format!("{}: {e}", root.display()));
            return Ok(());
        }
    };
    let mut entries = Vec::new();
    if let Some(path) = requested {
        control.entry(root, report.games.len())?;
        let kind = executable::inspect(&dir, &path, control)?;
        if kind == Some(Kind::AppImage) {
            appimage(&dir, anchor, &path, control, report)?;
        } else {
            entries.push(Entry { path, kind });
            candidate(anchor, Path::new(""), &entries, false, control, report)?;
        }
    } else {
        walk(
            &dir,
            anchor,
            Path::new(""),
            0,
            &mut entries,
            control,
            report,
        )?;
    }
    Ok(())
}

fn walk(
    dir: &Dir,
    anchor: &Path,
    relative: &Path,
    depth: u32,
    entries: &mut Vec<Entry>,
    control: &mut Control<'_>,
    report: &mut Report,
) -> Result<bool, SourceError> {
    control.check()?;
    let physical = anchor.join(relative);
    // Read the root identity through the capability, never through a link that
    // could leave the selected tree. Storage owns the contents of valid roots.
    let marker = relative.join(".caskade/root.json");
    if let Ok(file) = dir.open(&marker) {
        let mut bytes = Vec::new();
        if control
            .reader(file)
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .is_ok()
            && bytes.len() <= 16 * 1024
            && let Ok(root) = caskade_installer::manifest::RootManifest::from_json(&bytes)
        {
            report.caskade_roots.push(CaskadeRoot {
                path: physical,
                root_id: root.root_id().to_owned(),
            });
            return Ok(false);
        }
        control.check()?;
    }
    if crate::sources::prefixes::is_prefix(control.env, &physical) {
        if matches!(
            control.kind,
            ScanKind::WinePrefixes | ScanKind::ExternalDrives
        ) {
            control.check()?;
            match crate::sources::prefixes::scan_prefix(
                control.env,
                &physical,
                None,
                None,
                control.ctx,
            ) {
                Ok(games) => report.games.extend(games),
                Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
                Err(e) => control.ctx.warn(e.to_string()),
            }
            control.check()?;
        }
        return Ok(false);
    }
    let read = match dir.read_dir(if relative.as_os_str().is_empty() {
        Path::new(".")
    } else {
        relative
    }) {
        Ok(entries) => entries,
        Err(e) => {
            control.ctx.warn(format!("{}: {e}", physical.display()));
            return Ok(false);
        }
    };
    let mut children = Vec::new();
    for entry in read {
        control.entry(&physical, report.games.len())?;
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                control.ctx.warn(e.to_string());
                continue;
            }
        };
        let path = relative.join(entry.file_name());
        control.path_bytes = control.path_bytes.saturating_add(path.as_os_str().len());
        let typ = match entry.file_type() {
            Ok(t) => t,
            Err(e) => {
                control.ctx.warn(e.to_string());
                continue;
            }
        };
        if typ.is_symlink() || (!typ.is_file() && !typ.is_dir()) {
            continue;
        }
        if typ.is_dir() && excluded(&path, control.kind) {
            continue;
        }
        children.push((path, typ.is_dir()));
    }
    children.sort_by(|a, b| a.0.cmp(&b.0));
    let start = entries.len();
    let mut descendants = false;
    let mut steam_library = false;
    if control.kind == ScanKind::ExternalDrives
        && children.iter().any(|(p, d)| {
            *d && p
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("steamapps"))
        })
    {
        control.check()?;
        match crate::sources::steam::Steam::scan_library(control.env, &physical, control.ctx) {
            Ok(games) => {
                steam_library = !games.is_empty();
                report.games.extend(games);
            }
            Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
            Err(e) => control.ctx.warn(e.to_string()),
        }
        control.check()?;
    }
    for (path, directory) in children {
        control.check()?;
        if directory {
            if steam_library
                && path
                    .file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("steamapps"))
            {
                continue;
            }
            entries.push(Entry {
                path: path.clone(),
                kind: None,
            });
            if depth < control.options.max_depth {
                descendants |= walk(dir, anchor, &path, depth + 1, entries, control, report)?;
            } else {
                control.ctx.warn(format!(
                    "deep depth budget reached at {}",
                    anchor.join(&path).display()
                ));
            }
        } else {
            let kind = match executable::inspect(dir, &path, control) {
                Ok(kind) => kind,
                Err(e) => {
                    control.check()?;
                    control.ctx.warn(e.to_string());
                    None
                }
            };
            if kind == Some(Kind::AppImage) && control.kind != ScanKind::WinePrefixes {
                descendants |= appimage(dir, anchor, &path, control, report)?;
            }
            entries.push(Entry { path, kind });
        }
    }
    if control.kind == ScanKind::WinePrefixes || steam_library {
        return Ok(descendants);
    }
    candidate(
        anchor,
        relative,
        &entries[start..],
        descendants,
        control,
        report,
    )
}

fn candidate(
    anchor: &Path,
    relative: &Path,
    entries: &[Entry],
    descendants: bool,
    control: &Control<'_>,
    report: &mut Report,
) -> Result<bool, SourceError> {
    control.check()?;
    let physical = anchor.join(relative);
    let title = physical
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Game");
    let mut candidates: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e.kind, Some(Kind::Windows | Kind::Native)))
        .filter_map(|e| {
            e.path
                .strip_prefix(relative)
                .ok()
                .map(|p| (p, e.kind.expect("filtered")))
        })
        .collect();
    // Do not collapse a directory containing several discovered games into one
    // game. A root-level executable can still identify its own parent game.
    if descendants && !candidates.iter().any(|(p, _)| p.components().count() == 1) {
        return Ok(true);
    }
    candidates.sort_by(|(a, _), (b, _)| {
        executable::rank(b, title)
            .cmp(&executable::rank(a, title))
            .then_with(|| a.cmp(b))
    });
    let files: Vec<_> = entries
        .iter()
        .filter_map(|e| {
            e.path
                .strip_prefix(relative)
                .ok()?
                .to_str()
                .map(|s| s.replace('\\', "/"))
        })
        .collect();
    if files.is_empty() {
        return Ok(descendants);
    }
    let metadata = Metadata {
        title: Some(title.into()),
        install_dir: Some(physical.clone()),
        executable: candidates.first().map(|(p, _)| physical.join(p)),
        known_game_folders: vec![anchor.to_path_buf()],
        ..Default::default()
    };
    let classification =
        classify::classify_inventory(&metadata, &files, &control.ctx.config().classifier)
            .map_err(|e| SourceError::parse(&physical, e))?;
    control.check()?;
    let Some(mut confidence) = classification.confidence() else {
        return Ok(descendants);
    };
    if candidates.is_empty() && confidence != Confidence::Likely {
        return Ok(descendants);
    }
    let ambiguous = candidates.len() > 1
        && executable::rank(candidates[0].0, title) == executable::rank(candidates[1].0, title);
    if control.kind == ScanKind::FullDisk || ambiguous {
        confidence = Confidence::Maybe;
    }
    if confidence < control.ctx.config().classifier.min_confidence {
        return Ok(descendants);
    }
    let mut game = DetectedGame::new(
        ID,
        format!("{}:{}", control.kind.label(), physical.display()),
        caskade_installer::discover::clean_title(title),
    );
    game.confidence = confidence;
    game.install_dir = Some(physical.clone());
    game.evidence = classification
        .evidence
        .into_iter()
        .map(|e| match e {
            Evidence::File { path } => Evidence::File {
                path: physical.join(path),
            },
            e => e,
        })
        .collect();
    game.evidence.push(Evidence::Rule {
        rule: format!("deep.{}", control.kind.label()),
        score: 0,
    });
    if ambiguous {
        game.evidence.push(Evidence::Rule {
            rule: "deep.ambiguous_launch".into(),
            score: 0,
        });
    }
    for (path, _) in candidates.iter().take(if ambiguous { 8 } else { 1 }) {
        game.launch.push(LaunchTarget::Exec {
            program: physical.join(path),
            args: vec![],
            cwd: Some(physical.clone()),
            env: BTreeMap::new(),
        });
    }
    if candidates
        .first()
        .is_some_and(|(_, kind)| *kind == Kind::Windows)
    {
        game.runtime = GameRuntime::Windows {
            prefix: None,
            runner: None,
        };
    }
    if game.launch.is_empty() {
        game.state = InstallState::Partial;
    }
    game.tracking = Some(TrackingHint {
        install_dir: Some(physical),
        exe_names: candidates
            .iter()
            .take(8)
            .filter_map(|(p, _)| p.file_name()?.to_str().map(str::to_owned))
            .collect(),
        ..Default::default()
    });
    report.games.push(game);
    Ok(true)
}

fn appimage(
    dir: &Dir,
    anchor: &Path,
    path: &Path,
    control: &Control<'_>,
    report: &mut Report,
) -> Result<bool, SourceError> {
    use caskade_installer::formats::linux::appimage::{AppImageLimits, read_metadata};
    control.check()?;
    let mut file = control.reader(dir.open(path).map_err(|e| SourceError::io(path, e))?);
    file.seek(SeekFrom::Start(0))
        .map_err(|e| SourceError::io(path, e))?;
    let metadata = match read_metadata(
        file,
        AppImageLimits {
            max_entries: control.options.max_entries.min(4096) as usize,
            max_metadata_bytes: 256 * 1024,
            max_icon_bytes: 0,
        },
    ) {
        Ok(m) => m,
        Err(e) => {
            control.check()?;
            control
                .ctx
                .warn(format!("{}: {e}", anchor.join(path).display()));
            return Ok(false);
        }
    };
    control.check()?;
    let physical = anchor.join(path);
    let title = metadata
        .name
        .or(metadata.appstream_name)
        .unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let categories = metadata.categories.unwrap_or_default();
    if categories
        .split(';')
        .any(|c| c.eq_ignore_ascii_case("Emulator"))
        && !categories
            .split(';')
            .any(|c| c.eq_ignore_ascii_case("Game"))
    {
        return Ok(false);
    }
    let classified = classify::classify_inventory(
        &Metadata {
            title: Some(title.clone()),
            category: Some(categories),
            executable: Some(physical.clone()),
            ..Default::default()
        },
        &[],
        &control.ctx.config().classifier,
    )
    .map_err(|e| SourceError::parse(&physical, e))?;
    control.check()?;
    let Some(mut confidence) = classified.confidence() else {
        return Ok(false);
    };
    if control.kind == ScanKind::FullDisk {
        confidence = Confidence::Maybe;
    }
    if confidence < control.ctx.config().classifier.min_confidence {
        return Ok(false);
    }
    let mut game = DetectedGame::new(
        ID,
        format!("{}:{}", control.kind.label(), physical.display()),
        title,
    );
    game.confidence = confidence;
    game.evidence = classified.evidence;
    game.evidence.push(Evidence::File {
        path: physical.clone(),
    });
    game.launch.push(LaunchTarget::Exec {
        program: physical.clone(),
        args: vec![],
        cwd: physical.parent().map(Path::to_path_buf),
        env: BTreeMap::new(),
    });
    // A single-file portable game has no distinct install directory; using the
    // shared Applications folder would incorrectly merge all its neighbours.
    game.tracking = Some(TrackingHint {
        exe_names: physical
            .file_name()
            .and_then(|p| p.to_str())
            .map(str::to_owned)
            .into_iter()
            .collect(),
        ..Default::default()
    });
    report.games.push(game);
    Ok(true)
}

fn excluded(path: &Path, kind: ScanKind) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if matches!(
        name.as_str(),
        ".git"
            | ".cache"
            | ".trash"
            | ".caskade"
            | "node_modules"
            | "target"
            | "$recycle.bin"
            | "system volume information"
    ) || executable::excluded(path)
    {
        return true;
    }
    if kind != ScanKind::FullDisk {
        return false;
    }
    let value = path.to_string_lossy().replace('\\', "/").to_lowercase();
    [
        "proc",
        "sys",
        "dev",
        "run",
        "boot",
        "tmp",
        "etc",
        "bin",
        "sbin",
        "lib",
        "lib64",
        "usr/bin",
        "usr/sbin",
        "usr/lib",
        "usr/lib64",
        "usr/include",
        "usr/share",
        "var/cache",
        "var/log",
        "var/lib",
        "windows",
        "recovery",
        "perflogs",
        "system",
        "library",
    ]
    .iter()
    .any(|p| value == *p || value.starts_with(&format!("{p}/")))
}
