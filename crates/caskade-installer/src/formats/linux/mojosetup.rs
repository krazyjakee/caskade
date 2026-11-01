//! GOG/Humble MojoSetup game data is an appended ZIP. Bootstrap binaries,
//! shell hooks and installation scripts are never executed or installed.
use crate::{
    ArtifactNode, Candidate, Fidelity, Format, HandlerId, Requirements, Step, Strategy,
    discover::{self, Discovery, DiscoveryHints, LaunchHint},
    formats::archive::{self, ArchiveOptions, ExtractionControl},
    host::HostCaps,
    plan::{Handler, PlanContext, PlanError, Proposal},
    safety::{Limits, SafeDir, open_staging},
};
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub fn extract(
    reader: &mut (impl Read + Seek),
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> archive::Result<Vec<ArtifactNode>> {
    let header = super::makeself::read_header(reader)?;
    if header.family != super::makeself::Family::MojoSetup {
        return Err(archive::ArchiveError::Unsupported("not MojoSetup".into()));
    }
    reader.seek(SeekFrom::Start(0))?;
    let mut options = options.clone();
    // Never import MojoSetup's bootstrap or installer support trees. Keep the
    // archive's complete offset space: ZIP central offsets may be absolute or
    // relative to the appended ZIP, and zip handles both without a copy.
    if options.select.is_empty() {
        options.select = vec!["data/noarch/**".into()];
    }
    if options.strip.is_none() {
        options.strip = Some(PathBuf::from("data/noarch"));
    }
    let nodes = archive::extract(reader, Format::Zip, dir, &options, control)?;
    if nodes.is_empty() {
        return Err(archive::ArchiveError::Invalid(
            "MojoSetup ZIP contains no selected game data".into(),
        ));
    }
    Ok(nodes)
}

/// Recover GOG's line-oriented gameinfo and conventional start script through
/// the already-confined capability; no script contents are interpreted.
pub fn discovery_hints(dir: &SafeDir) -> DiscoveryHints {
    let mut hints = DiscoveryHints::default();
    if let Ok(mut file) = dir.open_read("gameinfo") {
        let mut bytes = Vec::new();
        if file
            .by_ref()
            .take(64 * 1024 + 1)
            .read_to_end(&mut bytes)
            .is_ok()
            && bytes.len() <= 64 * 1024
            && let Ok(text) = std::str::from_utf8(&bytes)
        {
            let lines: Vec<_> = text.lines().take(16).map(str::trim).collect();
            if let Some(title) = lines
                .first()
                .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
            {
                hints.installer_title = Some((*title).into());
            }
            if let Some(id) = [3, 2].into_iter().find_map(|index| {
                lines
                    .get(index)
                    .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            }) {
                hints.ids.insert("gog".into(), (*id).into());
            }
        }
    }
    if dir
        .extracted_files()
        .any(|path| path == Path::new("start.sh"))
        || dir
            .open_read("start.sh")
            .is_ok_and(|file| file.metadata().is_ok_and(|meta| meta.is_file()))
    {
        hints.launch_targets.push(LaunchHint {
            path: PathBuf::from("start.sh"),
            args: vec![],
            working_dir: Some(PathBuf::from(".")),
            reason: "GOG/MojoSetup start.sh layout".into(),
        });
    }
    hints
}
pub fn discover(root: &Path, host: &HostCaps) -> Result<Discovery, discover::DiscoverError> {
    let dir = SafeDir::new(
        open_staging(root).map_err(std::io::Error::other)?,
        Limits::default(),
        0,
        host.filesystem.case_sensitive,
    );
    discover::discover(root, host, &discovery_hints(&dir))
}

#[derive(Default)]
pub struct MojoSetupHandler;
impl MojoSetupHandler {
    pub fn new() -> Self {
        Self
    }
}
impl Handler for MojoSetupHandler {
    fn id(&self) -> HandlerId {
        HandlerId("mojosetup".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::MojoSetup
    }
    fn propose(&self, node: &ArtifactNode, _: &PlanContext<'_>) -> Result<Proposal, PlanError> {
        if node.facts.contains_key("makeself.error") {
            return Ok(Proposal::default());
        }
        Ok(Proposal {
            candidates: vec![Candidate {
                handler: self.id(),
                strategy: Strategy::Extract,
                confidence: 1.0,
                fidelity: Fidelity::Exact,
                requires: Requirements::default(),
                steps: vec![Step::Extract {
                    from: node.id.clone(),
                    select: vec!["data/noarch/**".into()],
                    strip: Some(PathBuf::from("data/noarch")),
                    to: PathBuf::from("."),
                }],
                warnings: vec![],
            }],
            question: None,
        })
    }
}
