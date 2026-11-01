use super::*;
use nsis::{
    ExecCommand, Instruction, NsisInstaller, RegistryOp,
    strings::{NsisString, StringSegment},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NsisFile {
    pub destination: String,
    pub path: Option<PathBuf>,
    pub size: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NsisShortcut {
    pub link: String,
    pub target: String,
    pub path: Option<PathBuf>,
    pub parameters: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NsisEffect {
    pub kind: String,
    pub fields: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NsisMetadata {
    pub uninstaller: bool,
    pub version: String,
    pub encoding: String,
    pub compression: String,
    pub mode: String,
    pub files: Vec<NsisFile>,
    pub shortcuts: Vec<NsisShortcut>,
    pub effects: Vec<NsisEffect>,
    pub custom_instructions: Vec<i32>,
    pub uninstallers: Vec<PathBuf>,
}
impl NsisMetadata {
    pub fn is_partial(&self) -> bool {
        !self.shortcuts.is_empty()
            || !self.effects.is_empty()
            || !self.custom_instructions.is_empty()
            || !self.uninstallers.is_empty()
            || self.files.iter().any(|f| f.path.is_none())
    }
    pub fn unpacked_bytes(&self) -> Option<u64> {
        self.files
            .iter()
            .filter(|f| f.path.is_some())
            .try_fold(0_u64, |sum, f| sum.checked_add(f.size?))
    }
    pub fn discovery_hints(&self) -> crate::discover::DiscoveryHints {
        crate::discover::DiscoveryHints {
            launch_targets: self
                .shortcuts
                .iter()
                .filter(|s| s.parameters.is_empty())
                .filter_map(|s| s.path.clone())
                .map(|path| crate::discover::LaunchHint {
                    path,
                    args: vec![],
                    working_dir: None,
                    reason: "NSIS CreateShortcut target".into(),
                })
                .collect(),
            ..Default::default()
        }
    }
}
/// Resolve only the application anchor. Other variable/shell roots are omitted;
/// traversal and absolute literal names fail rather than being rewritten.
pub fn app_path(raw: &NsisString) -> Result<Option<PathBuf>> {
    let mut text = String::new();
    let mut anchored = false;
    for (index, segment) in raw.segments.iter().enumerate() {
        match segment {
            StringSegment::Variable { name, .. }
                if index == 0 && name.eq_ignore_ascii_case("$INSTDIR") =>
            {
                anchored = true
            }
            StringSegment::Literal(value) => text.push_str(value),
            _ => return Ok(None),
        }
    }
    if anchored {
        if !text.is_empty() && !text.starts_with(['/', '\\']) {
            return Ok(None);
        }
        text = text.trim_start_matches(['/', '\\']).to_owned();
    }
    if text.is_empty() {
        return Ok(Some(PathBuf::from(".")));
    }
    Ok(Some(crate::safety::sanitize_path(&text)?))
}
pub(super) fn parse(bytes: &[u8], limits: NsisLimits) -> Result<NsisInstaller<'_>> {
    if bytes.len() as u64 > limits.max_input_bytes {
        return Err(NsisError::Limit);
    }
    // Upstream's max_decompressed_size bounds the solid payload cache, but its
    // initial header uses a separate Exact allocation from this signed field.
    // Use upstream's typed PE/header locator to cap it before that allocation.
    let overlay = nsis::addressmap::PeOverlay::from_bytes(bytes).map_err(invalid)?;
    let (_, header) = nsis::header::scan_for_first_header(overlay.overlay()).map_err(invalid)?;
    let header_size = usize::try_from(header.length_of_header()).map_err(|_| NsisError::Limit)?;
    if header_size == 0
        || header_size > limits.max_metadata_bytes.min(limits.max_decompressed_bytes)
    {
        return Err(NsisError::Limit);
    }
    let installer = NsisInstaller::builder(bytes)
        .max_decompressed_size(limits.max_decompressed_bytes)
        .parse()
        .map_err(invalid)?;
    if installer.entry_count() > limits.max_instructions {
        return Err(NsisError::Limit);
    }
    // Bound cumulative SetOutPath/save/restore growth before upstream's typed
    // iterator clones its tracked output directory for later instructions.
    let mut changes = 0_usize;
    let mut path_bytes = 0_usize;
    for entry in installer.entries() {
        let entry = entry.map_err(invalid)?;
        let opcode = installer.normalize_opcode(entry.which());
        if opcode == nsis::opcode::EW_CREATEDIR && entry.offset(1) != 0 {
            changes += 1;
            let value = installer.read_string(entry.offset(0)).map_err(invalid)?;
            path_bytes = path_bytes
                .checked_add(value.to_string().len())
                .ok_or(NsisError::Limit)?;
        } else if opcode == nsis::opcode::EW_ASSIGNVAR {
            changes += 1;
        }
        if changes > limits.max_path_updates || path_bytes > limits.max_path_bytes {
            return Err(NsisError::Limit);
        }
    }
    Ok(installer)
}
pub fn inspect_bytes(bytes: &[u8], limits: NsisLimits) -> Result<NsisMetadata> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let installer = parse(bytes, limits)?;
        analyze(&installer, limits)
    }))
    .map_err(|_| invalid("upstream NSIS parser rejected malformed input"))?
}
pub(super) fn analyze(installer: &NsisInstaller<'_>, limits: NsisLimits) -> Result<NsisMetadata> {
    let mut output = NsisMetadata {
        uninstaller: installer.is_uninstaller(),
        version: format!("{:?}", installer.version()),
        encoding: installer.string_encoding().to_string(),
        compression: format!("{:?}", installer.compression()),
        mode: format!("{:?}", installer.compression_mode()),
        files: vec![],
        shortcuts: vec![],
        effects: vec![],
        custom_instructions: vec![],
        uninstallers: vec![],
    };
    let mut custom = BTreeSet::new();
    let mut bytes = 0_usize;
    let mut paths = BTreeSet::new();
    for instruction in installer.instructions() {
        let instruction = instruction.map_err(invalid)?;
        match instruction {
            Instruction::File(file) => {
                let destination = file.dest_path().map_err(invalid)?;
                let path = app_path(&destination)?;
                if let Some(path) = &path
                    && (path == std::path::Path::new(".")
                        || !paths.insert(path.to_string_lossy().to_lowercase()))
                {
                    return Err(invalid("empty or duplicate NSIS file destination"));
                }
                let file = NsisFile {
                    destination: destination.to_string(),
                    path,
                    size: (!file.is_compressed()).then_some(file.data().len() as u64),
                };
                bytes = bytes
                    .checked_add(file.destination.len())
                    .ok_or(NsisError::Limit)?;
                output.files.push(file);
            }
            Instruction::Shortcut(shortcut) => {
                let target = shortcut.target().map_err(invalid)?;
                let shortcut = NsisShortcut {
                    link: shortcut.link_path().map_err(invalid)?.to_string(),
                    target: target.to_string(),
                    path: app_path(&target)?,
                    parameters: shortcut.parameters().map_err(invalid)?.to_string(),
                };
                bytes = bytes
                    .checked_add(
                        shortcut.link.len() + shortcut.target.len() + shortcut.parameters.len(),
                    )
                    .ok_or(NsisError::Limit)?;
                output.shortcuts.push(shortcut);
            }
            Instruction::Registry(operation) => {
                let (kind, fields) = match operation {
                    RegistryOp::Write(op) => (
                        "registry_write",
                        BTreeMap::from([
                            ("root".into(), op.root_name().into()),
                            ("key".into(), op.key().map_err(invalid)?.to_string()),
                            ("name".into(), op.value_name().map_err(invalid)?.to_string()),
                            ("type".into(), op.reg_type().to_string()),
                            ("encoded_data".into(), op.entry().offset(3).to_string()),
                            (
                                "data".into(),
                                op.data()
                                    .map(|value| value.to_string())
                                    .unwrap_or_else(|_| "<binary or unresolved>".into()),
                            ),
                        ]),
                    ),
                    RegistryOp::Delete(op) => (
                        "registry_delete",
                        BTreeMap::from([
                            ("root".into(), op.root_name().into()),
                            ("key".into(), op.key().map_err(invalid)?.to_string()),
                            ("name".into(), op.value_name().map_err(invalid)?.to_string()),
                        ]),
                    ),
                    RegistryOp::Read(_) => continue,
                };
                bytes = effect(&mut output, kind, fields, bytes)?;
            }
            Instruction::Exec(operation) => {
                let fields = match operation {
                    ExecCommand::Exec(op) => BTreeMap::from([
                        (
                            "command".into(),
                            op.command_line().map_err(invalid)?.to_string(),
                        ),
                        ("wait".into(), op.is_wait().to_string()),
                    ]),
                    ExecCommand::ShellExec(op) => BTreeMap::from([
                        ("verb".into(), op.verb().map_err(invalid)?.to_string()),
                        ("file".into(), op.file().map_err(invalid)?.to_string()),
                        (
                            "parameters".into(),
                            op.params().map_err(invalid)?.to_string(),
                        ),
                    ]),
                };
                bytes = effect(&mut output, "execute", fields, bytes)?;
            }
            Instruction::PluginCall(op) => {
                bytes = effect(
                    &mut output,
                    "plugin",
                    BTreeMap::from([
                        ("dll".into(), op.dll().map_err(invalid)?.to_string()),
                        (
                            "function".into(),
                            op.function().map_err(invalid)?.to_string(),
                        ),
                    ]),
                    bytes,
                )?;
            }
            Instruction::Uninstaller(op) => {
                if let Some(path) = app_path(&op.path().map_err(invalid)?)? {
                    output.uninstallers.push(path);
                }
            }
            Instruction::Other(entry) => {
                let opcode = installer.normalize_opcode(entry.which());
                if matches!(
                    opcode,
                    nsis::opcode::EW_WRITEINI
                        | nsis::opcode::EW_CALL
                        | nsis::opcode::EW_COPYFILES
                        | nsis::opcode::EW_FPUTS
                        | nsis::opcode::EW_FPUTWS
                        | nsis::opcode::EW_DELETEFILE
                        | nsis::opcode::EW_RMDIR
                ) {
                    custom.insert(opcode);
                }
            }
        }
        if bytes > limits.max_metadata_bytes {
            return Err(NsisError::Limit);
        }
    }
    output.custom_instructions = custom.into_iter().collect();
    Ok(output)
}
fn effect(
    output: &mut NsisMetadata,
    kind: &str,
    fields: BTreeMap<String, String>,
    bytes: usize,
) -> Result<usize> {
    let added = fields
        .iter()
        .try_fold(kind.len(), |sum, (key, value)| {
            sum.checked_add(key.len())?.checked_add(value.len())
        })
        .ok_or(NsisError::Limit)?;
    output.effects.push(NsisEffect {
        kind: kind.into(),
        fields,
    });
    bytes.checked_add(added).ok_or(NsisError::Limit)
}
