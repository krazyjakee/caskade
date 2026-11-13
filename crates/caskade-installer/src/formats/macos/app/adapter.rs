use super::{AppError, BundleMetadata, copy_bundle, installed_metadata};
use crate::{
    ArtifactId, ArtifactNode, Candidate, Fidelity, Format, HandlerId, Requirements, Step, Strategy,
    exec::{ExecError, Extractor, StepContext},
    formats::archive::{self, ArchiveOptions},
    plan::{Handler, PlanContext, PlanError, Proposal},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Default)]
pub struct AppHandler;
fn bundle_roots(node: &ArtifactNode) -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    for child in &node.children {
        let mut parts = child.path.components();
        if let Some(first) = parts.next() {
            let path = PathBuf::from(first.as_os_str());
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("app"))
                && parts.as_path() == Path::new("Contents/Info.plist")
            {
                roots.insert(path);
            }
        }
    }
    roots.into_iter().collect()
}
impl Handler for AppHandler {
    fn id(&self) -> HandlerId {
        HandlerId("macos-app".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::AppBundle
            || (node.format == Format::Zip && !bundle_roots(node).is_empty())
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        _: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        let roots = bundle_roots(node);
        let select = if node.format == Format::Zip {
            roots
                .iter()
                .map(|p| format!("{}/**", globset::escape(&p.to_string_lossy())))
                .chain(std::iter::once("__MACOSX/**".into()))
                .collect()
        } else {
            vec![]
        };
        Ok(Proposal {
            candidates: vec![Candidate {
                handler: self.id(),
                strategy: Strategy::Extract,
                confidence: 1.0,
                fidelity: Fidelity::Exact,
                requires: Requirements::default(),
                steps: vec![Step::Extract {
                    from: node.id.clone(),
                    select,
                    strip: None,
                    to: PathBuf::from("."),
                }],
                warnings: vec![],
            }],
            question: None,
        })
    }
}
#[derive(Default)]
pub struct AppExtractor {
    sources: BTreeMap<ArtifactId, PathBuf>,
    bundles: Mutex<BTreeMap<ArtifactId, BTreeMap<PathBuf, BundleMetadata>>>,
}
impl AppExtractor {
    pub fn new(sources: BTreeMap<ArtifactId, PathBuf>) -> Self {
        Self {
            sources,
            ..Self::default()
        }
    }
    pub fn take_bundles(&self, artifact: &ArtifactId) -> Option<BTreeMap<PathBuf, BundleMetadata>> {
        self.bundles
            .lock()
            .expect("bundle metadata poisoned")
            .remove(artifact)
    }
}
fn error(error: AppError) -> ExecError {
    match error {
        AppError::Io(e) => e.into(),
        AppError::Safety(e) => e.into(),
        AppError::Archive(e) => e.into(),
        other => ExecError::Extract {
            message: other.to_string(),
        },
    }
}
impl Extractor for AppExtractor {
    fn extract(
        &self,
        artifact: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> std::result::Result<(), ExecError> {
        let source = crate::exec::source_path(&self.sources, artifact)?;
        if strip.is_some() {
            return Err(ExecError::Extract {
                message: "app bundles preserve their complete layout".into(),
            });
        }
        let source_metadata = std::fs::symlink_metadata(source)?;
        let packed = if source_metadata.is_dir() {
            super::copy::measure(source).map_err(error)?
        } else {
            source_metadata.len()
        };
        let mut safe = context.open_destination(destination, packed)?;
        let mut control = context;
        let mut bundles = BTreeMap::new();
        if source_metadata.is_dir() {
            if !select.is_empty() {
                return Err(ExecError::Extract {
                    message: "folder app input does not accept archive selections".into(),
                });
            }
            let root = copy_bundle(source, &mut safe, &mut control).map_err(error)?;
            bundles.insert(
                root.clone(),
                installed_metadata(&safe, &root).map_err(error)?,
            );
        } else if source_metadata.is_file() {
            let options = ArchiveOptions {
                select: select.to_vec(),
                ..ArchiveOptions::default()
            };
            let entries = archive::list(std::fs::File::open(source)?, Format::Zip, &options)
                .map_err(|e| error(e.into()))?;
            let mut roots = BTreeSet::new();
            for entry in &entries {
                let path = crate::safety::sanitize_path(&entry.name)?;
                let mut parts = path.components();
                if let Some(first) = parts.next() {
                    let root = PathBuf::from(first.as_os_str());
                    if root
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("app"))
                        && parts.as_path() == Path::new("Contents/Info.plist")
                    {
                        roots.insert(root);
                    }
                }
            }
            if roots.is_empty() {
                return Err(ExecError::Extract {
                    message: "ZIP has no complete top-level app bundle".into(),
                });
            }
            archive::extract(
                std::fs::File::open(source)?,
                Format::Zip,
                &mut safe,
                &options,
                &mut control,
            )
            .map_err(|e| error(e.into()))?;
            super::restore_zip_attributes(&mut safe).map_err(error)?;
            for root in roots {
                bundles.insert(
                    root.clone(),
                    installed_metadata(&safe, &root).map_err(error)?,
                );
            }
            let mut directories: Vec<_> = entries
                .iter()
                .filter(|e| e.kind == archive::EntryKind::Directory)
                .collect();
            directories.sort_by_key(|entry| {
                std::cmp::Reverse(Path::new(&entry.name).components().count())
            });
            for entry in directories {
                if safe.has_entry(&entry.name)? {
                    safe.set_entry_mode(&entry.name, entry.mode)?;
                }
            }
        } else {
            return Err(ExecError::Extract {
                message: "app input is not a regular bundle or ZIP".into(),
            });
        }
        self.bundles
            .lock()
            .expect("bundle metadata poisoned")
            .insert(artifact.clone(), bundles);
        context.checkpoint(0)
    }
}
