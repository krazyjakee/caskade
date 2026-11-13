use super::{PackageError, PackageInfo, common, desktop};
use crate::{
    ArtifactId, ArtifactNode, Candidate, DelegateCheck, DelegateCommand, Fidelity, Format,
    HandlerId, OperatingSystem, PayloadPlatform, Requirements, Role, RuntimeSpec, Step, Strategy,
    Warning,
    exec::{ExecError, Extractor, StepContext, source_argument},
    formats::archive::{self, ArchiveOptions, PayloadReader},
    plan::{Handler, PlanContext, PlanError, Proposal},
    probe::{Detection, FormatProbe, ProbeError, ProbeSource},
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Default)]
pub struct PackageProbe;
fn inspect(reader: &mut (impl std::io::Read + Seek), format: Format) -> super::Result<PackageInfo> {
    if format == Format::Deb {
        Ok(super::inspect(reader)?.info)
    } else {
        Ok(super::super::rpm::inspect(reader)?.info)
    }
}
impl FormatProbe for PackageProbe {
    fn id(&self) -> &str {
        "linux-package"
    }
    fn detect(&self, source: &mut ProbeSource<'_>) -> Result<Option<Detection>, ProbeError> {
        let head = source.read_at(0, 24)?;
        let format = if head.starts_with(b"!<arch>\ndebian-binary") {
            Format::Deb
        } else if head.starts_with(b"\xed\xab\xee\xdb") {
            Format::Rpm
        } else {
            return Ok(None);
        };
        let mut detection = Detection::new(format, PayloadPlatform::Linux, true);
        detection.role = Some(Role::Installer);
        match inspect(source, format) {
            Ok(info) => {
                detection
                    .facts
                    .insert("linux.package".into(), serde_json::to_value(info).unwrap());
            }
            Err(error) => {
                detection
                    .facts
                    .insert("linux.package_error".into(), error.to_string().into());
            }
        }
        Ok(Some(detection))
    }
    fn list(
        &self,
        source: &mut ProbeSource<'_>,
        node: &ArtifactNode,
    ) -> Result<Option<Vec<ArtifactNode>>, ProbeError> {
        if !matches!(node.format, Format::Deb | Format::Rpm) {
            return Ok(None);
        }
        let entries = (|| -> super::Result<_> {
            if node.format == Format::Deb {
                let package = super::inspect(source)?;
                Ok(archive::list(
                    PayloadReader::new(&mut *source, package.data.offset, package.data.length)?,
                    package.format,
                    &ArchiveOptions::default(),
                )?)
            } else {
                super::super::rpm::list(source, &ArchiveOptions::default())
            }
        })()
        .map_err(|e| ProbeError::Io(std::io::Error::other(e)))?;
        let mut children = Vec::new();
        for (index, entry) in entries.into_iter().enumerate() {
            let path = crate::safety::sanitize_path(&entry.name)
                .map_err(|e| ProbeError::Io(std::io::Error::other(e)))?;
            let directory = entry.kind == archive::EntryKind::Directory;
            children.push(ArtifactNode {
                id: ArtifactId(format!("{}:entry:{index}", node.id.0)),
                path,
                format: if directory {
                    Format::Directory
                } else {
                    Format::Unknown
                },
                platform: PayloadPlatform::Linux,
                role: Role::Data,
                size: Some(entry.size),
                facts: BTreeMap::new(),
                children: vec![],
                children_complete: !directory,
            });
        }
        Ok(Some(children))
    }
}

#[derive(Default)]
pub struct DebHandler;
impl Handler for DebHandler {
    fn id(&self) -> HandlerId {
        HandlerId("deb".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::Deb
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> Result<Proposal, PlanError> {
        propose(self.id(), node, context)
    }
}
pub(crate) fn propose(
    handler: HandlerId,
    node: &ArtifactNode,
    context: &PlanContext<'_>,
) -> Result<Proposal, PlanError> {
    let Some(info) = node
        .facts
        .get("linux.package")
        .and_then(|value| serde_json::from_value::<PackageInfo>(value.clone()).ok())
    else {
        return Ok(Proposal::default());
    };
    let dependencies = common::dependency_report(&info, context.host);
    super::common::package_name(&info.name).map_err(|error| PlanError::Handler {
        handler: handler.clone(),
        message: error.to_string(),
    })?;
    let mut candidates = Vec::new();
    if dependencies.missing.is_empty() && !info.absolute_paths {
        let warnings = if info.maintainer_scripts.is_empty() {
            vec![]
        } else {
            vec![Warning {
                code: "maintainer_scripts_skipped".into(),
                message: format!(
                    "Extraction skips package scripts: {}",
                    info.maintainer_scripts.join(", ")
                ),
                path: None,
            }]
        };
        candidates.push(Candidate {
            handler: handler.clone(),
            strategy: Strategy::Extract,
            confidence: 1.0,
            fidelity: Fidelity::Exact,
            requires: Requirements::default(),
            steps: vec![Step::Extract {
                from: node.id.clone(),
                select: vec![],
                strip: None,
                to: PathBuf::from("."),
            }],
            warnings,
        });
    }
    if context.host.os == OperatingSystem::Linux {
        let managers: &[&str] = if info.format == Format::Deb {
            &["apt"]
        } else {
            &["dnf", "zypper"]
        };
        if let Some((kind, manager)) = managers.iter().find_map(|kind| {
            context
                .host
                .packages
                .managers
                .get(*kind)
                .map(|path| (*kind, path))
        }) {
            // DNF5 rejects an end-of-options marker after its subcommand.
            // The source token resolves to an absolute file path at execution;
            // package identities are validated before building any commands.
            let mut install = match kind {
                "zypper" => vec![
                    "--non-interactive".into(),
                    "install".into(),
                    "--".into(),
                    source_argument(&node.id),
                ],
                "dnf" => vec!["install".into(), "-y".into(), source_argument(&node.id)],
                _ => vec![
                    "install".into(),
                    "-y".into(),
                    "--".into(),
                    source_argument(&node.id),
                ],
            };
            let mut uninstall = match kind {
                "zypper" => vec![
                    "--non-interactive".into(),
                    "remove".into(),
                    "--".into(),
                    info.name.clone(),
                ],
                "dnf" => vec!["remove".into(), "-y".into(), info.name.clone()],
                _ => vec!["remove".into(), "-y".into(), "--".into(), info.name.clone()],
            };
            // A package removed by hand needs no removal: apt and zypper
            // fail on a package they can't find.
            let query = if kind == "apt" { "dpkg-query" } else { "rpm" };
            let installed =
                context
                    .host
                    .packages
                    .managers
                    .contains_key(query)
                    .then(|| DelegateCheck {
                        tool: query.into(),
                        args: vec![
                            if kind == "apt" { "--status" } else { "-q" }.into(),
                            "--".into(),
                            info.name.clone(),
                        ],
                    });
            // Tools are named, as in an install record, and found on PATH
            // when they run; pkexec gets the manager's path, which its
            // prompt shows.
            let tool = if context.host.packages.root {
                Some(kind)
            } else {
                context
                    .host
                    .packages
                    .managers
                    .contains_key("pkexec")
                    .then_some("pkexec")
            };
            if let Some(tool) = tool {
                if !context.host.packages.root {
                    install.insert(0, manager.to_string_lossy().into_owned());
                    uninstall.insert(0, manager.to_string_lossy().into_owned());
                }
                let tool = tool.to_owned();
                candidates.push(Candidate {
                    handler,
                    strategy: Strategy::Delegate,
                    confidence: 1.0,
                    fidelity: Fidelity::Native,
                    requires: Requirements {
                        executes_code: true,
                        runtime: Some(RuntimeSpec::Native),
                        may_prompt_elevation: !context.host.packages.root,
                        interactive: false,
                        est_disk_bytes: None,
                    },
                    steps: vec![Step::Delegate {
                        tool: tool.clone(),
                        args: install,
                        uninstall: vec![DelegateCommand {
                            tool,
                            args: uninstall,
                            only_if: installed,
                        }],
                        only_if: None,
                    }],
                    warnings: vec![],
                });
            }
        }
    }
    Ok(Proposal {
        candidates,
        question: None,
    })
}

#[derive(Default)]
pub struct PackageExtractor {
    sources: BTreeMap<ArtifactId, PathBuf>,
    references: Mutex<BTreeMap<ArtifactId, Vec<super::GeneratedReference>>>,
}
impl PackageExtractor {
    pub fn new(sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self {
            sources,
            ..Self::default()
        }
    }
    pub fn references(&self, artifact: &ArtifactId) -> Option<Vec<super::GeneratedReference>> {
        self.references
            .lock()
            .expect("package references poisoned")
            .get(artifact)
            .cloned()
    }
    pub fn take_references(&self, id: &ArtifactId) -> Option<Vec<super::GeneratedReference>> {
        self.references
            .lock()
            .expect("package references poisoned")
            .remove(id)
    }
}
impl Extractor for PackageExtractor {
    fn extract(
        &self,
        id: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> Result<(), ExecError> {
        if !select.is_empty() || strip.is_some() {
            return Err(ExecError::Extract {
                message: "package extraction requires its complete relative layout".into(),
            });
        }
        let path = crate::exec::source_path(&self.sources, id)?;
        context.checkpoint(0)?;
        let mut file = File::open(path)?;
        let size = file.seek(SeekFrom::End(0))?;
        let mut dir = context.open_destination(destination, size)?;
        let options = ArchiveOptions {
            max_entries: context.limits.max_entries.min(100_000) as usize,
            max_unpacked_bytes: context.limits.max_total_bytes,
            max_expansion_ratio: context.limits.max_expansion_ratio,
            ..ArchiveOptions::default()
        };
        let mut magic = [0; 4];
        file.seek(SeekFrom::Start(0))?;
        std::io::Read::read_exact(&mut file, &mut magic)?;
        let extracted = if &magic == b"\xed\xab\xee\xdb" {
            super::super::rpm::extract(&mut file, &mut dir, &options, &mut &*context)
        } else {
            super::extract(&mut file, &mut dir, &options, &mut &*context)
        };
        extracted.map_err(exec_error)?;
        desktop::check_absolute_payload_references(&dir, &mut &*context).map_err(exec_error)?;
        let references = desktop::rewrite_desktop_files(
            &mut dir,
            &context.final_destination_path(destination)?,
            &mut &*context,
        )
        .map_err(exec_error)?;
        self.references
            .lock()
            .expect("package references poisoned")
            .insert(id.clone(), references);
        Ok(())
    }
}
fn exec_error(error: PackageError) -> ExecError {
    match error {
        PackageError::Io(error) => ExecError::Io(error),
        PackageError::Safety(error) => ExecError::Safety(error),
        PackageError::Archive(error) => {
            error.into_exec_error(|error| PackageError::Archive(error).to_string())
        }
        other => ExecError::Extract {
            message: other.to_string(),
        },
    }
}
