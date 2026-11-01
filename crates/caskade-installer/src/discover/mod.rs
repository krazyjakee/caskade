//! Rank installed launch targets and collect metadata and dependencies.

mod binary;
mod icons;
mod metadata;
mod title;
mod tree;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use url::Url;

use crate::{
    LaunchTarget, OperatingSystem, Question, RuntimeSpec, Warning,
    host::{HostCaps, RunnerKind},
};

pub(crate) use metadata::split_args;
pub use icons::executable_icon;
pub use title::clean_title;

/// Exact launch targets recovered by a format handler. Inno [Icons] and MSI
/// Shortcut analysis can supply these in I1 without changing discovery ranking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct LaunchHint {
    pub path: PathBuf,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct DiscoveryHints {
    pub installer_title: Option<String>,
    pub package_name: Option<String>,
    pub resolver_title: Option<String>,
    pub source_name: Option<String>,
    pub source_url: Option<Url>,
    pub ids: BTreeMap<String, String>,
    pub launch_targets: Vec<LaunchHint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct DiscoveryConfig {
    pub max_entries: u64,
    pub max_metadata_bytes: u64,
    pub max_resource_bytes: u64,
    pub tie_threshold: f32,
    pub write_icon: bool,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_metadata_bytes: 1024 * 1024,
            max_resource_bytes: 32 * 1024 * 1024,
            tie_threshold: 15.0,
            write_icon: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Discovery {
    pub title: String,
    pub ids: BTreeMap<String, String>,
    pub launch_targets: Vec<LaunchTarget>,
    pub icon: Option<PathBuf>,
    pub question: Option<Question>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("discovery I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("installed tree exceeds {limit} entries")]
    EntryLimit { limit: u64 },
    #[error("invalid discovery configuration: {0}")]
    InvalidConfig(String),
}

/// Discover installed targets using explicit host capabilities. Never runs an
/// executable, reads outside `root`, or consults host detection. The optional
/// normalized icon is the only write, under `.caskade/icon.png` via cap-std.
pub fn discover(
    root: &Path,
    host: &HostCaps,
    hints: &DiscoveryHints,
) -> Result<Discovery, DiscoverError> {
    discover_with_config(root, host, hints, DiscoveryConfig::default())
}

pub fn discover_with_config(
    root: &Path,
    host: &HostCaps,
    hints: &DiscoveryHints,
    config: DiscoveryConfig,
) -> Result<Discovery, DiscoverError> {
    if !config.tie_threshold.is_finite() || config.tie_threshold < 0.0 {
        return Err(DiscoverError::InvalidConfig(
            "tie_threshold must be finite and nonnegative".into(),
        ));
    }
    let tree = tree::Tree::scan(root, config.max_entries)?;
    let mut found = metadata::scan(&tree, hints, config);
    let fallback = hints
        .source_name
        .as_deref()
        .unwrap_or_else(|| root.file_name().and_then(|n| n.to_str()).unwrap_or("Game"));
    let mut title = found.best_title().unwrap_or_else(|| clean_title(fallback));
    if title.is_empty() {
        title = "Game".into();
    }
    let deny: DenyList = toml::from_str(include_str!("../../data/exe-denylist.toml"))
        .expect("embedded deny-list is valid TOML");
    let mut ranked: BTreeMap<PathBuf, Ranked> = BTreeMap::new();
    let candidates: Vec<&Path> = tree
        .entries
        .iter()
        .filter(|entry| !entry.directory && !deny.excludes(&entry.path))
        .map(|entry| entry.path.as_path())
        .collect();
    let mut infos = binary::inspect_all(&tree, &candidates).into_iter();
    for entry in &tree.entries {
        if entry.directory
            && entry
                .path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        {
            if entry.path.components().count() == 1 {
                add_target(
                    &mut ranked,
                    entry.path.clone(),
                    vec![],
                    None,
                    130.0,
                    "Top-level macOS app bundle",
                    binary::Kind::Macos,
                );
            }
            continue;
        }
        if entry.directory || deny.excludes(&entry.path) {
            continue;
        }
        let info = infos.next().expect("one header per candidate");
        let Some((kind, mut score, reason)) = heuristic(&tree, &entry.path, info, host) else {
            continue;
        };
        let stem = entry.path.file_stem().unwrap_or_default().to_string_lossy();
        let lower = stem.to_lowercase();
        if deny
            .lower_contains
            .iter()
            .any(|needle| lower.contains(needle))
        {
            score -= 70.0;
        }
        score -= (entry.path.components().count().saturating_sub(1) as f32) * 8.0;
        score += title_similarity(&stem, &title) * 20.0;
        score += ((entry.size.max(1) as f64).log2().min(32.0) / 32.0) as f32;
        add_target(
            &mut ranked,
            entry.path.clone(),
            vec![],
            None,
            score,
            reason,
            kind,
        );
    }
    for exact in &found.targets {
        if deny.excludes(&exact.path) {
            continue;
        }
        if let Some(path) = tree.resolve(&exact.path) {
            let kind = binary::inspect(&tree, &path).kind.unwrap_or_else(|| {
                if path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                {
                    binary::Kind::Windows
                } else {
                    binary::Kind::Native
                }
            });
            add_target(
                &mut ranked,
                path,
                exact.args.clone(),
                exact.working_dir.clone(),
                exact.score,
                &exact.reason,
                kind,
            );
        } else {
            found.warnings.push(warning(
                "missing_launch_target",
                "Metadata launch target is missing or outside the game directory",
                Some(exact.path.clone()),
            ));
        }
    }
    let mut ranked: Vec<_> = ranked.into_values().collect();
    ranked.sort_by(|a, b| {
        b.target
            .score
            .total_cmp(&a.target.score)
            .then_with(|| a.target.path.cmp(&b.target.path))
    });
    // PE metadata is a title fallback, below installer/package/plist metadata.
    if found.titles.iter().all(|(rank, _)| *rank < 60)
        && let Some(best) = ranked.first()
        && best.kind == binary::Kind::Windows
        && let Some(product) =
            binary::product_name(&tree, &best.target.path, config.max_resource_bytes)
    {
        title = product;
    }
    for item in &mut ranked {
        item.target.runtime = match (item.kind, host.os) {
            (binary::Kind::Windows, OperatingSystem::Linux) => {
                RuntimeSpec::ProtonPrefix { id: "main".into() }
            }
            (binary::Kind::Windows, OperatingSystem::Macos) => {
                RuntimeSpec::WinePrefix { id: "main".into() }
            }
            _ => RuntimeSpec::Native,
        };
    }
    if let Some(best) = ranked.first() {
        let required = match best.target.runtime {
            RuntimeSpec::ProtonPrefix { .. } => RunnerKind::Proton,
            RuntimeSpec::WinePrefix { .. } => RunnerKind::Wine,
            RuntimeSpec::Native => RunnerKind::Native,
        };
        if !host.has_runner(required) {
            found.warnings.push(warning(
                "missing_runtime",
                "The discovered target needs a runner that is not currently available",
                Some(best.target.path.clone()),
            ));
        }
    }
    let launch_targets: Vec<_> = ranked.into_iter().map(|entry| entry.target).collect();
    let question = if launch_targets.len() > 1
        && launch_targets[0].score - launch_targets[1].score <= config.tie_threshold
    {
        let top = launch_targets[0].score;
        Some(Question::ChooseLaunchTarget {
            targets: launch_targets
                .iter()
                .filter(|target| top - target.score <= config.tie_threshold)
                .cloned()
                .collect(),
        })
    } else {
        None
    };
    let icon = if config.write_icon {
        icons::normalize(
            &tree,
            &found.icons,
            &found.embedded_icons,
            &launch_targets,
            config,
            &mut found.warnings,
        )
    } else {
        None
    };
    Ok(Discovery {
        title,
        ids: found.ids,
        launch_targets,
        icon,
        question,
        warnings: found.warnings,
    })
}

struct Ranked {
    target: LaunchTarget,
    kind: binary::Kind,
}

fn add_target(
    ranked: &mut BTreeMap<PathBuf, Ranked>,
    path: PathBuf,
    args: Vec<String>,
    working_dir: Option<PathBuf>,
    score: f32,
    reason: &str,
    kind: binary::Kind,
) {
    if ranked
        .get(&path)
        .is_some_and(|old| old.target.score >= score)
    {
        return;
    }
    let working_dir = working_dir.unwrap_or_else(|| {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf()
    });
    ranked.insert(
        path.clone(),
        Ranked {
            target: LaunchTarget {
                path,
                args,
                working_dir,
                env: BTreeMap::new(),
                runtime: RuntimeSpec::Native,
                score,
                reason: reason.into(),
            },
            kind,
        },
    );
}

fn heuristic(
    tree: &tree::Tree,
    path: &Path,
    info: binary::BinaryInfo,
    host: &HostCaps,
) -> Option<(binary::Kind, f32, &'static str)> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let kind = info.kind?;
    if kind == binary::Kind::Windows {
        let stem = path.file_stem()?.to_string_lossy();
        if tree.contains(&path.with_file_name(format!("{stem}_Data"))) {
            return Some((
                kind,
                200.0,
                "Unity executable with matching _Data directory",
            ));
        }
        if path.components().count() == 1 && tree.contains(Path::new("Engine")) {
            return Some((kind, 190.0, "Unreal root bootstrapper"));
        }
        if name.ends_with("-shipping.exe") {
            return Some((kind, 90.0, "Unreal shipping executable fallback"));
        }
        return Some((
            kind,
            if info.gui { 115.0 } else { 100.0 },
            if info.gui {
                "Windows GUI executable"
            } else {
                "Windows executable"
            },
        ));
    }
    if kind == binary::Kind::Linux {
        if let Some(arch) = info.arch
            && arch != host.arch
        {
            return None;
        }
        return Some((
            kind,
            if name.ends_with(".x86_64") {
                115.0
            } else {
                100.0
            },
            "ELF executable matching host architecture",
        ));
    }
    if kind == binary::Kind::Script && (name == "start.sh" || name == "run.sh") {
        return Some((kind, 125.0, "Conventional game start script"));
    }
    if kind == binary::Kind::Macos {
        return Some((kind, 95.0, "Mach-O executable"));
    }
    None
}

fn title_similarity(stem: &str, title: &str) -> f32 {
    let clean = |value: &str| {
        value
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let stem = clean(stem);
    let title = clean(title);
    if stem.is_empty() || title.is_empty() {
        0.0
    } else if stem == title {
        1.0
    } else if stem.contains(&title) || title.contains(&stem) {
        0.5
    } else {
        0.0
    }
}

#[derive(Deserialize)]
struct DenyList {
    exclude_prefixes: Vec<String>,
    exclude_contains: Vec<String>,
    exclude_directories: Vec<String>,
    lower_contains: Vec<String>,
}
impl DenyList {
    fn excludes(&self, path: &Path) -> bool {
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        self.exclude_prefixes
            .iter()
            .any(|prefix| stem.starts_with(prefix))
            || self
                .exclude_contains
                .iter()
                .any(|needle| stem.contains(needle))
            || path.parent().is_some_and(|parent| {
                parent.components().any(|component| {
                    self.exclude_directories
                        .contains(&component.as_os_str().to_string_lossy().to_lowercase())
                })
            })
    }
}

fn warning(code: &str, message: &str, path: Option<PathBuf>) -> Warning {
    Warning {
        code: code.into(),
        message: message.into(),
        path,
    }
}
