use super::{
    ArchiveMember, CancellationToken, Exclusions, Result, RomFile, RomGame, ScanError, ScanReport,
    ScanRequest, ScanWarning, check, extension, parse_name, path_text, playlist_references,
};
use crate::emulation::{GameRom, RomLocation};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// Transform an enumerated file list without IO. Playlist text and archive
/// directories are input data; no ROM bytes are read by this function.
type GroupKey = (String, String, String);
type GroupPart = (u32, String, RomGame);

pub fn scan_files(
    files: &[RomFile],
    request: &ScanRequest<'_>,
    cancel: &CancellationToken,
) -> Result<ScanReport> {
    check(cancel)?;
    if files.len() > request.limits.max_files {
        return Err(ScanError::Limit("files"));
    }
    if request.platforms.is_empty() {
        return Err(ScanError::Invalid(crate::tr!(
            "scanner profile has no compatible platforms"
        )));
    }
    if let Some(id) = &request.scanner.platform
        && !request.platforms.iter().any(|p| p.id == *id)
    {
        return Err(ScanError::Invalid(crate::tr!(
            "This scanner's platform doesn't fit its profile."
        )));
    }
    let exclusions = Exclusions::compile(
        request
            .global_exclusions
            .iter()
            .chain(&request.scanner.exclusions),
    )?;
    let mut report = ScanReport {
        enumerated: files.len() as u32,
        ..Default::default()
    };
    let mut inventory = BTreeMap::new();
    let mut total_text = 0usize;
    let mut total_entries = 0usize;
    for file in files {
        check(cancel)?;
        let path = path_text(&file.path)?;
        total_text = total_text.saturating_add(file.playlist.as_ref().map_or(0, String::len));
        total_entries = total_entries.saturating_add(file.archive_entries.len());
        if total_text > request.limits.max_total_text_bytes {
            return Err(ScanError::Limit("total playlist text"));
        }
        if total_entries > request.limits.max_total_archive_entries {
            return Err(ScanError::Limit("total archive entries"));
        }
        if file.hidden
            || path.split('/').any(|p| p.starts_with('.'))
            || (request.scanner.skip_cloud_placeholders && file.cloud_placeholder)
            || (!request.scanner.recursive && path.contains('/'))
            || exclusions.matches(&path)
        {
            continue;
        }
        report
            .warnings
            .extend(file.warnings.iter().map(|message| warning(&path, message)));
        if inventory.insert(path, file).is_some() {
            return Err(ScanError::Invalid(crate::tr!(
                "duplicate ROM path in file list"
            )));
        }
    }
    let mut folded: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in inventory.keys() {
        folded
            .entry(path.to_lowercase())
            .or_default()
            .push(path.clone());
    }
    let mut candidates = BTreeMap::new();
    let accepted = |path: &Path| {
        request.extensions.iter().any(|e| {
            e.trim_start_matches('.')
                .eq_ignore_ascii_case(&extension(path))
        })
    };
    for (path, file) in &inventory {
        check(cancel)?;
        let mut inner: Vec<&ArchiveMember> = Vec::new();
        let direct = accepted(Path::new(path));
        if !direct
            && request.scanner.scan_archives
            && matches!(extension(Path::new(path)).as_str(), "zip" | "7z")
        {
            if file.archive_entries.len() > request.limits.max_archive_entries {
                return Err(ScanError::Limit("archive entries"));
            }
            for entry in &file.archive_entries {
                let entry_path = path_text(&entry.path)?;
                if accepted(&entry.path)
                    && !entry_path.split('/').any(|p| p.starts_with('.'))
                    && !exclusions.matches(&format!("{path}/{entry_path}"))
                {
                    inner.push(entry);
                }
            }
        }
        if !direct && inner.is_empty() {
            continue;
        }
        let extensions: Vec<_> = if direct {
            vec![extension(Path::new(path))]
        } else {
            inner.iter().map(|entry| extension(&entry.path)).collect()
        };
        let inferred: Vec<_> = request
            .platforms
            .iter()
            .filter(|p| {
                p.extensions
                    .iter()
                    .any(|e| extensions.iter().any(|ext| e.eq_ignore_ascii_case(ext)))
            })
            .collect();
        let platform = if let Some(id) = &request.scanner.platform {
            id.clone()
        } else if inferred.len() == 1 {
            inferred[0].id.clone()
        } else if request.platforms.len() == 1 {
            request.platforms[0].id.clone()
        } else {
            report.warnings.push(warning(
                path,
                &crate::tr!("ROM matches multiple platforms; choose a scanner platform"),
            ));
            continue;
        };
        let mut parsed = parse_name(Path::new(path), request.regions);
        let platform =
            if let Some(identity) = request.identifier.and_then(|id| id.identify(file, &parsed)) {
                if !request.platforms.iter().any(|p| p.id == identity.platform) {
                    return Err(ScanError::Invalid(crate::tr!(
                        "identifier returned an incompatible platform"
                    )));
                }
                parsed.title = identity.title;
                parsed.regions = identity.regions;
                identity.platform
            } else {
                platform
            };
        if parsed.title.is_empty() {
            report.warnings.push(warning(
                path,
                &crate::tr!("ROM has no title after removing properties"),
            ));
            continue;
        }
        candidates.insert(path.clone(), (platform, parsed));
    }
    // Validate playlist graphs against the complete (filtered) inventory.
    // Unsupported playlists do not hide files the selected emulator can play.
    let mut edges = BTreeMap::new();
    let mut invalid = BTreeSet::new();
    let mut count = 0;
    for path in inventory.keys().filter(|p| is_playlist(Path::new(p))) {
        let file = inventory[path];
        let result = (|| {
            let text = file
                .playlist
                .as_deref()
                .ok_or_else(|| ScanError::Invalid(crate::tr!("playlist text is unavailable")))?;
            if text.len() > request.limits.max_playlist_bytes {
                return Err(ScanError::Limit("playlist bytes"));
            }
            let refs = playlist_references(Path::new(path), text)?;
            count += refs.len();
            if count > request.limits.max_references {
                return Err(ScanError::Limit("playlist references"));
            }
            refs.into_iter()
                .map(|p| {
                    let key = path_text(&p)?;
                    if inventory.contains_key(&key) {
                        return Ok(key);
                    }
                    match folded.get(&key.to_lowercase()).map(Vec::as_slice) {
                        Some([only]) => Ok(only.clone()),
                        _ => Err(ScanError::Invalid(crate::tr!(
                            "playlist member is missing or ambiguous: {member}",
                            member = key
                        ))),
                    }
                })
                .collect::<Result<Vec<_>>>()
        })();
        match result {
            Ok(refs) => {
                edges.insert(path.clone(), refs);
            }
            Err(ScanError::Limit(v)) => return Err(ScanError::Limit(v)),
            Err(e) => {
                invalid.insert(path.clone());
                if candidates.contains_key(path) {
                    report.warnings.push(warning(path, &e.to_string()));
                }
            }
        }
    }
    let mut valid = BTreeMap::new();
    let mut validator = PlaylistGraph {
        edges: &edges,
        invalid: &invalid,
        cancel,
        remaining: request.limits.max_references.saturating_mul(32),
    };
    for path in candidates
        .keys()
        .filter(|p| is_playlist(Path::new(p)) && edges.contains_key(*p))
    {
        check(cancel)?;
        let okay = validator.validate(path, &mut BTreeSet::new(), 0)?;
        valid.insert(path.clone(), okay);
        if !okay {
            report.warnings.push(warning(
                path,
                &crate::tr!("playlist contains a cycle, invalid member or excessive nesting"),
            ));
        }
    }
    let mut claimed = BTreeSet::new();
    let mut pending: Vec<_> = candidates
        .keys()
        .filter(|path| is_playlist(Path::new(path)) && valid.get(*path) == Some(&true))
        .cloned()
        .collect();
    let mut visited = BTreeSet::new();
    while let Some(path) = pending.pop() {
        check(cancel)?;
        if !visited.insert(path.clone()) {
            continue;
        }
        if let Some(refs) = edges.get(&path) {
            for reference in refs {
                claimed.insert(reference.clone());
                pending.push(reference.clone());
            }
        }
    }
    let mut grouped: BTreeMap<GroupKey, Vec<GroupPart>> = BTreeMap::new();
    for (path, (platform, parsed)) in candidates {
        check(cancel)?;
        if claimed.contains(&path) || invalid.contains(&path) || valid.get(&path) == Some(&false) {
            continue;
        }
        let parent = Path::new(&path)
            .parent()
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .into_owned();
        let key = (parent, platform.clone(), parsed.title.to_lowercase());
        let part = parsed.part.as_ref();
        let mut members = BTreeSet::from([path.clone()]);
        let mut pending = vec![path.clone()];
        let mut steps = 0;
        while let Some(p) = pending.pop() {
            check(cancel)?;
            if let Some(refs) = edges.get(&p) {
                for r in refs {
                    if members.insert(r.clone()) {
                        pending.push(r.clone());
                    }
                }
            }
            steps += 1;
            if steps > request.limits.max_references {
                return Err(ScanError::Limit("playlist expansion"));
            }
        }
        let rom = GameRom {
            location: RomLocation::Scanner {
                scanner_id: request.scanner.id.clone(),
                path: PathBuf::from(&path),
            },
            name: part.map(|p| p.label.clone()),
            size: Some(inventory[&path].size),
        };
        let game = RomGame {
            title: parsed.title,
            platform,
            regions: parsed.regions,
            properties: parsed.properties,
            roms: vec![rom],
            files: members.into_iter().map(PathBuf::from).collect(),
        };
        grouped
            .entry(key)
            .or_default()
            .push((part.map_or(0, |p| p.number), path, game));
    }
    for mut group in grouped.into_values() {
        group.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut games = group.into_iter().map(|(_, _, game)| game);
        let mut game = games.next().expect("group is never empty");
        for other in games {
            game.roms.extend(other.roms);
            game.files.extend(other.files);
            game.regions.extend(other.regions);
            game.properties.extend(other.properties);
        }
        game.files.sort();
        game.files.dedup();
        game.regions.sort();
        game.regions.dedup();
        game.properties.sort();
        game.properties.dedup();
        report.games.push(game);
    }
    let imported: usize = report.games.iter().map(|g| g.roms.len()).sum();
    report.skipped = report.enumerated.saturating_sub(imported as u32);
    Ok(report)
}
fn is_playlist(path: &Path) -> bool {
    matches!(extension(path).as_str(), "m3u" | "m3u8" | "cue" | "gdi")
}
fn warning(path: &str, message: &str) -> ScanWarning {
    ScanWarning {
        path: Some(path.into()),
        message: message.into(),
    }
}
struct PlaylistGraph<'a> {
    edges: &'a BTreeMap<String, Vec<String>>,
    invalid: &'a BTreeSet<String>,
    cancel: &'a CancellationToken,
    remaining: usize,
}
impl PlaylistGraph<'_> {
    // A depth failure belongs to this root, not to a child that may be a valid
    // independent playlist. Do not memoise context-dependent false results.
    fn validate(
        &mut self,
        path: &str,
        active: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<bool> {
        check(self.cancel)?;
        if self.remaining == 0 {
            return Err(ScanError::Limit("playlist graph visits"));
        }
        self.remaining -= 1;
        if depth > 32 || self.invalid.contains(path) || !active.insert(path.into()) {
            return Ok(false);
        }
        let mut valid = true;
        let edges = self.edges;
        if let Some(refs) = edges.get(path) {
            for reference in refs {
                if !self.validate(reference, active, depth + 1)? {
                    valid = false;
                    break;
                }
            }
        }
        active.remove(path);
        Ok(valid)
    }
}
