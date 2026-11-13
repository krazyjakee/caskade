use super::{Family, MAX_HEADER_BYTES, Span, header};
use crate::{
    ArtifactId, ArtifactNode, Candidate, Fidelity, Format, HandlerId, OperatingSystem,
    PayloadPlatform, Program, Requirements, Role, RuntimeSpec, SourcePath, Step, Strategy,
    discover::DiscoveryHints,
    exec::{ExecError, Extractor, INSTALL_DIR, StepContext},
    formats::archive::{ArchiveOptions, ArchiveProbe, PayloadReader},
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
pub struct MakeselfProbe;
impl FormatProbe for MakeselfProbe {
    fn id(&self) -> &str {
        "makeself"
    }
    fn detect(&self, source: &mut ProbeSource<'_>) -> Result<Option<Detection>, ProbeError> {
        if !source.read_at(0, 2)?.starts_with(b"#!") {
            return Ok(None);
        }
        let bytes = source.read_at(0, MAX_HEADER_BYTES)?;
        let Some(family) = header::recognised(&bytes) else {
            return Ok(None);
        };
        let mut detection = Detection::new(
            if family == Family::MojoSetup {
                Format::MojoSetup
            } else {
                Format::Makeself
            },
            PayloadPlatform::Linux,
            true,
        );
        detection.role = Some(Role::Installer);
        match header::parse_header(&bytes, source.size()) {
            Ok(header) => {
                detection
                    .facts
                    .insert("makeself.offset".into(), header.spans[0].offset.into());
                if let Some(label) = header.label {
                    detection
                        .facts
                        .insert("makeself.label".into(), label.into());
                }
                if family == Family::MojoSetup {
                    detection.facts.insert(
                        "makeself.payload_format".into(),
                        serde_json::json!(Format::Zip),
                    );
                } else {
                    match super::payload_format(source, header.spans[0]) {
                        Ok(format) => {
                            detection.facts.insert(
                                "makeself.payload_format".into(),
                                serde_json::json!(format),
                            );
                        }
                        Err(error) => {
                            detection
                                .facts
                                .insert("makeself.error".into(), error.to_string().into());
                        }
                    }
                }
            }
            Err(error) => {
                detection
                    .facts
                    .insert("makeself.error".into(), error.to_string().into());
            }
        }
        Ok(Some(detection))
    }
    fn list(
        &self,
        source: &mut ProbeSource<'_>,
        node: &ArtifactNode,
    ) -> Result<Option<Vec<ArtifactNode>>, ProbeError> {
        if !matches!(node.format, Format::Makeself | Format::MojoSetup) {
            return Ok(None);
        }
        let header =
            super::read_header(source).map_err(|e| ProbeError::Io(std::io::Error::other(e)))?;
        let mut children = Vec::new();
        let spans = if header.family == Family::MojoSetup {
            vec![Span {
                offset: 0,
                length: source.size(),
            }]
        } else {
            header.spans
        };
        for span in spans {
            let format = if header.family == Family::MojoSetup {
                Format::Zip
            } else {
                super::payload_format(source, span)
                    .map_err(|e| ProbeError::Io(std::io::Error::other(e)))?
            };
            let mut view = PayloadReader::new(&mut *source, span.offset, span.length)?;
            let mut bounded = ProbeSource::new(&mut view, u64::MAX)?;
            let mut archive_node = node.clone();
            archive_node.format = format;
            for mut child in ArchiveProbe
                .list(&mut bounded, &archive_node)?
                .unwrap_or_default()
            {
                child.path =
                    crate::safety::sanitize_path(child.path.to_str().ok_or_else(|| {
                        ProbeError::Io(std::io::Error::other("non-UTF-8 shell payload name"))
                    })?)
                    .map_err(|error| ProbeError::Io(std::io::Error::other(error)))?;
                if header.family == Family::MojoSetup {
                    let Ok(path) = child.path.strip_prefix("data/noarch") else {
                        continue;
                    };
                    if path.as_os_str().is_empty() {
                        continue;
                    }
                    child.path = path.to_path_buf();
                }
                child.id = ArtifactId(format!("{}:entry:{}", node.id.0, children.len()));
                children.push(child);
            }
        }
        Ok(Some(children))
    }
}

#[derive(Default)]
pub struct MakeselfHandler;
impl Handler for MakeselfHandler {
    fn id(&self) -> HandlerId {
        HandlerId("makeself".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::Makeself
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> Result<Proposal, PlanError> {
        let mut candidates = Vec::new();
        if node.facts.contains_key("makeself.payload_format")
            && !node.facts.contains_key("makeself.error")
        {
            candidates.push(Candidate {
                handler: self.id(),
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
                warnings: vec![],
            });
        }
        if context.host.os == OperatingSystem::Linux {
            // Retained only as a fallback; the plan's execution policy filters
            // this candidate. Even --noexec invokes the untrusted shell header.
            candidates.push(Candidate {
                handler: self.id(),
                strategy: Strategy::RunSilent,
                confidence: 0.5,
                fidelity: Fidelity::Partial,
                requires: Requirements {
                    executes_code: true,
                    runtime: Some(RuntimeSpec::Native),
                    may_prompt_elevation: false,
                    interactive: false,
                    est_disk_bytes: None,
                },
                steps: vec![Step::Run {
                    program: Program::Source(SourcePath::Artifact {
                        artifact: node.id.clone(),
                        path: PathBuf::from("."),
                    }),
                    args: vec![
                        "--noexec".into(),
                        "--target".into(),
                        INSTALL_DIR.into(),
                        "--nox11".into(),
                    ],
                    env: BTreeMap::new(),
                    runtime: RuntimeSpec::Native,
                    timeout_secs: 300,
                    expect: vec![],
                }],
                warnings: vec![],
            });
        }
        Ok(Proposal {
            candidates,
            question: None,
        })
    }
}

#[derive(Default)]
pub struct MakeselfExtractor {
    sources: BTreeMap<ArtifactId, PathBuf>,
    hints: Mutex<BTreeMap<ArtifactId, DiscoveryHints>>,
}
impl MakeselfExtractor {
    pub fn new(sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self {
            sources,
            ..Self::default()
        }
    }
    pub fn take_discovery_hints(&self, id: &ArtifactId) -> Option<DiscoveryHints> {
        self.hints.lock().expect("shell hints poisoned").remove(id)
    }
}
impl Extractor for MakeselfExtractor {
    fn extract(
        &self,
        id: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> Result<(), ExecError> {
        let path = crate::exec::source_path(&self.sources, id)?;
        context.checkpoint(0)?;
        let mut file = File::open(path)?;
        let mut dir = context.open_destination(destination, file.seek(SeekFrom::End(0))?)?;
        let options = ArchiveOptions::for_step(select, strip, &context.limits);
        super::extract(&mut file, &mut dir, &options, &mut &*context).map_err(ExecError::from)?;
        self.hints
            .lock()
            .expect("shell hints poisoned")
            .insert(id.clone(), super::super::mojosetup::discovery_hints(&dir));
        Ok(())
    }
}
