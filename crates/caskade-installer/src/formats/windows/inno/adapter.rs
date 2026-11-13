use super::{
    InnoError, InnoLimits, InnoMetadata, Result, app_path, extract_path, inspect_bytes,
    split_arguments,
};
use crate::{
    ArtifactId, ArtifactNode, Format, HandlerId, PayloadPlatform, Program, Role, RuntimeSpec,
    SourcePath, Step, Warning,
    discover::{DiscoveryHints, LaunchHint},
    exec::{ExecError, Extractor, INSTALL_DIR, StepContext},
    formats::archive::ArchiveOptions,
    plan::{
        Handler, PlanContext, PlanError, Proposal,
        presets::{InstallerFamily, InstallerMode, SwitchSettings, build_args},
    },
    probe::{Detection, FormatProbe, ProbeError, ProbeSource},
};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Default)]
pub struct InnoProbe;
impl FormatProbe for InnoProbe {
    fn id(&self) -> &str {
        "inno"
    }
    fn detect(
        &self,
        source: &mut ProbeSource<'_>,
    ) -> std::result::Result<Option<Detection>, ProbeError> {
        self.detect_named(source, Path::new("setup.exe"))
    }
    fn detect_named(
        &self,
        source: &mut ProbeSource<'_>,
        name: &Path,
    ) -> std::result::Result<Option<Detection>, ProbeError> {
        if !source.read_at(0, 2)?.starts_with(b"MZ") {
            return Ok(None);
        }
        let Some(mut detection) = crate::probe::PeProbe::new().detect_named(source, name)? else {
            return Ok(None);
        };
        if detection.format != Format::Inno {
            return Ok(None);
        }
        source.seek(SeekFrom::Start(0))?;
        let limits = InnoLimits::default();
        // The normal metadata path streams through the shared probe budget.
        let parsed = ::inno::Inno::new(&mut *source);
        let metadata = match parsed {
            Ok(installer) => super::metadata::from_inno(&installer, limits),
            Err(_) => {
                if source.size() > limits.max_metadata_input_bytes {
                    return Ok(None);
                }
                let bytes = source.read_at(0, source.size() as usize)?;
                inspect_bytes(&bytes, limits)
            }
        };
        let Ok(metadata) = metadata else {
            return Ok(None);
        };
        detection.role = Some(Role::Installer);
        detection.container = true;
        detection.facts.insert(
            "inno.metadata".into(),
            serde_json::to_value(&metadata).expect("Inno metadata serializes"),
        );
        if let Some(title) = &metadata.app_name {
            detection.facts.insert("title".into(), title.clone().into());
        }
        Ok(Some(detection))
    }
    fn list(
        &self,
        _source: &mut ProbeSource<'_>,
        node: &ArtifactNode,
    ) -> std::result::Result<Option<Vec<ArtifactNode>>, ProbeError> {
        if node.format != Format::Inno {
            return Ok(None);
        }
        let Some(metadata) = metadata_from_artifact(node) else {
            return Ok(Some(vec![]));
        };
        let mut nodes = Vec::new();
        for (index, file) in metadata.files.iter().enumerate() {
            let path = app_path(&file.destination).map_err(std::io::Error::other)?;
            let Some(path) = path else {
                continue;
            };
            if !file.embedded {
                continue;
            }
            nodes.push(ArtifactNode {
                id: ArtifactId(format!("{}:inno:{index}", node.id.0)),
                path,
                format: Format::Unknown,
                platform: PayloadPlatform::Windows,
                role: Role::Data,
                size: Some(file.size),
                facts: BTreeMap::new(),
                children: vec![],
                children_complete: true,
            });
        }
        Ok(Some(nodes))
    }
}

pub fn metadata_from_artifact(node: &ArtifactNode) -> Option<InnoMetadata> {
    serde_json::from_value(node.facts.get("inno.metadata")?.clone()).ok()
}

#[derive(Default)]
pub struct InnoHandler;
impl Handler for InnoHandler {
    fn id(&self) -> HandlerId {
        HandlerId("inno".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::Inno
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        let metadata = metadata_from_artifact(node);
        // No password field exists in the shared plan DSL. An empty proposal is
        // an explicit Unsupported outcome at the application boundary; execution
        // also returns PasswordRequired before writing or spawning anything.
        if metadata.as_ref().is_none_or(|m| m.password_required) {
            return Ok(Proposal::default());
        }
        let partial = metadata
            .as_ref()
            .is_none_or(InnoMetadata::is_partial)
            .then(|| Warning { code: "inno.partial".into(), message: "Static extraction does not execute Code, Registry, INI or custom install actions".into(), path: None });
        let estimate = metadata.as_ref().and_then(InnoMetadata::unpacked_bytes);
        let expected: Vec<PathBuf> = metadata
            .as_ref()
            .into_iter()
            .flat_map(|metadata| &metadata.files)
            .filter(|file| file.embedded)
            .filter_map(|file| app_path(&file.destination).ok().flatten())
            .filter(|path| path != Path::new("."))
            .take(1)
            .collect();
        let mut candidates = vec![super::super::extract_candidate(
            self.id(),
            node,
            metadata.is_some(),
            estimate,
            partial,
        )];
        if let Some(runtime) = super::super::installer_runtime(context.host) {
            let settings = SwitchSettings::from(&context.policy.preset_args);
            debug_assert_eq!(settings.install_dir, INSTALL_DIR);
            let run = super::super::Run {
                handler: self.id(),
                program: Program::Source(SourcePath::Artifact {
                    artifact: node.id.clone(),
                    path: ".".into(),
                }),
                leading_args: vec![],
                timeout_secs: 600,
                expect: expected,
                may_prompt_elevation: metadata.as_ref().is_none_or(|m| m.needs_admin),
                est_disk_bytes: estimate,
                confidence: 1.0,
                warning_code: "inno.preset",
            };
            candidates.extend(super::super::run_candidates(
                run,
                runtime,
                &[InstallerMode::Silent, InstallerMode::Interactive],
                |mode| build_args(InstallerFamily::Inno, mode, &settings),
            )?);
        }
        Ok(Proposal {
            candidates,
            question: None,
        })
    }
}

impl InnoMetadata {
    pub fn discovery_hints(&self) -> Result<DiscoveryHints> {
        let mut hints = DiscoveryHints {
            installer_title: self.app_name.clone(),
            ..DiscoveryHints::default()
        };
        if let Some(id) = &self.app_id {
            hints.ids.insert("inno_app_id".into(), id.clone());
        }
        for icon in &self.icons {
            let Some(path) = app_path(&icon.filename)? else {
                continue;
            };
            if path == Path::new(".") {
                continue;
            }
            let working_dir = if icon.working_directory.is_empty() {
                None
            } else {
                app_path(&icon.working_directory)?
            };
            hints.launch_targets.push(LaunchHint {
                path,
                args: split_arguments(&icon.parameters)?,
                working_dir,
                reason: "Inno [Icons]".into(),
            });
        }
        Ok(hints)
    }
}

/// Family-specific native uninstall data; the Windows coordinator only records
/// this for executed candidates, and copies their actual runtime into the step.
pub fn uninstall_steps(node: &ArtifactNode) -> Vec<Step> {
    let Some(metadata) = metadata_from_artifact(node) else {
        return vec![];
    };
    let Some(path) = metadata
        .uninstaller
        .as_deref()
        .and_then(|path| app_path(path).ok().flatten())
    else {
        return vec![];
    };
    if path == Path::new(".") {
        return vec![];
    }
    vec![Step::Run {
        program: Program::Source(SourcePath::Game(path)),
        args: vec![
            "/VERYSILENT".into(),
            "/SUPPRESSMSGBOXES".into(),
            "/NORESTART".into(),
        ],
        env: BTreeMap::new(),
        runtime: RuntimeSpec::Native,
        timeout_secs: 600,
        expect: vec![],
    }]
}

#[derive(Default)]
pub struct InnoExtractor {
    sources: BTreeMap<ArtifactId, PathBuf>,
    limits: InnoLimits,
    hints: Mutex<BTreeMap<ArtifactId, DiscoveryHints>>,
}
impl InnoExtractor {
    pub fn new(sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self {
            sources,
            ..Self::default()
        }
    }
    pub fn with_limits(mut self, limits: InnoLimits) -> Self {
        self.limits = limits;
        self
    }
    pub fn take_discovery_hints(&self, artifact: &ArtifactId) -> Option<DiscoveryHints> {
        self.hints
            .lock()
            .expect("Inno hints poisoned")
            .remove(artifact)
    }
}
/// The installer plus its external slices (`<name>-1.bin`, `<name>-1a.bin`,
/// …), which hold the data of most GOG installers: the expansion-ratio budget
/// measures what was downloaded, not the small setup program alone.
fn packed_bytes(path: &Path) -> std::io::Result<u64> {
    let mut total = std::fs::metadata(path)?.len();
    let (Some(parent), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str()))
    else {
        return Ok(total);
    };
    let prefix = format!("{}-", stem.to_lowercase());
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_lowercase) else {
            continue;
        };
        let Some(slice) = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".bin"))
        else {
            continue;
        };
        let numbered = slice.trim_end_matches(|c: char| c.is_ascii_lowercase());
        if numbered.is_empty() || !numbered.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

impl Extractor for InnoExtractor {
    fn extract(
        &self,
        artifact: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> std::result::Result<(), ExecError> {
        let path = crate::exec::source_path(&self.sources, artifact)?;
        context.checkpoint(0)?;
        let mut directory = context.open_destination(destination, packed_bytes(path)?)?;
        let mut options = ArchiveOptions::default();
        options.select = select.to_vec();
        options.strip = strip.map(Path::to_path_buf);
        options.max_entries = options.max_entries.min(context.limits.max_entries as usize);
        options.max_unpacked_bytes = options
            .max_unpacked_bytes
            .min(context.limits.max_total_bytes);
        let extracted = extract_path(path, &mut directory, &options, self.limits, &mut &*context);
        context.checkpoint(0)?;
        let metadata = extracted.map_err(exec_error)?;
        let mut hints = metadata.discovery_hints().map_err(exec_error)?;
        // GOG metadata is parsed by the shared discovery scanner after extract.
        // Preserve any explicit goggame ID in the safely opened installed info.
        for file in &metadata.files {
            let Some(path) = app_path(&file.destination).map_err(exec_error)? else {
                continue;
            };
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with("goggame-") && name.ends_with(".info") {
                let path = path.to_string_lossy().replace('\\', "/");
                if let Ok(file) = directory.open_read(&path) {
                    let mut bytes = Vec::new();
                    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
                    if bytes.len() <= 1024 * 1024
                        && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
                        && let Some(id) = value.get("gameId").and_then(|value| {
                            value
                                .as_str()
                                .map(str::to_owned)
                                .or_else(|| value.as_u64().map(|id| id.to_string()))
                        })
                    {
                        hints.ids.insert("gog".into(), id);
                    }
                }
            }
        }
        self.hints
            .lock()
            .expect("Inno hints poisoned")
            .insert(artifact.clone(), hints);
        Ok(())
    }
}
fn exec_error(error: InnoError) -> ExecError {
    match error {
        InnoError::Io(error) => ExecError::Io(error),
        InnoError::Safety(error) => ExecError::Safety(error),
        InnoError::Control(error) => {
            error.into_exec_error(|error| InnoError::Control(error).to_string())
        }
        other => ExecError::Extract {
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::packed_bytes;

    #[test]
    fn packed_bytes_count_the_installers_own_slices() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, len: usize| std::fs::write(dir.path().join(name), vec![0; len]);
        write("setup_game_(1).exe", 10).unwrap();
        write("setup_game_(1)-1.bin", 100).unwrap();
        write("Setup_Game_(1)-2A.BIN", 1000).unwrap();
        write("setup_game_(1)-extra.bin", 10_000).unwrap();
        write("setup_other-1.bin", 100_000).unwrap();
        write("setup_game_(1)-3.txt", 1_000_000).unwrap();
        let packed = packed_bytes(&dir.path().join("setup_game_(1).exe")).unwrap();
        assert_eq!(packed, 1110);
    }
}
