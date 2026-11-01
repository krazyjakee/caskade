//! Install macOS bundles as data. Quarantine is preserved by default; no
//! xattr-removal command or executable from the bundle is ever run here.
mod adapter;
mod appledouble;
pub use appledouble::parse as parse_appledouble;
pub use appledouble::restore_zip_attributes;
mod binary;
mod copy;
mod metadata;
use crate::{
    ArtifactId, ArtifactNode, Format, PayloadPlatform, Role,
    safety::{SafeDir, sanitize_path},
};
pub use adapter::{AppExtractor, AppHandler};
use cap_std::{ambient_authority, fs::Dir};
pub use copy::copy_bundle;
pub use metadata::{MAX_PLIST_BYTES, parse_info};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bundle I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Safety(#[from] crate::safety::SafetyError),
    #[error(transparent)]
    Archive(#[from] crate::formats::archive::ArchiveError),
    #[error("invalid app bundle: {0}")]
    Invalid(String),
    #[error("bundle metadata exceeds limits")]
    MetadataLimit,
}
pub type Result<T> = std::result::Result<T, AppError>;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleMetadata {
    pub name: String,
    pub identifier: Option<String>,
    pub executable: PathBuf,
    pub icon: Option<PathBuf>,
}
fn bounded_info(mut file: impl Read) -> Result<BundleMetadata> {
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_PLIST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    parse_info(&bytes)
}
pub fn inspect_bundle(path: &Path) -> Result<BundleMetadata> {
    let root = Dir::open_ambient_dir(path, ambient_authority())?;
    let metadata = bounded_info(root.open("Contents/Info.plist")?)?;
    if !root.metadata(&metadata.executable)?.is_file() {
        return Err(AppError::Invalid("bundle executable is not a file".into()));
    }
    Ok(metadata)
}
/// Directory edge adapter for the opt-in builtin probe registry. Only fixed
/// Info.plist and executable paths are read; this does not walk the bundle.
pub fn probe_bundle(path: &Path) -> Result<ArtifactNode> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AppError::Invalid("bundle filename encoding".into()))?;
    let name = sanitize_path(name)?;
    if !name
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
    {
        return Err(AppError::Invalid("bundle must end in .app".into()));
    }
    let metadata = inspect_bundle(path)?;
    let mut facts = BTreeMap::new();
    facts.insert(
        "macos.bundle".into(),
        serde_json::to_value(metadata).unwrap(),
    );
    facts.insert("probe.handler".into(), "macos-app".into());
    Ok(ArtifactNode {
        id: ArtifactId("source:0".into()),
        path: name,
        format: Format::AppBundle,
        platform: PayloadPlatform::Macos,
        role: Role::Application,
        size: None,
        facts,
        children: vec![],
        children_complete: false,
    })
}
/// Inspect installed metadata through the staging capability, without ambient
/// reads or executing the launch target.
pub fn installed_metadata(dir: &SafeDir, bundle: &Path) -> Result<BundleMetadata> {
    let bundle = sanitize_path(
        bundle
            .to_str()
            .ok_or(crate::safety::SafetyError::InvalidEncoding)?,
    )?;
    let info = bundle.join("Contents/Info.plist");
    let metadata = bounded_info(
        dir.open_read(
            info.to_str()
                .ok_or(crate::safety::SafetyError::InvalidEncoding)?,
        )?,
    )?;
    dir.open_read(
        bundle
            .join(&metadata.executable)
            .to_str()
            .ok_or(crate::safety::SafetyError::InvalidEncoding)?,
    )?;
    Ok(metadata)
}
/// Existing discovery handles plist titles/IDs and ICNS normalization into
/// .caskade/icon.png. Keep this entry point consistent for folder and ZIP input.
pub fn discover(
    root: &Path,
    host: &crate::host::HostCaps,
) -> std::result::Result<crate::discover::Discovery, crate::discover::DiscoverError> {
    crate::discover::discover(root, host, &crate::discover::DiscoveryHints::default())
}
