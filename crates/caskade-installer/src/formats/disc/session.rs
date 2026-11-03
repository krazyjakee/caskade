use super::{DiscError, Result, SectorReader, parse_cue};
use crate::{
    ArtifactId, ArtifactNode, HandOff, InstallOutcome, Role, Warning,
    formats::archive::{Entry, ExtractionControl, sevenzip::SevenZip},
    input::ResolvedInput,
    probe::ProbeEngine,
    safety::{Limits, SafeDir, open_staging, sanitize_path},
};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscKind {
    Pc,
    Console { platform: String },
    Unknown,
}
/// Owns the quarantined filesystem until a nested install completes. All input
/// discs remain in `files`; native insert-next-disc requests use interactive
/// fallback with the original set retained. This module never mounts a disc.
pub struct DiscSession {
    _scratch: tempfile::TempDir,
    contents: PathBuf,
    pub files: Vec<PathBuf>,
    pub entries: Vec<Entry>,
    pub artifacts: Vec<ArtifactNode>,
    pub kind: DiscKind,
    installer: Option<ArtifactId>,
    warnings: Vec<Warning>,
}
impl DiscSession {
    /// Resolve the complete set, normalize a CUE data track if necessary, list
    /// and extract via 7zz, then re-probe confined content with builtin handlers.
    /// Console classification requires filesystem evidence, never a .bin suffix.
    pub fn open(
        path: &Path,
        tool: &SevenZip,
        scratch_parent: &Path,
        limits: Limits,
        control: &mut dyn ExtractionControl,
    ) -> Result<Self> {
        control.checkpoint(0)?;
        let resolved =
            crate::input::resolve_path(path).map_err(|e| DiscError::Invalid(e.to_string()))?;
        let multi_disc = matches!(
            &resolved,
            ResolvedInput::MultiPart {
                kind: crate::input::SetKind::Discs,
                ..
            }
        );
        let (primary, members) = match resolved {
            ResolvedInput::File { path } => (path.clone(), vec![path]),
            ResolvedInput::MultiPart { primary, files, .. } => (primary, files),
            _ => return Err(DiscError::Unsupported("disc input is a folder".into())),
        };
        let parent = primary
            .parent()
            .ok_or_else(|| DiscError::Invalid("disc has no parent".into()))?;
        let files = members
            .iter()
            .map(|p| {
                let relative = p.strip_prefix(parent).map_err(|_| {
                    DiscError::Invalid("disc set escapes its input directory".into())
                })?;
                let text = relative
                    .to_str()
                    .ok_or_else(|| DiscError::Invalid("non-Unicode disc member".into()))?;
                Ok(sanitize_path(text)?)
            })
            .collect::<Result<Vec<_>>>()?;
        let scratch = tempfile::Builder::new()
            .prefix("caskade-disc-")
            .tempdir_in(scratch_parent)?;
        // Use a fresh isolated .iso name. SevenZip's general input resolver may
        // otherwise promote a .bin path back to its .cue sibling.
        let image = scratch.path().join("data.iso");
        let is_cue = primary
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("cue"));
        if is_cue {
            let mut bytes = vec![];
            File::open(&primary)?
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 {
                return Err(DiscError::Limit("cue sheet"));
            }
            let text = crate::safety::decode_name(&bytes, crate::safety::NameEncoding::Detect)?;
            let track = parse_cue(&text)?;
            let candidate = parent.join(&track.file);
            let actual = dunce::canonicalize(&candidate).or_else(|error| {
                members
                    .iter()
                    .find(|p| {
                        p.to_string_lossy()
                            .eq_ignore_ascii_case(&candidate.to_string_lossy())
                    })
                    .cloned()
                    .ok_or(error)
            })?;
            if !members.contains(&actual) || !actual.starts_with(parent) {
                return Err(DiscError::Invalid(
                    "unresolved or escaping cue track".into(),
                ));
            }
            let mut view = SectorReader::new(File::open(actual)?, track)?;
            copy_image(&mut view, &image, limits.max_total_bytes, control)?;
        } else {
            let mut source = File::open(&primary)?;
            // Raw BIN images without a cue require an explicit data-track mode.
            let mut signature = [0; 8];
            source.read_exact(&mut signature)?;
            if &signature == b"MComprHD" {
                return Err(DiscError::Unsupported(
                    "CHD needs a configured CHD decoder; image was retained".into(),
                ));
            }
            source.seek(SeekFrom::Start(0))?;
            copy_image(&mut source, &image, limits.max_total_bytes, control)?;
        }
        let contents = scratch.path().join("contents");
        fs::create_dir(&contents)?;
        let tool = tool.clone().with_limits(limits);
        let entries = tool.list(&image, control)?;
        let mut writer = SafeDir::new(
            open_staging(&contents)?,
            limits,
            image.metadata()?.len(),
            true,
        );
        let extracted = tool.extract(&image, scratch.path(), &mut writer, control)?;
        let engine = ProbeEngine::with_builtin_formats(Default::default());
        let mut artifacts = vec![];
        for entry in extracted {
            control.checkpoint(0)?;
            let name = entry
                .path
                .to_str()
                .ok_or_else(|| DiscError::Invalid("non-Unicode disc entry".into()))?;
            let mut node = engine
                .probe_reader(&mut writer.open_read(name)?, &entry.path)
                .map_err(|e| DiscError::Invalid(e.to_string()))?
                .root;
            node.id = ArtifactId(format!("disc-entry:{}", artifacts.len()));
            artifacts.push(node);
        }
        let cnf = artifacts.iter().find(|n| {
            n.path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("SYSTEM.CNF"))
        });
        let console = if let Some(cnf) = cnf {
            let text = read_small(&writer, &cnf.path)?;
            let boot = text
                .lines()
                .filter_map(|line| line.split_once('='))
                .find(|(key, _)| key.trim().eq_ignore_ascii_case("BOOT"))
                .map(|(_, value)| value.trim().to_ascii_lowercase());
            boot.is_some_and(|boot| boot.starts_with("cdrom:") && boot.contains(";1"))
        } else {
            false
        };
        let autorun = artifacts.iter().find(|n| {
            n.path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("autorun.inf"))
        });
        let autorun_target = if let Some(autorun) = autorun {
            autorun_program(&read_small(&writer, &autorun.path)?)?
        } else {
            None
        };
        let installers = artifacts
            .iter()
            .filter(|n| n.role == Role::Installer)
            .collect::<Vec<_>>();
        let installer = autorun_target
            .and_then(|target| {
                installers
                    .iter()
                    .find(|n| {
                        n.path
                            .to_string_lossy()
                            .eq_ignore_ascii_case(&target.to_string_lossy())
                    })
                    .copied()
            })
            .or_else(|| {
                if installers.len() == 1 {
                    Some(installers[0])
                } else {
                    None
                }
            })
            .map(|n| n.id.clone());
        let kind = if console {
            let probe = crate::formats::rom::RomProbe::default();
            let platform = probe
                .platforms()
                .iter()
                .find(|p| p.id == "sony_playstation")
                .ok_or_else(|| {
                    DiscError::Unsupported("PlayStation Platform definition is unavailable".into())
                })?;
            DiscKind::Console {
                platform: platform.id.clone(),
            }
        } else if autorun.is_some() || !installers.is_empty() {
            DiscKind::Pc
        } else {
            DiscKind::Unknown
        };
        let warnings = if multi_disc {
            vec![Warning {
                code: "disc_interactive_fallback".into(),
                message: "All discs are retained together. Insert-next-disc requests use the interactive installer fallback; automatic mounting is not enabled.".into(),
                path: None,
            }]
        } else {
            vec![]
        };
        Ok(Self {
            _scratch: scratch,
            contents,
            files,
            entries,
            artifacts,
            kind,
            installer,
            warnings,
        })
    }
    pub fn handoff(&self) -> Option<InstallOutcome> {
        let DiscKind::Console { platform } = &self.kind else {
            return None;
        };
        Some(InstallOutcome::HandOff(HandOff::Rom {
            platform: platform.clone(),
            files: self.files.clone(),
        }))
    }
    /// Installer bytes are owned by this session. Keep it alive throughout
    /// planning/execution, and use these sources with the normal family handler.
    pub fn installer(&self) -> Result<&ArtifactNode> {
        if self.kind != DiscKind::Pc {
            return Err(DiscError::Unsupported("disc is not a PC installer".into()));
        }
        self.installer
            .as_ref()
            .and_then(|id| self.artifacts.iter().find(|n| &n.id == id))
            .ok_or_else(|| {
                DiscError::Unsupported(
                    "PC disc has no unique validated installer; select one explicitly".into(),
                )
            })
    }
    pub fn question(&self) -> Option<crate::Question> {
        if self.kind != DiscKind::Pc || self.installer.is_some() {
            return None;
        }
        let artifacts = self
            .artifacts
            .iter()
            .filter(|node| node.role == Role::Installer)
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();
        (artifacts.len() > 1).then_some(crate::Question::ChooseInstaller { artifacts })
    }
    pub fn installer_path(&self) -> Result<PathBuf> {
        Ok(self.contents.join(&self.installer()?.path))
    }
    pub fn sources(&self) -> BTreeMap<ArtifactId, PathBuf> {
        self.artifacts
            .iter()
            .map(|n| (n.id.clone(), self.contents.join(&n.path)))
            .collect()
    }
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
    pub fn contents(&self) -> &Path {
        &self.contents
    }
}
fn copy_image(
    source: &mut (impl Read + Seek),
    destination: &Path,
    limit: u64,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    let size = source.seek(SeekFrom::End(0))?;
    if size > limit {
        return Err(DiscError::Limit("disc image"));
    }
    source.seek(SeekFrom::Start(0))?;
    control.checkpoint(size.saturating_mul(2))?;
    let mut output = File::create(destination)?;
    let mut bytes = [0; 65536];
    let mut copied = 0u64;
    loop {
        control.checkpoint(0)?;
        let n = source.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        copied = copied
            .checked_add(n as u64)
            .filter(|total| *total <= limit)
            .ok_or(DiscError::Limit("disc image"))?;
        output.write_all(&bytes[..n])?;
    }
    output.flush()?;
    Ok(())
}
fn read_small(dir: &SafeDir, path: &Path) -> Result<String> {
    let mut bytes = vec![];
    dir.open_read(
        path.to_str()
            .ok_or_else(|| DiscError::Invalid("non-Unicode disc path".into()))?,
    )?
    .take(65537)
    .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(DiscError::Limit("disc metadata"));
    }
    Ok(crate::safety::decode_name(
        &bytes,
        crate::safety::NameEncoding::Detect,
    )?)
}
fn autorun_program(text: &str) -> Result<Option<PathBuf>> {
    let mut section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.eq_ignore_ascii_case("[autorun]");
            continue;
        }
        if !section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("open") {
            continue;
        }
        let value = value.trim();
        let program = if let Some(tail) = value.strip_prefix('"') {
            tail.split_once('"')
                .ok_or_else(|| DiscError::Invalid("unterminated autorun program".into()))?
                .0
        } else {
            value.split_whitespace().next().unwrap_or("")
        };
        return Ok(Some(sanitize_path(program)?));
    }
    Ok(None)
}
