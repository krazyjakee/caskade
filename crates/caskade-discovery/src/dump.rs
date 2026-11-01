//! Allowlisted, bounded metadata export into an anonymized fixture (#70).
//! Nothing executes and no source files are changed. Databases are logically
//! rebuilt, never copied with deleted rows/free pages or raw authentication data.
mod binary;
#[cfg(feature = "sqlite")]
mod database;
mod registry;
mod scrub;
mod select;
use crate::{
    OperatingSystem,
    env::{HostEnv, KnownFolder},
    registry::{RegistryHive, RegistryView},
};
use scrub::Scrubber;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub type Result<T> = std::result::Result<T, DumpError>;
pub(super) const MAX_FILE: u64 = 2_000_000;
pub(super) const MAX_INPUT: u64 = 64 * 1024 * 1024;
pub(super) const MAX_TOTAL: u64 = 16_000_000;
#[derive(Debug, thiserror::Error)]
pub enum DumpError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("invalid dump metadata: {0}")]
    Invalid(String),
    #[error("dump budget exceeded: {0}")]
    Limit(&'static str),
}
#[derive(Debug, Serialize, Deserialize)]
pub struct DumpReport {
    pub schema_version: u32,
    pub directory: PathBuf,
    pub files: Vec<String>,
    pub bytes: u64,
    pub warnings: Vec<String>,
}
#[derive(Serialize)]
struct Machine {
    schema_version: u32,
    os: OperatingSystem,
    now_unix_secs: u64,
    env: BTreeMap<String, String>,
    known_folders: BTreeMap<KnownFolder, String>,
}

/// Export explicit launcher metadata only. Tests pass an injected synthetic
/// HostEnv; calling this with HostEnv::live is an explicit CLI/user operation.
pub fn dump_anonymized(env: &HostEnv, destination: &Path) -> Result<DumpReport> {
    if destination.exists() || fs::symlink_metadata(destination).is_ok() {
        return Err(DumpError::Invalid("destination already exists".into()));
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".caskade-discovery-dump-")
        .tempdir_in(parent)?;
    fs::create_dir(stage.path().join("root"))?;
    fs::create_dir(stage.path().join("registry"))?;
    let scrub = Scrubber::new(env);
    let mut report = DumpReport {
        schema_version: 1,
        directory: destination.to_owned(),
        files: vec![],
        bytes: 0,
        warnings: vec![],
    };
    let mut folders = BTreeMap::new();
    for folder in [
        KnownFolder::Home,
        KnownFolder::ConfigHome,
        KnownFolder::DataHome,
        KnownFolder::CacheHome,
        KnownFolder::StateHome,
        KnownFolder::Documents,
        KnownFolder::Desktop,
        KnownFolder::ProgramData,
        KnownFolder::AppData,
        KnownFolder::LocalAppData,
        KnownFolder::ProgramFiles,
        KnownFolder::ProgramFilesX86,
        KnownFolder::StartMenu,
        KnownFolder::CommonStartMenu,
        KnownFolder::UserLibrary,
        KnownFolder::SystemLibrary,
        KnownFolder::Applications,
        KnownFolder::UserApplications,
    ] {
        if let Some(path) = env.known_folder(folder) {
            folders.insert(folder, scrub.virtual_path(path)?);
        }
    }
    let home = folders[&KnownFolder::Home].clone();
    let mut vars = BTreeMap::from([(
        if env.os() == OperatingSystem::Windows {
            "USERPROFILE"
        } else {
            "HOME"
        }
        .to_owned(),
        home,
    )]);
    for (variable, folder) in [
        ("XDG_CONFIG_HOME", KnownFolder::ConfigHome),
        ("XDG_DATA_HOME", KnownFolder::DataHome),
        ("ProgramData", KnownFolder::ProgramData),
        ("APPDATA", KnownFolder::AppData),
        ("LOCALAPPDATA", KnownFolder::LocalAppData),
        ("ProgramFiles", KnownFolder::ProgramFiles),
        ("ProgramFiles(x86)", KnownFolder::ProgramFilesX86),
    ] {
        if let Some(path) = folders.get(&folder) {
            vars.insert(variable.to_owned(), path.clone());
        }
    }
    let machine = Machine {
        schema_version: 1,
        os: env.os(),
        now_unix_secs: env
            .now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        env: vars,
        known_folders: folders,
    };
    write(
        stage.path(),
        Path::new("machine.toml"),
        toml::to_string(&machine)
            .map_err(|e| DumpError::Invalid(e.to_string()))?
            .as_bytes(),
        &mut report,
        &scrub,
    )?;
    let sources = select::files(env)?;
    let installed_ids: BTreeSet<u32> = sources
        .iter()
        .filter_map(|path| {
            path.file_name()?
                .to_str()?
                .strip_prefix("appmanifest_")?
                .strip_suffix(".acf")?
                .parse()
                .ok()
        })
        .collect();
    for source in sources {
        let relative = Path::new("root").join(scrub.relative(&source)?);
        let file = fs::File::open(&source)?;
        if file.metadata()?.len() > MAX_INPUT {
            return Err(DumpError::Limit("metadata input bytes"));
        }
        let mut bytes = vec![];
        file.take(MAX_INPUT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_INPUT {
            return Err(DumpError::Limit("growing metadata input"));
        }
        let name = source
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let data = if source
            .extension()
            .is_some_and(|extension| extension == "reg")
        {
            let hive = if name == "system.reg" {
                RegistryHive::LocalMachine
            } else {
                RegistryHive::CurrentUser
            };
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| DumpError::Invalid("Wine registry file is not UTF-8".into()))?;
            let values = crate::registry::WineRegistry::parse(
                text,
                hive,
                RegistryView::Native,
                crate::registry::Limits::default(),
            )?;
            let mut text = registry::export(&values, hive, RegistryView::Registry64, &scrub)?;
            let narrow = registry::export(&values, hive, RegistryView::Registry32, &scrub)?;
            let narrow = narrow.replace(
                "\n[HKEY_LOCAL_MACHINE\\Software\\",
                "\n[HKEY_LOCAL_MACHINE\\Software\\Wow6432Node\\",
            );
            text.push_str(
                narrow
                    .strip_prefix("Windows Registry Editor Version 5.00\n")
                    .unwrap_or(&narrow),
            );
            text.into_bytes()
        } else if bytes.starts_with(b"SQLite format 3\0") {
            #[cfg(feature = "sqlite")]
            {
                let target = stage.path().join(&relative);
                fs::create_dir_all(target.parent().expect("file parent"))?;
                database::export(&source, &target, &scrub)?;
                let bytes = fs::read(&target)?;
                fs::remove_file(&target)?;
                bytes
            }
            #[cfg(not(feature = "sqlite"))]
            {
                report
                    .warnings
                    .push(format!("{}: sqlite feature disabled", relative.display()));
                continue;
            }
        } else if name == "shortcuts.vdf" {
            binary::vdf(&bytes, &scrub)?
        } else if name == "appinfo.vdf" {
            binary::appinfo(&bytes, &scrub, &installed_ids)?
        } else if source
            .extension()
            .is_some_and(|extension| extension == "acf" || extension == "vdf")
        {
            binary::text_vdf(&bytes, &scrub)?
        } else if name == "product.db" {
            binary::product(&bytes, &scrub)?
        } else if source
            .extension()
            .is_some_and(|extension| extension == "lnk")
        {
            binary::link(&bytes, &scrub)?
        } else if source
            .extension()
            .is_some_and(|extension| extension == "plist")
        {
            let mut value =
                crate::parsers::plist::parse(&bytes, &crate::parsers::Limits::default())
                    .map_err(|e| DumpError::Invalid(e.to_string()))?;
            scrub_plist(&mut value, &scrub);
            let mut bytes = vec![];
            value
                .to_writer_xml(&mut bytes)
                .map_err(|e| DumpError::Invalid(e.to_string()))?;
            bytes
        } else if source
            .extension()
            .is_some_and(|extension| extension == "json" || extension == "item")
        {
            let mut value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| DumpError::Invalid(e.to_string()))?;
            scrub.json(&mut value);
            serde_json::to_vec_pretty(&value).map_err(|e| DumpError::Invalid(e.to_string()))?
        } else {
            scrub
                .text_file(
                    std::str::from_utf8(&bytes)
                        .map_err(|_| DumpError::Invalid("unknown binary metadata format".into()))?,
                )
                .into_bytes()
        };
        write(stage.path(), &relative, &data, &mut report, &scrub)?;
    }
    for (name, hive, view) in [
        ("hkcu.reg", RegistryHive::CurrentUser, RegistryView::Native),
        (
            "hklm32.reg",
            RegistryHive::LocalMachine,
            RegistryView::Registry32,
        ),
        (
            "hklm64.reg",
            RegistryHive::LocalMachine,
            RegistryView::Registry64,
        ),
    ] {
        let text = registry::export(env.registry(), hive, view, &scrub)?;
        if text.contains("\n[") {
            write(
                stage.path(),
                &Path::new("registry").join(name),
                text.as_bytes(),
                &mut report,
                &scrub,
            )?;
        }
    }
    report.files.sort();
    report.files.dedup();
    fs::rename(stage.path(), destination)?;
    Ok(report)
}
fn write(
    root: &Path,
    relative: &Path,
    bytes: &[u8],
    report: &mut DumpReport,
    scrub: &Scrubber,
) -> Result<()> {
    if bytes.len() as u64 > MAX_FILE {
        return Err(DumpError::Limit("output file bytes"));
    }
    let size = report.bytes.saturating_add(bytes.len() as u64);
    if size > MAX_TOTAL || report.files.len() >= 4096 {
        return Err(DumpError::Limit("output corpus"));
    }
    scrub.validate(bytes)?;
    let target = root.join(relative);
    fs::create_dir_all(target.parent().expect("file parent"))?;
    if target.exists() {
        return Err(DumpError::Invalid(
            "anonymized metadata paths collide".into(),
        ));
    }
    fs::write(target, bytes)?;
    report.bytes = size;
    report
        .files
        .push(relative.to_string_lossy().replace('\\', "/"));
    Ok(())
}
fn scrub_plist(value: &mut plist::Value, scrub: &Scrubber) {
    match value {
        plist::Value::String(text) => *text = scrub.text(text),
        plist::Value::Data(bytes) => bytes.clear(),
        plist::Value::Array(values) => {
            for value in values {
                scrub_plist(value, scrub);
            }
        }
        plist::Value::Dictionary(values) => {
            for (key, value) in values {
                if scrub::account(key) || scrub::secret(key) {
                    *value = plist::Value::String(scrub.field(key, "fixture"));
                } else {
                    scrub_plist(value, scrub);
                }
            }
        }
        _ => {}
    }
}
