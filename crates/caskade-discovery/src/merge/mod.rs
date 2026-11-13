//! Ordered identity union, metadata preference and exclusions (plan §8).
//! Never compares titles. Inputs are retained as alternatives in each group;
//! only the preferred entry is enriched with missing metadata.
use crate::{
    Confidence, DetectedGame, Evidence, ExternalId, GameGroup, HostEnv, LaunchTarget, SourceId,
    identity::{CanonicalPath, canonical_path},
    orchestrator::Exclusions,
    uri::{self, UriIdentity},
};
use globset::{GlobBuilder, GlobMatcher};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct MergeConfig {
    /// Explicit sources first; omitted sources keep their default class order.
    pub source_priority: Vec<SourceId>,
    pub exclusions: Exclusions,
    pub min_confidence: Confidence,
}
impl Default for MergeConfig {
    fn default() -> Self {
        Self {
            source_priority: Vec::new(),
            exclusions: Exclusions::default(),
            min_confidence: Confidence::Maybe,
        }
    }
}

/// Merge a scan deterministically. Invalid exclusion globs are configuration
/// errors; unreadable path evidence is ignored, without discarding ID evidence.
pub fn merge(
    env: &HostEnv,
    entries: Vec<DetectedGame>,
    config: &MergeConfig,
) -> crate::Result<Vec<GameGroup>> {
    // Every path's identity looks up its volume: read the list once.
    let env = &env.with_volume_snapshot();
    let globs = ExclusionGlobs::new(&config.exclusions.paths)?;
    let data = crate::data::Data::builtin()?;
    let redists = caskade_installer::redist::RedistTable::default();
    let mut managed = HashMap::new();
    let mut records = Vec::new();
    for mut entry in entries {
        let mut ids: BTreeSet<_> = entry
            .ids
            .iter()
            .flat_map(uri::external_identities)
            .collect();
        let flatpak_scope = entry.launch.iter().find_map(|target| {
            if let LaunchTarget::Flatpak { app_id, .. } = target {
                Some(app_id.as_str())
            } else {
                None
            }
        });
        for target in entry.launch.iter().chain(
            entry
                .uninstall
                .iter()
                .flat_map(|u| std::iter::once(&u.target).chain(u.quiet.iter())),
        ) {
            ids.extend(uri::scope_local(
                uri::target_identities(target),
                flatpak_scope,
            ));
        }
        for id in &ids {
            if let UriIdentity::External(id) = id
                && !entry.ids.contains(id)
            {
                entry.ids.push(id.clone());
            }
        }
        let paths = anchors(env, &entry, !ids.is_empty());
        if builtin_excluded(env, &entry, &paths, &ids, data, &redists, &mut managed) {
            continue;
        }
        records.push(Record { entry, ids, paths });
    }
    // A short Epic app-name URI cannot choose between conflicting complete
    // namespace/catalog identities. Keep those owners separate conservatively.
    let mut epic_owners: BTreeMap<String, BTreeSet<ExternalId>> = BTreeMap::new();
    for record in &records {
        for id in &record.entry.ids {
            if let ExternalId::Epic {
                namespace,
                catalog_item,
                app_name,
            } = id
                && !namespace.is_empty()
                && !catalog_item.is_empty()
            {
                epic_owners
                    .entry(app_name.clone())
                    .or_default()
                    .insert(id.clone());
            }
        }
    }
    let ambiguous: BTreeSet<_> = epic_owners
        .into_iter()
        .filter(|(_, owners)| owners.len() > 1)
        .map(|(key, _)| UriIdentity::Launcher {
            source: SourceId::new("epic.app"),
            key,
        })
        .collect();
    let mut union = Union::new(records.len());
    let mut identities = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        for identity in &record.ids {
            if ambiguous.contains(identity) {
                continue;
            }
            if let Some(other) = identities.insert(identity.clone(), index) {
                union.join(index, other);
            }
        }
    }
    // ID-less launcher-owned registry aliases may attach to an owner's
    // EXACT install directory. Never give their paths containment authority:
    // a library root or wrapper parent must not join unrelated games.
    let owner_dirs: BTreeMap<_, _> = records
        .iter()
        .enumerate()
        .filter(|(_, r)| !uri::is_proxy(&r.entry))
        .flat_map(|(index, r)| {
            r.paths
                .iter()
                .filter(|p| p.directory)
                .map(move |p| (p.path.key(), index))
        })
        .collect();
    for (index, record) in records.iter().enumerate() {
        if record.ids.is_empty()
            && is_launcher_alias(&record.entry)
            && let Some(dir) = &record.entry.install_dir
            && let Ok(path) = canonical_path(env, dir, &record.entry.runtime)
            && let Some(owner) = owner_dirs.get(&path.key())
        {
            union.join(index, *owner);
        }
    }
    // Sorted component keys make containment O(n log n) rather than checking
    // every pair. A parent links all descendants transitively through itself.
    let mut paths: Vec<_> = records
        .iter()
        .enumerate()
        .flat_map(|(index, r)| r.paths.iter().map(move |p| (p.path.key(), index, p)))
        .collect();
    paths.sort_by(|a, b| (&a.0, !a.2.directory, a.1).cmp(&(&b.0, !b.2.directory, b.1)));
    let mut parents: Vec<(Vec<Vec<u8>>, usize, &Anchor)> = Vec::new();
    let mut previous: Option<(Vec<Vec<u8>>, usize)> = None;
    for (key, index, path) in paths {
        while parents.last().is_some_and(|(k, _, _)| !key.starts_with(k)) {
            parents.pop();
        }
        if let Some((_, parent, anchor)) = parents.last()
            && anchor.path.contains(&path.path)
        {
            union.join(index, *parent);
        }
        if let Some((old, other)) = &previous
            && *old == key
        {
            union.join(index, *other);
        }
        previous = Some((key.clone(), index));
        if path.directory {
            parents.push((key, index, path));
        }
    }
    let mut groups: BTreeMap<usize, Vec<Record>> = BTreeMap::new();
    for (index, record) in records.into_iter().enumerate() {
        groups.entry(union.root(index)).or_default().push(record);
    }
    let excluded_ids: BTreeSet<_> = config
        .exclusions
        .external_ids
        .iter()
        .flat_map(uri::external_identities)
        .filter(|identity| !ambiguous.contains(identity))
        .collect();
    let mut result = Vec::new();
    for mut group in groups.into_values() {
        if group.iter().all(|r| uri::is_proxy(&r.entry)) {
            continue;
        }
        // A user exclusion belongs to the logical game, including lower-priority
        // shortcuts that would otherwise resurrect it through another source.
        if group.iter().any(|r| {
            !r.ids.is_disjoint(&excluded_ids)
                || config
                    .exclusions
                    .source_keys
                    .iter()
                    .any(|k| k.source == r.entry.source && k.key == r.entry.key)
                || globs.matches(&r.entry, &r.paths)
        }) {
            continue;
        }
        group.sort_by(|a, b| {
            uri::is_proxy(&a.entry)
                .cmp(&uri::is_proxy(&b.entry))
                .then_with(|| rank(&a.entry, config).cmp(&rank(&b.entry, config)))
                .then_with(|| a.entry.install_dir.cmp(&b.entry.install_dir))
                .then_with(|| a.entry.title.cmp(&b.entry.title))
        });
        let mut entries: Vec<_> = group.into_iter().map(|r| r.entry).collect();
        enrich(&mut entries);
        if entries[0].confidence < config.min_confidence {
            continue;
        }
        result.push(GameGroup {
            primary: 0,
            entries,
        });
    }
    result.sort_by(|a, b| rank(&a.entries[0], config).cmp(&rank(&b.entries[0], config)));
    Ok(result)
}
fn is_launcher_alias(entry: &DetectedGame) -> bool {
    entry
        .evidence
        .iter()
        .any(|e| matches!(e, Evidence::Rule { rule, .. } if rule == "alias.launcher_owned"))
}
struct Record {
    entry: DetectedGame,
    ids: BTreeSet<UriIdentity>,
    paths: Vec<Anchor>,
}
struct Anchor {
    path: CanonicalPath,
    directory: bool,
}
fn anchors(env: &HostEnv, entry: &DetectedGame, has_id: bool) -> Vec<Anchor> {
    if uri::is_proxy(entry) {
        return Vec::new();
    }
    let mut paths = Vec::new();
    if let Some(dir) = &entry.install_dir
        && let Ok(path) = canonical_path(env, dir, &entry.runtime)
    {
        paths.push(Anchor {
            path,
            directory: true,
        });
    }
    // Launcher wrappers shared by many games are not install identities.
    for target in &entry.launch {
        if let LaunchTarget::Exec { program, .. } = target
            && uri::target_identities(target).is_empty()
            && !wrapper_program(program)
            && !(has_id && launcher_program(program, None))
            && let Ok(path) = canonical_path(env, program, &entry.runtime)
        {
            paths.push(Anchor {
                path,
                directory: false,
            });
        }
    }
    paths
}
fn rank<'a>(entry: &'a DetectedGame, config: &MergeConfig) -> (usize, &'a SourceId, &'a str) {
    let source = entry.source.as_str();
    let tier = if source.contains("deep") {
        400
    } else if source.contains("shortcut")
        || source.contains("desktop")
        || source.contains("start_menu")
        || source.contains("startmenu")
    {
        300
    } else if source.contains("uninstall")
        || source.contains("registry")
        || source.ends_with(".flatpak")
        || source.ends_with(".snap")
    {
        200
    } else if source == "macos.apps"
        || source.contains("game_explorer")
        || source.contains("gameexplorer")
        || source.contains("gameux")
    {
        100
    } else {
        0
    };
    let order = config
        .source_priority
        .iter()
        .position(|s| s == &entry.source)
        .unwrap_or(config.source_priority.len() + tier);
    (order, &entry.source, &entry.key)
}
fn enrich(entries: &mut [DetectedGame]) {
    let Some((primary, rest)) = entries.split_first_mut() else {
        return;
    };
    for other in rest {
        for id in &other.ids {
            if !primary.ids.contains(id) {
                primary.ids.push(id.clone());
            }
        }
        // Identity-only links contribute ids, not their wrapper metadata or
        // role markers. In particular they cannot turn an owner into a proxy.
        if uri::is_proxy(other) {
            // A launcher-owned registry alias can supply a missing inverse,
            // never wrapper title/path/category/runtime metadata.
            if primary.uninstall.is_none() && is_launcher_alias(other) {
                primary.uninstall.clone_from(&other.uninstall);
            }
            continue;
        }
        if primary.title.is_empty() {
            primary.title.clone_from(&other.title);
        }
        if primary.install_dir.is_none() {
            primary.install_dir.clone_from(&other.install_dir);
        }
        if primary.icon.is_none() {
            primary.icon.clone_from(&other.icon);
        }
        if primary.uninstall.is_none() {
            primary.uninstall.clone_from(&other.uninstall);
        }
        if primary.size.is_none() {
            primary.size = other.size;
        }
        if primary.playtime_secs.is_none() {
            primary.playtime_secs = other.playtime_secs;
        }
        if primary.last_played_unix.is_none() {
            primary.last_played_unix = other.last_played_unix;
        }
        if primary.tracking.is_none() {
            primary.tracking.clone_from(&other.tracking);
        }
        if let (
            crate::GameRuntime::Windows { prefix, runner },
            crate::GameRuntime::Windows {
                prefix: other_prefix,
                runner: other_runner,
            },
        ) = (&mut primary.runtime, &other.runtime)
        {
            if prefix.is_none() {
                prefix.clone_from(other_prefix);
            }
            if runner.is_none() {
                runner.clone_from(other_runner);
            }
        }
        primary.confidence = primary.confidence.max(other.confidence);
        for art in &other.artwork {
            if !primary.artwork.iter().any(|a| a.kind == art.kind) {
                primary.artwork.push(art.clone());
            }
        }
        for target in &other.launch {
            if !primary.launch.contains(target) {
                primary.launch.push(target.clone());
            }
        }
        for evidence in &other.evidence {
            if !primary.evidence.contains(evidence) {
                primary.evidence.push(evidence.clone());
            }
        }
    }
}
/// Whether `dir` holds a Caskade install manifest. Anchors share most of
/// their ancestors, so one merge asks each directory once.
fn managed_dir(env: &HostEnv, dir: &Path, managed: &mut HashMap<PathBuf, bool>) -> bool {
    if let Some(&known) = managed.get(dir) {
        return known;
    }
    let found = env
        .resolve_path(&dir.join(".caskade/install.json"))
        .is_ok_and(|p| p.is_file());
    managed.insert(dir.to_path_buf(), found);
    found
}
fn builtin_excluded(
    env: &HostEnv,
    entry: &DetectedGame,
    paths: &[Anchor],
    ids: &BTreeSet<UriIdentity>,
    data: &crate::data::Data,
    redists: &caskade_installer::redist::RedistTable,
    managed: &mut HashMap<PathBuf, bool>,
) -> bool {
    if entry.evidence.iter().any(|e| matches!(e, Evidence::Rule { rule, .. } if matches!(rule.as_str(),"category.launcher"|"category.emulator"|"category.redist"|"category.driver"|"category.sdk"|"category.tool"))) { return true; }
    if entry.ids.iter().any(|id| matches!(id,ExternalId::Steam { app_id } if data.steam_tools.tools.iter().any(|t| t.app_id == *app_id))) { return true; }
    if paths.iter().any(|anchor| {
        anchor
            .path
            .path
            .ancestors()
            .any(|dir| managed_dir(env, dir, managed))
    }) {
        return true;
    }
    let explicit_game = entry.evidence.iter().any(|e| matches!(e, Evidence::Rule { rule, score } if rule == "metadata.game_category" && *score > 0));
    if ids.is_empty() && !explicit_game {
        for target in &entry.launch {
            match target {
                LaunchTarget::Exec { program, .. } => {
                    if launcher_program(program, Some(data)) {
                        return true;
                    }
                    let name = program_name(program);
                    if ["retroarch", "pcsx2", "rpcs3", "ppsspp", "dolphin-emu"]
                        .contains(&name.trim_end_matches(".exe"))
                    {
                        return true;
                    }
                    if !redists
                        .detect(
                            std::iter::once(Path::new(&name)),
                            std::iter::empty(),
                            &|_| None,
                        )
                        .is_empty()
                    {
                        return true;
                    }
                }
                LaunchTarget::Flatpak { app_id, .. }
                    if data
                        .launchers
                        .launchers
                        .iter()
                        .any(|l| l.flatpak_ids.contains(app_id)) =>
                {
                    return true;
                }
                _ => {}
            }
        }
    }
    false
}
fn program_name(path: &Path) -> String {
    path.to_str()
        .and_then(|v| v.rsplit(['/', '\\']).next())
        .unwrap_or_default()
        .to_ascii_lowercase()
}
fn wrapper_program(path: &Path) -> bool {
    let name = program_name(path);
    [
        "flatpak",
        "env",
        "wine",
        "wine64",
        "proton",
        "umu-run",
        "xdg-open",
        "explorer.exe",
    ]
    .contains(&name.as_str())
}
fn launcher_program(path: &Path, data: Option<&crate::data::Data>) -> bool {
    let name = program_name(path);
    let base = name.strip_suffix(".exe").unwrap_or(&name);
    [
        "steam",
        "heroic",
        "galaxyclient",
        "epicgameslauncher",
        "ubisoftconnect",
        "upc",
        "uplay",
        "battle.net",
        "origin",
        "eadesktop",
        "riotclientservices",
        "amazongames",
        "lutris",
        "itch",
    ]
    .contains(&base)
        || data.is_some_and(|d| {
            d.launchers
                .launchers
                .iter()
                .any(|l| l.exes.iter().any(|e| e.eq_ignore_ascii_case(&name)))
        })
}
struct ExclusionGlobs {
    sensitive: Vec<GlobMatcher>,
    insensitive: Vec<GlobMatcher>,
}
impl ExclusionGlobs {
    fn new(patterns: &[String]) -> crate::Result<Self> {
        let compile = |insensitive| {
            patterns
                .iter()
                .map(|p| {
                    GlobBuilder::new(&p.replace('\\', "/"))
                        .literal_separator(true)
                        .case_insensitive(insensitive)
                        .build()
                        .map(|g| g.compile_matcher())
                        .map_err(|e| crate::Error::Usage(format!("invalid exclusion glob: {e}")))
                })
                .collect::<crate::Result<Vec<_>>>()
        };
        Ok(Self {
            sensitive: compile(false)?,
            insensitive: compile(true)?,
        })
    }
    fn matches(&self, entry: &DetectedGame, paths: &[Anchor]) -> bool {
        if uri::is_proxy(entry) {
            return false;
        }
        let canonical = paths.iter().any(|p| {
            let globs = if p.path.case_sensitive {
                &self.sensitive
            } else {
                &self.insensitive
            };
            globs.iter().any(|g| g.is_match(&p.path.path))
        });
        if canonical {
            return true;
        }
        let sensitive = paths.iter().all(|p| p.path.case_sensitive);
        let globs = if sensitive {
            &self.sensitive
        } else {
            &self.insensitive
        };
        entry
            .install_dir
            .iter()
            .chain(entry.launch.iter().filter_map(|t| {
                if let LaunchTarget::Exec { program, .. } = t {
                    Some(program)
                } else {
                    None
                }
            }))
            .any(|p| {
                p.to_str().is_some_and(|p| {
                    globs
                        .iter()
                        .any(|g| g.is_match(PathBuf::from(p.replace('\\', "/"))))
                })
            })
    }
}
struct Union(Vec<usize>);
impl Union {
    fn new(len: usize) -> Self {
        Self((0..len).collect())
    }
    fn root(&mut self, index: usize) -> usize {
        let mut root = index;
        while self.0[root] != root {
            root = self.0[root];
        }
        let mut current = index;
        while self.0[current] != root {
            let next = self.0[current];
            self.0[current] = root;
            current = next;
        }
        root
    }
    fn join(&mut self, a: usize, b: usize) {
        let a = self.root(a);
        let b = self.root(b);
        self.0[a.max(b)] = a.min(b);
    }
}
