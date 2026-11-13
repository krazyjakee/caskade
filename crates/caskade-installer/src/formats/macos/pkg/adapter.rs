use super::{Package, PkgError, Result, XarLimits, inspect, invalid};
use crate::{
    ArtifactId, ArtifactNode, Candidate, DelegateCommand, Fidelity, Format, HandlerId,
    OperatingSystem, PayloadPlatform, Requirements, Role, RuntimeSpec, Step, Strategy, Warning,
    exec::{ExecError, Extractor, StepContext, source_argument},
    formats::archive::{self, ArchiveOptions, ExtractionControl},
    plan::{Handler, PlanContext, PlanError, Proposal},
    probe::{Detection, FormatProbe, ProbeError, ProbeSource},
    safety::SafeDir,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
#[derive(Default)]
pub struct PkgProbe;
impl FormatProbe for PkgProbe {
    fn id(&self) -> &str {
        "macos-pkg"
    }
    fn detect(
        &self,
        source: &mut ProbeSource<'_>,
    ) -> std::result::Result<Option<Detection>, ProbeError> {
        if source.read_at(0, 4)? != b"xar!" {
            return Ok(None);
        }
        let mut detection = Detection::new(Format::Pkg, PayloadPlatform::Macos, true);
        detection.role = Some(Role::Installer);
        match inspect(source, XarLimits::default()) {
            Ok((_, package)) => {
                detection
                    .facts
                    .insert("macos.pkg".into(), serde_json::to_value(package).unwrap());
            }
            Err(error) => {
                detection
                    .facts
                    .insert("macos.pkg_error".into(), error.to_string().into());
            }
        }
        Ok(Some(detection))
    }
    fn list(
        &self,
        source: &mut ProbeSource<'_>,
        node: &ArtifactNode,
    ) -> std::result::Result<Option<Vec<ArtifactNode>>, ProbeError> {
        if node.format != Format::Pkg {
            return Ok(None);
        }
        let (xar, _) = inspect(source, XarLimits::default())
            .map_err(|e| ProbeError::Io(std::io::Error::other(e)))?;
        Ok(Some(
            xar.toc
                .entries
                .iter()
                .enumerate()
                .map(|(index, entry)| ArtifactNode {
                    id: ArtifactId(format!("{}:pkg:{index}", node.id.0)),
                    path: entry.path.clone(),
                    format: if entry.directory {
                        Format::Directory
                    } else {
                        Format::Unknown
                    },
                    platform: PayloadPlatform::Macos,
                    role: Role::Data,
                    size: Some(entry.size),
                    facts: BTreeMap::new(),
                    children: vec![],
                    children_complete: !entry.directory,
                })
                .collect(),
        ))
    }
}
// Fixed program; every input remains a separately quoted argv item. Only the
// system authentication dialog may ask the user for administrator credentials.
const ADMIN_INSTALL_SCRIPT: &str = "on run argv\nset commandText to \"\"\nrepeat with argument in argv\nset commandText to commandText & quoted form of (argument as text) & \" \"\nend repeat\ndo shell script commandText with administrator privileges\nend run";

#[derive(Default)]
pub struct PkgHandler;
impl Handler for PkgHandler {
    fn id(&self) -> HandlerId {
        HandlerId("macos-pkg".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::Pkg
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        let Some(value) = node.facts.get("macos.pkg") else {
            return Ok(Proposal::default());
        };
        let package: Package =
            serde_json::from_value(value.clone()).map_err(|_| PlanError::Handler {
                handler: self.id(),
                message: "invalid package metadata fact".into(),
            })?;
        let mut candidates = Vec::new();
        if !package.requires_delegate {
            let scripts = package
                .components
                .iter()
                .any(|component| component.has_scripts);
            candidates.push(Candidate {
                handler: self.id(),
                strategy: Strategy::Extract,
                confidence: 1.0,
                fidelity: if scripts {
                    Fidelity::Partial
                } else {
                    Fidelity::Exact
                },
                requires: Requirements::default(),
                steps: vec![Step::Extract {
                    from: node.id.clone(),
                    select: vec![],
                    strip: None,
                    to: ".".into(),
                }],
                warnings: if scripts {
                    vec![Warning {
                        code: "package_scripts_skipped".into(),
                        message: "Unreferenced package script archive is skipped without execution"
                            .into(),
                        path: None,
                    }]
                } else {
                    vec![]
                },
            });
        }
        if context.host.os == OperatingSystem::Macos
            && let Some(installer) = context.host.packages.managers.get("installer")
            && let Some(pkgutil) = context.host.packages.managers.get("pkgutil")
        {
            let direct = vec![
                "-pkg".into(),
                source_argument(&node.id),
                "-target".into(),
                "/".into(),
            ];
            let command = if context.host.packages.root {
                Some((installer.to_string_lossy().into_owned(), direct))
            } else {
                context
                    .host
                    .packages
                    .managers
                    .get("osascript")
                    .map(|osascript| {
                        let mut args = vec![
                            "-e".into(),
                            ADMIN_INSTALL_SCRIPT.into(),
                            "--".into(),
                            installer.to_string_lossy().into_owned(),
                        ];
                        args.extend(direct);
                        (osascript.to_string_lossy().into_owned(), args)
                    })
            };
            if let Some((tool, args)) = command {
                let uninstall = package
                    .components
                    .iter()
                    .map(|component| DelegateCommand {
                        tool: pkgutil.to_string_lossy().into_owned(),
                        args: vec!["--forget".into(), component.identifier.clone()],
                        only_if: None,
                    })
                    .collect();
                candidates.push(Candidate {
                    handler: self.id(),
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
                        tool,
                        args,
                        uninstall,
                        only_if: None,
                    }],
                    warnings: vec![Warning {
                        code: "package_system_install".into(),
                        message: "Uses macOS installer; receipts are recorded, installed files require native uninstall tracking".into(),
                        path: None,
                    }],
                });
            }
        }
        Ok(Proposal {
            candidates,
            question: None,
        })
    }
}
struct Controlled<'a, 'b, R> {
    inner: R,
    control: &'a RefCell<&'b mut dyn ExtractionControl>,
}
impl<R: Read> Read for Controlled<'_, '_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let size = bytes.len().min(64 * 1024);
        self.control
            .borrow_mut()
            .checkpoint(size as u64)
            .map_err(std::io::Error::other)?;
        self.inner.read(&mut bytes[..size])
    }
}
impl<R: Seek> Seek for Controlled<'_, '_, R> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(position)
    }
}
pub fn extract(
    mut source: impl Read + Seek,
    dir: &mut SafeDir,
    control: &mut dyn ExtractionControl,
    max_bytes: u64,
) -> Result<Package> {
    let control = RefCell::new(control);
    let mut source = Controlled {
        inner: &mut source,
        control: &control,
    };
    let (xar, package) = inspect(
        &mut source,
        XarLimits {
            max_member_bytes: max_bytes,
            ..XarLimits::default()
        },
    )?;
    if package.requires_delegate {
        return Err(PkgError::RequiresDelegate(
            "system target, active scripts or product Distribution".into(),
        ));
    }
    for component in &package.components {
        let entry = xar
            .entry(&component.path.join("Payload"))
            .ok_or_else(|| invalid("missing component Payload"))?;
        let mut member = BufReader::new(xar.open_entry(&mut source, entry)?);
        let prefix = member.fill_buf()?;
        let (pbzx, compression) = (
            prefix.starts_with(b"pbzx"),
            crate::probe::compression(prefix),
        );
        let payload: Box<dyn Read + '_> = if pbzx {
            Box::new(super::pbzx::Pbzx::new(member, max_bytes)?)
        } else if let Some(format) = compression {
            archive::decoded(member, format, &ArchiveOptions::default())?
        } else {
            Box::new(member)
        };
        super::cpio::extract(payload, component, dir, &control, max_bytes)?;
    }
    let bundles: Vec<_> = dir
        .extracted_files()
        .filter(|path| path.ends_with("Contents/Info.plist"))
        .filter_map(|path| path.parent().and_then(Path::parent).map(Path::to_path_buf))
        .collect();
    if bundles.is_empty() {
        return Err(invalid("package has no complete app bundle"));
    }
    for bundle in bundles {
        super::super::app::installed_metadata(dir, &bundle)
            .map_err(|error| invalid(error.to_string()))?;
    }
    Ok(package)
}
#[derive(Default)]
pub struct PkgExtractor {
    sources: BTreeMap<ArtifactId, PathBuf>,
}
impl PkgExtractor {
    pub fn new(sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self { sources }
    }
}
fn exec_error(error: PkgError) -> ExecError {
    match error {
        PkgError::Io(error) => error.into(),
        PkgError::Safety(error) => error.into(),
        PkgError::Control(error) => error.into(),
        other => ExecError::Extract {
            message: other.to_string(),
        },
    }
}
impl Extractor for PkgExtractor {
    fn extract(
        &self,
        artifact: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> std::result::Result<(), ExecError> {
        if !select.is_empty() || strip.is_some() {
            return Err(ExecError::Extract {
                message: "package expansion keeps whole app bundles".into(),
            });
        }
        let source = crate::exec::source_path(&self.sources, artifact)?;
        let file = std::fs::File::open(source)?;
        let mut dir = context.open_destination(destination, file.metadata()?.len())?;
        let result = extract(
            file,
            &mut dir,
            &mut &*context,
            context.limits.max_total_bytes,
        );
        context.checkpoint(0)?;
        result.map_err(exec_error)?;
        Ok(())
    }
}
