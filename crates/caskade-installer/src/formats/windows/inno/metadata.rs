use super::{InnoError, InnoLimits, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Cursor, Read, Seek},
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMetadata {
    pub destination: String,
    pub size: u64,
    pub embedded: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconMetadata {
    pub name: String,
    pub filename: String,
    pub parameters: String,
    pub working_directory: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMetadata {
    pub filename: String,
    pub parameters: String,
    pub working_directory: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryMetadata {
    pub hive: String,
    pub key: String,
    pub name: String,
    pub value: String,
    pub value_type: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IniMetadata {
    pub filename: String,
    pub section: String,
    pub key: String,
    pub value: String,
}

/// Static installer records. Strings containing Inno constants remain data;
/// only validated {app} paths may become game-relative filesystem operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InnoMetadata {
    pub setup_version: String,
    pub app_name: Option<String>,
    pub app_version: Option<String>,
    pub app_publisher: Option<String>,
    pub app_id: Option<String>,
    pub password_required: bool,
    pub has_code: bool,
    pub has_custom_steps: bool,
    pub needs_admin: bool,
    pub external_slices: bool,
    pub uninstaller: Option<String>,
    pub files: Vec<FileMetadata>,
    pub directories: Vec<String>,
    pub icons: Vec<IconMetadata>,
    pub run: Vec<RunMetadata>,
    pub registry: Vec<RegistryMetadata>,
    pub ini: Vec<IniMetadata>,
    pub parser: String,
}
impl InnoMetadata {
    pub fn is_partial(&self) -> bool {
        self.has_code
            || self.has_custom_steps
            || !self.run.is_empty()
            || !self.registry.is_empty()
            || !self.ini.is_empty()
            || self.files.iter().any(|file| {
                !file.embedded || super::app_path(&file.destination).ok().flatten().is_none()
            })
    }
    pub fn unpacked_bytes(&self) -> Option<u64> {
        self.files
            .iter()
            .try_fold(0_u64, |sum, file| sum.checked_add(file.size))
    }
    pub(super) fn validate(self, limits: InnoLimits) -> Result<Self> {
        let entries = self
            .files
            .len()
            .saturating_add(self.directories.len())
            .saturating_add(self.icons.len())
            .saturating_add(self.run.len())
            .saturating_add(self.registry.len())
            .saturating_add(self.ini.len());
        if entries > limits.max_entries {
            return Err(InnoError::Limit("metadata entries"));
        }
        if self.unpacked_bytes().is_none() {
            return Err(InnoError::Limit("unpacked size overflow"));
        }
        Ok(self)
    }
}
fn text(value: Option<&str>) -> String {
    value.unwrap_or_default().to_owned()
}

// Upstream exposes the raw Pascal bytes as a UTF-8 view. Decode a valid
// byte view according to Unicode mode before treating it as registry text.
fn registry_text(value: Option<&str>, unicode: bool, kind: &str) -> String {
    let value = value.unwrap_or_default();
    if unicode
        && matches!(kind, "String" | "ExpandString" | "MultiString")
        && value.len().is_multiple_of(2)
    {
        let words = value
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&words);
    }
    value.to_owned()
}
pub(super) fn from_inno<R: Read + Seek>(
    installer: &::inno::Inno<R>,
    limits: InnoLimits,
) -> Result<InnoMetadata> {
    let header = installer.header();
    let locations = installer.file_locations();
    let files = installer
        .file_entries()
        .iter()
        .map(|file| {
            let location = locations.get(file.location() as usize);
            FileMetadata {
                destination: text(file.destination()),
                size: location.map_or(file.external_size(), |location| {
                    location.uncompressed_size()
                }),
                embedded: location.is_some(),
            }
        })
        .collect();
    let uninstaller = if header.is_uninstallable() {
        let directory = header.uninstall_files_dir().unwrap_or("{app}");
        Some(format!(
            "{}\\unins000.exe",
            directory.trim_end_matches(['/', '\\'])
        ))
    } else {
        None
    };
    InnoMetadata {
        setup_version: installer.version().to_string(),
        app_name: header.app_name().map(str::to_owned),
        app_version: header.app_version().map(str::to_owned),
        app_publisher: header.app_publisher().map(str::to_owned),
        app_id: header.app_id().map(str::to_owned),
        password_required: header.flags.intersects(
            ::inno::header::HeaderFlags::PASSWORD | ::inno::header::HeaderFlags::ENCRYPTION_USED,
        ),
        has_code: header
            .compiled_code_text()
            .is_some_and(|code| !code.is_empty()),
        has_custom_steps: !installer.tasks().is_empty()
            || !installer.components().is_empty()
            || installer
                .file_entries()
                .iter()
                .any(|file| file.location() == u32::MAX),
        needs_admin: matches!(
            header.privileges_required(),
            ::inno::header::PrivilegeLevel::Admin | ::inno::header::PrivilegeLevel::PowerUser
        ),
        external_slices: installer.setup_loader().data_offset() == 0,
        uninstaller,
        files,
        directories: installer
            .directories()
            .iter()
            .filter_map(|d| d.name().map(str::to_owned))
            .collect(),
        icons: installer
            .icon_entries()
            .iter()
            .map(|icon| IconMetadata {
                name: text(icon.name()),
                filename: text(icon.filename()),
                parameters: text(icon.parameters()),
                working_directory: text(icon.working_directory()),
            })
            .collect(),
        run: installer
            .run_entries()
            .iter()
            .map(|run| RunMetadata {
                filename: text(run.name()),
                parameters: text(run.parameters()),
                working_directory: text(run.working_directory()),
            })
            .collect(),
        registry: installer
            .registry_entries()
            .iter()
            .map(|entry| RegistryMetadata {
                hive: format!("{:?}", entry.registry_root()),
                key: text(entry.key()),
                name: text(entry.name()),
                value: registry_text(
                    entry.value(),
                    installer.version().is_unicode(),
                    entry.r#type().as_str(),
                ),
                value_type: format!("{:?}", entry.r#type()),
            })
            .collect(),
        ini: installer
            .ini_entries()
            .iter()
            .map(|entry| IniMetadata {
                filename: entry.file_path().into(),
                section: text(entry.section_name()),
                key: text(entry.key_name()),
                value: text(entry.value()),
            })
            .collect(),
        parser: "inno/0.7.1".into(),
    }
    .validate(limits)
}

fn from_innospect(bytes: &[u8], limits: InnoLimits) -> Result<InnoMetadata> {
    let installer = innospect::InnoInstaller::from_bytes(bytes).map_err(|error| match error {
        innospect::Error::ResourceLimit { what } => InnoError::Limit(what),
        error => InnoError::Invalid(error.to_string()),
    })?;
    let header = installer
        .header()
        .ok_or_else(|| InnoError::Invalid("missing setup header".into()))?;
    let locations = installer.file_locations();
    InnoMetadata {
        setup_version: format!(
            "{}.{}.{}",
            installer.version().a,
            installer.version().b,
            installer.version().c
        ),
        app_name: header.app_name().map(str::to_owned),
        app_version: header.app_version().map(str::to_owned),
        app_publisher: header.app_publisher().map(str::to_owned),
        app_id: header.app_id().map(str::to_owned),
        password_required: installer.is_encrypted(),
        has_code: installer
            .compiled_code_bytes()
            .is_some_and(|bytes| !bytes.is_empty()),
        has_custom_steps: !installer.tasks().is_empty() || !installer.components().is_empty(),
        needs_admin: true,
        external_slices: installer.offset_table().offset_setup1 == 0,
        // The fallback is used for encrypted 6.4 metadata. Never guess a native
        // uninstaller when the primary metadata parser cannot validate it.
        uninstaller: None,
        files: installer
            .files()
            .iter()
            .map(|file| FileMetadata {
                destination: file.destination.clone(),
                size: locations
                    .get(file.location_index as usize)
                    .map_or(file.external_size, |location| location.original_size),
                embedded: (file.location_index as usize) < locations.len(),
            })
            .collect(),
        directories: installer
            .directories()
            .iter()
            .map(|d| d.name.clone())
            .collect(),
        icons: installer
            .icons()
            .iter()
            .map(|icon| IconMetadata {
                name: icon.name.clone(),
                filename: icon.filename.clone(),
                parameters: icon.parameters.clone(),
                working_directory: icon.working_dir.clone(),
            })
            .collect(),
        run: installer
            .run_entries()
            .iter()
            .map(|run| RunMetadata {
                filename: run.name.clone(),
                parameters: run.parameters.clone(),
                working_directory: run.working_dir.clone(),
            })
            .collect(),
        registry: installer
            .registry_entries()
            .iter()
            .map(|entry| RegistryMetadata {
                hive: entry.hive.to_string(),
                key: entry.subkey.clone(),
                name: entry.value_name.clone(),
                value: entry.value_text().into_owned(),
                value_type: entry
                    .value_type
                    .map_or_else(|| "unknown".into(), |kind| kind.to_string()),
            })
            .collect(),
        ini: installer
            .ini_entries()
            .iter()
            .map(|entry| IniMetadata {
                filename: entry.inifile.clone(),
                section: entry.section.clone(),
                key: entry.key.clone(),
                value: entry.value.clone(),
            })
            .collect(),
        parser: "innospect/0.2.0".into(),
    }
    .validate(limits)
}

/// Pure, bounded byte-entry point for fuzzing and metadata-only callers.
pub fn inspect_bytes(bytes: &[u8], limits: InnoLimits) -> Result<InnoMetadata> {
    if bytes.len() as u64 > limits.max_metadata_input_bytes {
        return Err(InnoError::Limit("metadata input"));
    }
    match ::inno::Inno::new(Cursor::new(bytes)) {
        Ok(installer) => from_inno(&installer, limits),
        Err(::inno::error::InnoError::Io(error))
            if error.kind() == std::io::ErrorKind::FileTooLarge =>
        {
            Err(InnoError::Limit("upstream metadata allocation"))
        }
        Err(_) => from_innospect(bytes, limits),
    }
}

pub fn inspect_path(path: &Path, limits: InnoLimits) -> Result<InnoMetadata> {
    let mut file = File::open(path)?;
    let mut source = crate::probe::ProbeSource::new(&mut file, limits.max_metadata_input_bytes)
        .map_err(|error| InnoError::Invalid(error.to_string()))?;
    let installer = ::inno::Inno::new(&mut source);
    match installer {
        Ok(installer) => from_inno(&installer, limits),
        Err(::inno::error::InnoError::Io(error))
            if error.kind() == std::io::ErrorKind::FileTooLarge =>
        {
            Err(InnoError::Limit("upstream metadata allocation"))
        }
        Err(_) => {
            let mut bytes = Vec::new();
            File::open(path)?
                .take(limits.max_metadata_input_bytes.saturating_add(1))
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > limits.max_metadata_input_bytes {
                return Err(InnoError::Limit("fallback metadata input"));
            }
            from_innospect(&bytes, limits)
        }
    }
}
