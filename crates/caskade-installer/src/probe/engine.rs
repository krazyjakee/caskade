use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    ArtifactId, ArtifactNode, Format, PayloadPlatform, ProbeSource, ReadSeek, ReadStats, Role,
    magic,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ProbeLimits {
    pub max_depth: u32,
    pub max_read_bytes: u64,
    pub max_children: u64,
}

impl Default for ProbeLimits {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_read_bytes: 8 * 1024 * 1024,
            max_children: 100_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Probe {
    pub root: ArtifactNode,
    pub io: ReadStats,
}

/// Metadata returned by a registered format probe, before it has listed children.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Detection {
    pub format: Format,
    pub platform: PayloadPlatform,
    pub role: Option<Role>,
    pub facts: BTreeMap<String, serde_json::Value>,
    pub container: bool,
}

impl Detection {
    pub fn new(format: Format, platform: PayloadPlatform, container: bool) -> Self {
        Self {
            format,
            platform,
            container,
            role: None,
            facts: BTreeMap::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("probe I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("probe read budget exceeded ({limit} bytes)")]
    ReadBudgetExceeded { limit: u64 },
    #[error("read offset {offset} is outside a {size}-byte source")]
    InvalidOffset { offset: u64, size: u64 },
    #[error("container depth limit exceeded ({limit})")]
    DepthLimit { limit: u32 },
    #[error("container entry limit exceeded ({limit})")]
    EntryLimit { limit: u64 },
    #[error("no registered probe can list {format:?}")]
    ListingUnavailable { format: Format },
    #[error("artifact {id:?} was not found")]
    ArtifactNotFound { id: ArtifactId },
    #[error("container listing returned duplicate artifact id {id:?}")]
    DuplicateArtifactId { id: ArtifactId },
    #[error("probe input is not a regular file or directory: {path:?}")]
    UnsupportedFileType { path: PathBuf },
}

/// Format modules register static detection and optional lazy listing. Returning
/// None declines the input; no extraction or execution takes place in this API.
pub trait FormatProbe: Send + Sync {
    fn id(&self) -> &str;
    /// Optional input-relative filename for extension-based formats. Existing
    /// probes remain byte-only unless they explicitly override this method.
    fn detect_named(
        &self,
        source: &mut ProbeSource<'_>,
        _name: &Path,
    ) -> Result<Option<Detection>, ProbeError> {
        self.detect(source)
    }
    fn detect(&self, _source: &mut ProbeSource<'_>) -> Result<Option<Detection>, ProbeError> {
        Ok(None)
    }
    fn list(
        &self,
        _source: &mut ProbeSource<'_>,
        _node: &ArtifactNode,
    ) -> Result<Option<Vec<ArtifactNode>>, ProbeError> {
        Ok(None)
    }
}

pub struct ProbeEngine {
    limits: ProbeLimits,
    probes: Vec<Box<dyn FormatProbe>>,
    /// Built-in directory classification (macOS `.app` bundles); minimal
    /// engines keep every directory as a plain `Directory` node.
    #[cfg(feature = "macos-formats")]
    builtin_directories: bool,
}

impl Default for ProbeEngine {
    fn default() -> Self {
        Self::new(ProbeLimits::default())
    }
}

impl ProbeEngine {
    /// Opt-in built-in registry; `new` and `default` retain minimal magic-only
    /// identification for callers that supply their own format plugins.
    pub fn with_builtin_formats(limits: ProbeLimits) -> Self {
        let mut engine = Self::new(limits);
        #[cfg(feature = "macos-formats")]
        {
            engine.builtin_directories = true;
        }
        // Engine order is first-match: format probes that refine a PE or CFB
        // container run before the generic PE probe.
        #[cfg(feature = "windows-formats")]
        {
            // Burn bundles are checked first: one bounded header/UX read
            // instead of repeated overlay scans by the other PE probes.
            engine.register(Box::new(crate::formats::windows::burn::BurnProbe::new()));
            engine.register(Box::new(crate::formats::windows::inno::InnoProbe::new()));
            engine.register(Box::new(crate::formats::windows::msi::MsiProbe::new()));
            engine.register(Box::new(crate::formats::windows::nsis::NsisProbe::new()));
            engine.register(Box::new(super::PeProbe::new()));
        }
        engine.register(Box::new(crate::formats::archive::sfx::SfxProbe::new()));
        #[cfg(feature = "linux-formats")]
        engine.register(Box::new(
            crate::formats::linux::makeself::MakeselfProbe::new(),
        ));
        #[cfg(feature = "linux-formats")]
        engine.register(Box::new(crate::formats::linux::deb::PackageProbe::new()));
        // Console discs are classified from their filesystem before the
        // suffix/header ROM catalogue sees shared .iso/.bin/.cue names.
        #[cfg(feature = "disc")]
        engine.register(Box::new(crate::formats::disc::DiscProbe));
        #[cfg(feature = "macos-formats")]
        engine.register(Box::new(crate::formats::macos::pkg::PkgProbe::new()));
        #[cfg(feature = "macos-formats")]
        engine.register(Box::new(crate::formats::macos::dmg::DmgProbe::new()));
        #[cfg(feature = "linux-formats")]
        engine.register(Box::new(crate::formats::linux::flatpak::FlatpakProbe));
        engine.register(Box::new(crate::formats::rom::RomProbe::default()));
        engine.register(Box::new(crate::formats::archive::ArchiveProbe::new()));
        engine
    }
    pub fn new(limits: ProbeLimits) -> Self {
        Self {
            limits,
            probes: vec![],
            #[cfg(feature = "macos-formats")]
            builtin_directories: false,
        }
    }
    pub fn register(&mut self, probe: Box<dyn FormatProbe>) {
        self.probes.push(probe);
    }

    pub fn probe_reader(
        &self,
        reader: &mut dyn ReadSeek,
        name: &Path,
    ) -> Result<Probe, ProbeError> {
        let mut source = ProbeSource::new(reader, self.limits.max_read_bytes)?;
        let mut detected = None;
        for probe in &self.probes {
            if let Some(mut detection) = probe.detect_named(&mut source, name)? {
                detection
                    .facts
                    .insert("probe.handler".into(), probe.id().into());
                detected = Some(detection);
                break;
            }
        }
        let detection = match detected {
            Some(detected) => detected,
            None => magic::detect(&mut source)?,
        };
        let role = detection
            .role
            .unwrap_or_else(|| magic::guess_role(name, detection.format));
        Ok(Probe {
            root: ArtifactNode {
                id: ArtifactId("source:0".into()),
                path: name.to_path_buf(),
                format: detection.format,
                platform: detection.platform,
                role,
                size: Some(source.size()),
                facts: detection.facts,
                children: vec![],
                children_complete: !detection.container,
            },
            io: source.stats(),
        })
    }

    /// Edge adapter for an already-resolved local input. Directories remain
    /// unlisted; no directory walk occurs during initial identification.
    pub fn probe_path(&self, path: &Path) -> Result<Probe, ProbeError> {
        let metadata = fs::metadata(path)?;
        let name = path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        #[cfg(feature = "macos-formats")]
        if metadata.is_dir()
            && self.builtin_directories
            && name
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
            && let Ok(root) = crate::formats::macos::app::probe_bundle(path)
        {
            return Ok(Probe {
                root,
                io: ReadStats::default(),
            });
        }
        if metadata.is_dir() {
            return Ok(Probe {
                root: ArtifactNode {
                    id: ArtifactId("source:0".into()),
                    path: name,
                    format: Format::Directory,
                    platform: PayloadPlatform::Unknown,
                    role: Role::Unknown,
                    size: None,
                    facts: BTreeMap::new(),
                    children: vec![],
                    children_complete: false,
                },
                io: ReadStats::default(),
            });
        }
        if !metadata.is_file() {
            return Err(ProbeError::UnsupportedFileType {
                path: path.to_path_buf(),
            });
        }
        self.probe_reader(&mut fs::File::open(path)?, &name)
    }

    /// List an existing node, deriving its depth from the supplied tree rather
    /// than trusting a caller-provided depth counter. `reader` must expose that
    /// node's container bytes. The returned listing is validated transactionally
    /// and is attached only when it passes depth, count and identifier checks.
    pub fn list_children(
        &self,
        reader: &mut dyn ReadSeek,
        root: &mut ArtifactNode,
        id: &ArtifactId,
    ) -> Result<ReadStats, ProbeError> {
        let mut all_ids = BTreeSet::new();
        validate_tree(root, 0, self.limits, &mut all_ids)?;
        let (node, depth) = find_node(root, id, 0)
            .ok_or_else(|| ProbeError::ArtifactNotFound { id: id.clone() })?;
        if node.children_complete {
            return Ok(ReadStats::default());
        }
        if depth >= self.limits.max_depth {
            return Err(ProbeError::DepthLimit {
                limit: self.limits.max_depth,
            });
        }
        let mut source = ProbeSource::new(reader, self.limits.max_read_bytes)?;
        for probe in &self.probes {
            if let Some(children) = probe.list(&mut source, node)? {
                // A partial prior listing is replaced, so its identifiers may
                // legitimately recur. Other branches must remain disjoint.
                for child in &node.children {
                    remove_ids(child, &mut all_ids);
                }
                for child in &children {
                    validate_tree(child, depth + 1, self.limits, &mut all_ids)?;
                }
                if all_ids.len() as u64 > self.limits.max_children.saturating_add(1) {
                    return Err(ProbeError::EntryLimit {
                        limit: self.limits.max_children,
                    });
                }
                node.children = children;
                node.children_complete = true;
                return Ok(source.stats());
            }
        }
        Err(ProbeError::ListingUnavailable {
            format: node.format,
        })
    }
}

fn remove_ids(node: &ArtifactNode, ids: &mut BTreeSet<ArtifactId>) {
    ids.remove(&node.id);
    for child in &node.children {
        remove_ids(child, ids);
    }
}

fn find_node<'a>(
    node: &'a mut ArtifactNode,
    id: &ArtifactId,
    depth: u32,
) -> Option<(&'a mut ArtifactNode, u32)> {
    if node.id == *id {
        return Some((node, depth));
    }
    for child in &mut node.children {
        if let Some(found) = find_node(child, id, depth + 1) {
            return Some(found);
        }
    }
    None
}

fn validate_tree(
    node: &ArtifactNode,
    depth: u32,
    limits: ProbeLimits,
    ids: &mut BTreeSet<ArtifactId>,
) -> Result<(), ProbeError> {
    if depth > limits.max_depth {
        return Err(ProbeError::DepthLimit {
            limit: limits.max_depth,
        });
    }
    if !ids.insert(node.id.clone()) {
        return Err(ProbeError::DuplicateArtifactId {
            id: node.id.clone(),
        });
    }
    if ids.len() as u64 > limits.max_children.saturating_add(1) {
        return Err(ProbeError::EntryLimit {
            limit: limits.max_children,
        });
    }
    for child in &node.children {
        validate_tree(child, depth + 1, limits, ids)?;
    }
    Ok(())
}
